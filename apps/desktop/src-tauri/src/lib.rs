//! Prisma AIRS Desktop v2 native host.
//!
//! The host owns window/origin/schema admission for the two application
//! commands, the packaged main window and its navigation policy, and native
//! menus. Domain authority lives in `desktop-core`; the host creates the core
//! once and derives the caller principal from the invoking window, never from
//! request JSON.

use desktop_core::wire::{
    DispatchRequest, DispatchResponse, MenuCommand, ObservationUpdate, ObserveRequest,
    ObserveResponse, WireScope,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use desktop_core::{AuthorityHandle, Core, CoreConfig, Observation, Refusal, Update, ViewScope};
use tauri::async_runtime::JoinHandle;
use tauri::ipc::{Channel, InvokeBody, JavaScriptChannelId, Request};
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{
    Manager, Runtime, State, Url, Webview, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};
use tokio::sync::broadcast;

pub mod adapters;
#[cfg(all(feature = "test-fixtures", target_os = "macos"))]
pub mod fixture_capture;
#[cfg(target_os = "macos")]
pub mod keychain;
mod menu;

/// Label of the single packaged window allowed to call application commands.
pub const MAIN_WINDOW: &str = "main";

/// Application commands registered in the permission manifest (build.rs).
pub const APP_COMMANDS: [&str; 2] = ["desktop_dispatch", "desktop_observe"];

/// Live shell observations a single webview may hold; further observations
/// are refused (`Busy`) until one is closed or the page reloads.
pub const MAX_OBSERVATIONS_PER_WEBVIEW: usize = 8;

struct ObservationTask {
    id: u64,
    webview: String,
    token: String,
    handle: JoinHandle<()>,
}

#[derive(Default)]
struct Observations {
    next_id: u64,
    tasks: VecDeque<ObservationTask>,
}

/// Native host state managed by Tauri.
pub struct Host {
    operator: AuthorityHandle,
    menu: broadcast::Sender<MenuCommand>,
    observations: Arc<Mutex<Observations>>,
    zoom: Mutex<f64>,
}

impl Host {
    pub fn new(core: &Core) -> Self {
        let (menu, _) = broadcast::channel(32);
        Self {
            operator: core.operator(),
            menu,
            observations: Arc::default(),
            zoom: Mutex::new(1.0),
        }
    }

    /// Forwards a native menu selection to every live shell observation.
    pub fn menu_command(&self, command: MenuCommand) {
        let _ = self.menu.send(command);
    }

    /// Live observation tasks held for a webview.
    pub fn active_observations(&self, webview: &str) -> usize {
        self.observations
            .lock()
            .map(|o| {
                o.tasks
                    .iter()
                    .filter(|t| t.webview == webview && !t.handle.inner().is_finished())
                    .count()
            })
            .unwrap_or(0)
    }

    /// Core subscribers still registered (after pruning retired ones).
    pub fn core_observers(&self) -> usize {
        self.operator.observer_count()
    }

    /// Ends every observation of a webview: used on page reload and window
    /// destruction. Business work is unaffected.
    pub fn retire_webview(&self, webview: &str) {
        if let Ok(mut observations) = self.observations.lock() {
            observations.tasks.retain(|task| {
                if task.webview == webview {
                    task.handle.abort();
                    false
                } else {
                    true
                }
            });
        }
    }

    fn close(&self, webview: &str, token: &str) {
        if let Ok(mut observations) = self.observations.lock() {
            observations.tasks.retain(|task| {
                if task.webview == webview && task.token == token {
                    task.handle.abort();
                    false
                } else {
                    true
                }
            });
        }
    }

    /// Registers an observation task. Re-observing on the same channel token
    /// replaces that task. At the per-webview cap the new observation is
    /// refused (`Busy`); live observations are never silently evicted.
    fn register(
        &self,
        webview: &str,
        token: &str,
        start: impl FnOnce(u64, Arc<Mutex<Observations>>) -> JoinHandle<()>,
    ) -> Result<(), Refusal> {
        self.close(webview, token);
        let Ok(mut observations) = self.observations.lock() else {
            return Err(Refusal::ShuttingDown);
        };
        observations
            .tasks
            .retain(|t| !t.handle.inner().is_finished());
        let held = observations
            .tasks
            .iter()
            .filter(|t| t.webview == webview)
            .count();
        if held >= MAX_OBSERVATIONS_PER_WEBVIEW {
            return Err(Refusal::Busy);
        }
        observations.next_id += 1;
        let id = observations.next_id;
        let handle = start(id, Arc::clone(&self.observations));
        observations.tasks.push_back(ObservationTask {
            id,
            webview: webview.to_owned(),
            token: token.to_owned(),
            handle,
        });
        Ok(())
    }

    /// Native zoom for the main webview (menu-driven; no renderer permission).
    pub fn zoom<R: Runtime>(&self, webview: &WebviewWindow<R>, step: Option<f64>) {
        let Ok(mut zoom) = self.zoom.lock() else {
            return;
        };
        *zoom = match step {
            Some(delta) => (*zoom + delta).clamp(0.5, 3.0),
            None => 1.0,
        };
        let _ = webview.set_zoom(*zoom);
    }
}

/// Registers the two application commands on a Tauri builder.
pub fn commands<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![desktop_dispatch, desktop_observe])
}

