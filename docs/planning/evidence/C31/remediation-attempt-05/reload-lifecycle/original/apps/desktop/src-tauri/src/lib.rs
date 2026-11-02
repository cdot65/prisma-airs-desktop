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
    AppHandle, Manager, Runtime, State, Url, Webview, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tokio::sync::broadcast;

pub mod adapters;
pub mod clipboard_secret;
#[cfg(all(feature = "test-fixtures", target_os = "macos"))]
pub mod fixture_capture;
#[cfg(target_os = "macos")]
pub mod keychain;
mod menu;
pub mod native_confirmation;
#[cfg(target_os = "macos")]
pub mod power;
#[cfg(target_os = "macos")]
pub mod sidecar;

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
    /// The presentation the native Go menu was last reconciled with (C28).
    menu_variant: Mutex<Option<desktop_core::features::UiVariant>>,
}

impl Host {
    pub fn new(core: &Core) -> Self {
        let (menu, _) = broadcast::channel(32);
        Self {
            operator: core.operator(),
            menu,
            observations: Arc::default(),
            zoom: Mutex::new(1.0),
            menu_variant: Mutex::new(None),
        }
    }

    /// Reconciles the native Go menu with `variant` (C28): called whenever a
    /// shell observation starts or reports a changed presentation, so a
    /// transition lost to an overflowing stream is repaired on the client's
    /// resubscribe. Without an installed menu (host tests) only the record
    /// is kept.
    pub fn sync_menu<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        variant: desktop_core::features::UiVariant,
    ) {
        if let Ok(mut current) = self.menu_variant.lock() {
            *current = Some(variant);
        }
        if app.menu().is_some() {
            let _ = menu::rebuild(app, variant);
        }
    }

    /// The presentation the Go menu was last reconciled with.
    pub fn menu_variant(&self) -> Option<desktop_core::features::UiVariant> {
        self.menu_variant.lock().ok().and_then(|v| *v)
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
            let mut variant = snapshot.ui_variant;
            let app = webview.app_handle().clone();
            // Every (re)subscription reconciles the native menu with the fresh
            // snapshot: a variant change dropped by an overflowing stream
            // (replaced by Resync) must not leave the Go menu stale (C28).
            host.sync_menu(&app, variant);
            host.register(webview.label(), &updates, move |id, registry| {
                tauri::async_runtime::spawn(async move {
                loop {
                    let update = tokio::select! {
                        next = subscription.next() => match next {
                            Some(Update::Shell(snapshot)) => {
                                // A changed presentation changes the Go menu (C28).
                                if snapshot.ui_variant != variant {
                                    variant = snapshot.ui_variant;
                                    if let Some(host) = app.try_state::<Host>() {
                                        host.sync_menu(&app, variant);
                                    }
                                }
                                ObservationUpdate::Shell { snapshot }
                            }
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
        WireScope::Workbench { operation } => {
            let Observation::Workbench(handle) =
                host.operator.observe(ViewScope::Workbench(operation))?
            else {
                return Err(Refusal::InvalidInput);
            };
            Ok(ObserveResponse::Workbench {
                result: Box::new(handle.result().await),
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
            // API Client edits not yet protected on this Mac: the operator
            // decides in a native sheet (v1's close guard).
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == MAIN_WINDOW
                && let Some(exit) = window.try_state::<Exit>()
                && exit.guard(window.app_handle())
            {
                api.prevent_close();
            }
        })
        .setup(|app| {
            #[allow(unused_mut)]
            let mut config = CoreConfig {
                adoption_source: adapters::adoption_source(app.handle()),
                // Fixture builds send API Client requests to loopback only.
                workbench_network: if cfg!(feature = "test-fixtures") {
                    desktop_core::workbench::admission::Network::LoopbackOnly
                } else {
                    desktop_core::workbench::admission::Network::Live
                },
                ..CoreConfig::default()
            };
            #[cfg(feature = "test-fixtures")]
            if let Some(origin) = adapters::fixture_academy_origin(app.handle()) {
                config.academy_origin = origin;
            }
            #[cfg(feature = "test-fixtures")]
            if let Some(origin) = adapters::fixture_management_origin(app.handle()) {
                config.management =
                    desktop_core::management::ManagementEndpoints::at_origin(&origin);
            }
            #[cfg(feature = "test-fixtures")]
            if let Some(catalog) = adapters::fixture_case_catalog(app.handle()) {
                config.case_catalog = catalog;
            }
            let core = Core::new(config, adapters::production(app.handle()));
            #[cfg(feature = "test-fixtures")]
            if let Some(policy) = adapters::fixture_transport(app.handle()) {
                core.set_transport_policy(policy)
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
            }
            // The acceptance driver's crash point for the next v1 import
            // (C23): the import stops there, and the driver ends the app.
            #[cfg(feature = "test-fixtures")]
            if let Some(point) = adapters::fixture_import_crash(app.handle()) {
                core.interrupt_v1_import(point);
            }
            // A panic is recorded locally by where it happened, never by its
            // message (D051); the default report still follows.
            let recorder = core.clone();
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                if let Some(location) = info.location() {
                    recorder.record_panic(location.file(), location.line());
                }
                previous(info);
            }));
            #[cfg(target_os = "macos")]
            power::observe_sleep(core.clone());
            app.manage(Host::new(&core));
            // The Go menu follows the stored presentation (C28).
            if let Ok(Observation::Shell { snapshot, .. }) =
                core.operator().observe(ViewScope::Shell)
            {
                app.state::<Host>()
                    .sync_menu(app.handle(), snapshot.ui_variant);
            }
            app.manage(Exit {
                core: core.clone(),
                quitting: std::sync::atomic::AtomicBool::new(false),
                asking: std::sync::atomic::AtomicBool::new(false),
            });
            tauri::async_runtime::spawn(async move { core.start() });
            main_window(app)?;
            Ok(())
        })
        .build(context)?
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = &event
                && code.is_none()
                && let Some(exit) = app.try_state::<Exit>()
                && exit.guard(app)
            {
                api.prevent_exit();
            }
            // Offline work is locked as the app exits.
            if matches!(event, tauri::RunEvent::Exit)
                && let Some(exit) = app.try_state::<Exit>()
            {
                // A Harness engine is stopped and reaped before the app goes
                // (DESIGN §3: 2 s graceful plus 2 s forced, with margin).
                exit.core
                    .end_harness_for_exit(std::time::Duration::from_secs(5));
                exit.core.shutdown();
            }
        });
    Ok(())
}

/// The core, for the close guard and shutdown at exit.
struct Exit {
    core: Core,
    /// The operator chose Quit anyway.
    quitting: std::sync::atomic::AtomicBool,
    /// The close guard's sheet is shown.
    asking: std::sync::atomic::AtomicBool,
}

impl Exit {
    /// True when closing must wait. Edits typed just before the request may
    /// still be on their way from the presentation, so closing always waits
    /// a moment first; then, when API Client edits are not protected on this
    /// Mac, the operator decides in a native sheet (Quit anyway exits) and
    /// otherwise the app exits.
    fn guard<R: Runtime>(&self, app: &tauri::AppHandle<R>) -> bool {
        use std::sync::atomic::Ordering;
        if self.quitting.load(Ordering::SeqCst) {
            return false;
        }
        if self.asking.swap(true, Ordering::SeqCst) {
            return true;
        }
        let confirmation = adapters::close_guard(app);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(CLOSE_SETTLE).await;
            let Some(exit) = app.try_state::<Exit>() else {
                return;
            };
            let quit = !exit.core.workbench_unprotected()
                || confirmation
                    .confirm(
                        u64::MAX,
                        desktop_core::ConfirmationRequest {
                            title: "Quit with unprotected API Client edits?".to_owned(),
                            message: "A request has edits that are not protected on this Mac yet. Cancel, let them be kept, then close again; or quit anyway and lose them.".to_owned(),
                            confirm_label: "Quit anyway".to_owned(),
                            destructive: true,
                        },
                    )
                    .await;
            exit.asking.store(false, Ordering::SeqCst);
            if quit {
                exit.quitting.store(true, Ordering::SeqCst);
                app.exit(0);
            }
        });
        true
    }
}

/// The app menu's Quit: through the close guard, then exit.
fn request_quit<R: Runtime>(app: &tauri::AppHandle<R>) {
    match app.try_state::<Exit>() {
        Some(exit) if exit.guard(app) => {}
        _ => app.exit(0),
    }
}

/// How long closing waits for intents already sent by the presentation.
const CLOSE_SETTLE: std::time::Duration = std::time::Duration::from_millis(300);