/// Installs host state for an existing core plus the commands (tests use
/// this with fixture adapters; the app builds its core in `setup`).
pub fn install<R: Runtime>(builder: tauri::Builder<R>, core: &Core) -> tauri::Builder<R> {
    commands(builder.manage(Host::new(core)))
}

/// A core over deterministic fixture adapters, for host tests.
#[cfg(feature = "test-fixtures")]
pub fn fixture_core() -> (Core, desktop_core::fixtures::Handles) {
    fixture_core_with(CoreConfig::default())
}

/// A fixture core with a given configuration (for example the synthetic
/// Academy origin).
#[cfg(feature = "test-fixtures")]
pub fn fixture_core_with(config: CoreConfig) -> (Core, desktop_core::fixtures::Handles) {
    let (adapters, handles) = desktop_core::fixtures::adapters();
    (Core::new(config, adapters), handles)
}

/// Builds the packaged main window with its navigation policy: it may only
/// display packaged assets; remote navigation, new windows and downloads are
/// refused, so remote content can never acquire command authority.
pub fn main_window<R: Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<WebviewWindow<R>> {
    let builder =
        WebviewWindowBuilder::new(manager, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
            .title(product_title())
            .inner_size(1280.0, 820.0)
            .min_inner_size(900.0, 600.0)
            .on_navigation(is_packaged_origin)
            .on_new_window(|_, _| NewWindowResponse::Deny)
            .on_download(|_, _| false)
            .on_page_load(|webview, payload| {
                // A (re)loading page owns none of the previous page's
                // observations; retire them so nothing leaks across reloads.
                if payload.event() == PageLoadEvent::Started {
                    webview.state::<Host>().retire_webview(webview.label());
                }
            });
    builder.build()
}

fn product_title() -> &'static str {
    if cfg!(feature = "test-fixtures") {
        "Prisma AIRS Desktop v2 (Fixture)"
    } else {
        "Prisma AIRS Desktop v2"
    }
}

/// True for the packaged asset origin served by Tauri's custom protocol.
pub fn is_packaged_origin(url: &Url) -> bool {
    url.scheme() == "tauri" && url.host_str() == Some("localhost") && url.port().is_none()
}

/// Defense in depth beyond the capability grant: the caller must be the main
/// webview of the main window currently showing packaged content.
fn admit_caller<R: Runtime>(webview: &Webview<R>) -> Result<(), Refusal> {
    if webview.label() != MAIN_WINDOW || webview.window().label() != MAIN_WINDOW {
        return Err(Refusal::CapabilityMissing);
    }
    match webview.url() {
        Ok(url) if is_packaged_origin(&url) => Ok(()),
        _ => Err(Refusal::CapabilityMissing),
    }
}

#[tauri::command]
async fn desktop_dispatch<R: Runtime>(
    webview: Webview<R>,
    request: Request<'_>,
    host: State<'_, Host>,
) -> Result<DispatchResponse, Refusal> {
    admit_caller(&webview)?;
    // The facade sends raw bytes so the size ceiling applies before parsing.
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err(Refusal::InvalidInput);
    };
    let request = DispatchRequest::parse(bytes).map_err(|e| e.refusal())?;
    let delivery = request.delivery().map_err(|e| e.refusal())?;
    Ok(host
        .operator
        .dispatch(delivery, request.intent)
        .await
        .into())
}

#[tauri::command]
async fn desktop_observe<R: Runtime>(
    webview: Webview<R>,
    request: Request<'_>,
    host: State<'_, Host>,
) -> Result<ObserveResponse, Refusal> {
    admit_caller(&webview)?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err(Refusal::InvalidInput);
    };
    let request = ObserveRequest::parse(bytes).map_err(|e| e.refusal())?;
    match request.scope {
        WireScope::DeliverySession => Ok(host
            .operator
            .register_delivery_session()
            .map_or_else(|refusal| ObserveResponse::Refusal { refusal }, Into::into)),
        WireScope::Shell { updates } => {
            let channel: Channel<ObservationUpdate> = updates
                .parse::<JavaScriptChannelId>()
                .map_err(|_| Refusal::InvalidInput)?
                .channel_on(webview.clone());
            let Observation::Shell {
                snapshot,
                updates: mut subscription,
            } = host.operator.observe(ViewScope::Shell)?
            else {
                return Err(Refusal::InvalidInput);
            };
            let mut menu = host.menu.subscribe();
            host.register(webview.label(), &updates, move |id, registry| {
                tauri::async_runtime::spawn(async move {
                loop {
                    let update = tokio::select! {
                        next = subscription.next() => match next {
                            Some(Update::Shell(snapshot)) => ObservationUpdate::Shell { snapshot },
                            Some(Update::Resync) | None => ObservationUpdate::Resync,
                        },
                        command = menu.recv() => match command {
                            Ok(command) => ObservationUpdate::Menu { command },
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => ObservationUpdate::Resync,
                        },
                    };
                    let last = matches!(update, ObservationUpdate::Resync);
                    if channel.send(update).is_err() || last {
                        break;
                    }
                }
                if let Ok(mut observations) = registry.lock() {
                    observations.tasks.retain(|t| t.id != id);
                }
                })
            })?;
            Ok(ObserveResponse::Shell {
                snapshot: Box::new(snapshot),
            })
        }
        WireScope::CloseShell { updates } => {
            host.close(webview.label(), &updates);
            Ok(ObserveResponse::Closed)
        }
        WireScope::AcademyCall { operation } => {
            let Observation::AcademyCall(handle) =
                host.operator.observe(ViewScope::AcademyCall(operation))?
            else {
                return Err(Refusal::InvalidInput);
            };
            Ok(ObserveResponse::AcademyCall {
                result: handle.result().await,
            })
        }
        WireScope::Management { operation } => {
            let Observation::Management(handle) =
                host.operator.observe(ViewScope::Management(operation))?
            else {
                return Err(Refusal::InvalidInput);
            };
            Ok(ObserveResponse::Management {
                result: handle.result().await,
            })
        }
    }
}

/// Runs the application with the given generated context. The core is
/// created in `setup`, where the OS data directory is known, with the
/// production adapters; durable cleanup resumes on the async runtime.
pub fn run(context: tauri::Context<tauri::Wry>) -> tauri::Result<()> {
    commands(tauri::Builder::default())
        .menu(menu::build)
        .on_menu_event(menu::on_event)
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::Destroyed) {
                window.state::<Host>().retire_webview(window.label());
            }
        })
        .setup(|app| {
            #[allow(unused_mut)]
            let mut config = CoreConfig::default();
            #[cfg(feature = "test-fixtures")]
            if let Some(origin) = adapters::fixture_academy_origin(app.handle()) {
                config.academy_origin = origin;
            }
            #[cfg(feature = "test-fixtures")]
            if let Some(origin) = adapters::fixture_management_origin(app.handle()) {
                config.management =
                    desktop_core::management::ManagementEndpoints::at_origin(&origin);
            }
            let core = Core::new(config, adapters::production(app.handle()));
            #[cfg(feature = "test-fixtures")]
            if let Some(policy) = adapters::fixture_transport(app.handle()) {
                core.set_transport_policy(policy)
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
            }
            app.manage(Host::new(&core));
            app.manage(Exit(core.clone()));
            tauri::async_runtime::spawn(async move { core.start() });
            main_window(app)?;
            Ok(())
        })
        .build(context)?
        .run(|app, event| {
            // Offline work is locked as the app exits.
            if matches!(event, tauri::RunEvent::Exit)
                && let Some(exit) = app.try_state::<Exit>()
            {
                exit.0.shutdown();
            }
        });
    Ok(())
}

/// The core, for shutdown at exit.
struct Exit(Core);
