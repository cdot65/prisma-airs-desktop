//! Native macOS menus. Selections become typed [`MenuCommand`]s delivered on
//! the shell observation channel; menus never carry authority themselves.

use desktop_core::features::FeatureId;
use desktop_core::wire::{MenuCommand, Theme};
use tauri::menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::Host;

/// Feature destinations reachable from the Go menu, with their shortcuts.
const GO: [(&str, FeatureId, &str, Option<&str>); 11] = [
    (
        "go:runtime",
        FeatureId::Runtime,
        "Runtime Security",
        Some("CmdOrCtrl+1"),
    ),
    (
        "go:academy",
        FeatureId::Academy,
        "Academy",
        Some("CmdOrCtrl+2"),
    ),
    (
        "go:administration",
        FeatureId::Administration,
        "Administration",
        Some("CmdOrCtrl+3"),
    ),
    (
        "go:sessions",
        FeatureId::Sessions,
        "Sessions & Reports",
        Some("CmdOrCtrl+4"),
    ),
    (
        "go:library",
        FeatureId::Library,
        "Case Library",
        Some("CmdOrCtrl+5"),
    ),
    (
        "go:redTeam",
        FeatureId::RedTeam,
        "Red Team & Models",
        Some("CmdOrCtrl+6"),
    ),
    (
        "go:apiClient",
        FeatureId::ApiClient,
        "API Client",
        Some("CmdOrCtrl+7"),
    ),
    (
        "go:gateway",
        FeatureId::Gateway,
        "AI Gateway",
        Some("CmdOrCtrl+8"),
    ),
    (
        "go:harness",
        FeatureId::Harness,
        "Harness",
        Some("CmdOrCtrl+9"),
    ),
    ("go:import", FeatureId::Import, "Import from v1", None),
    ("go:settings", FeatureId::Settings, "Settings", None),
];

const APPEARANCE: [(&str, Theme, &str); 3] = [
    ("appearance:system", Theme::System, "Match System"),
    ("appearance:light", Theme::Light, "Light"),
    ("appearance:dark", Theme::Dark, "Dark"),
];

/// Maps a menu item ID to its command; unknown IDs map to nothing.
pub fn command_for(id: &str) -> Option<MenuCommand> {
    if id == "palette" {
        return Some(MenuCommand::CommandPalette);
    }
    if id == "settings" {
        return Some(MenuCommand::Navigate {
            feature: FeatureId::Settings,
        });
    }
    if let Some((_, feature, _, _)) = GO.iter().find(|(item, ..)| *item == id) {
        return Some(MenuCommand::Navigate { feature: *feature });
    }
    APPEARANCE
        .iter()
        .find(|(item, ..)| *item == id)
        .map(|(_, theme, _)| MenuCommand::SetAppearance { theme: *theme })
}

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let app_menu = SubmenuBuilder::new(app, "Prisma AIRS Desktop v2")
        .about(None)
        .separator()
        .item(
            &MenuItemBuilder::with_id("settings", "Settings…")
                .accelerator("CmdOrCtrl+,")
                .build(app)?,
        )
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let mut appearance = SubmenuBuilder::new(app, "Appearance");
    for (id, _, label) in APPEARANCE {
        appearance = appearance.item(&MenuItemBuilder::with_id(id, label).build(app)?);
    }
    let view = SubmenuBuilder::new(app, "View")
        .item(
            &MenuItemBuilder::with_id("palette", "Command Palette…")
                .accelerator("CmdOrCtrl+K")
                .build(app)?,
        )
        .item(&appearance.build()?)
        .separator()
        .item(
            &MenuItemBuilder::with_id("zoom:in", "Zoom In")
                .accelerator("CmdOrCtrl+=")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("zoom:out", "Zoom Out")
                .accelerator("CmdOrCtrl+-")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("zoom:reset", "Actual Size")
                .accelerator("CmdOrCtrl+0")
                .build(app)?,
        )
        .separator()
        .fullscreen()
        .build()?;
    let mut go = SubmenuBuilder::new(app, "Go");
    for (id, _, label, accelerator) in GO {
        let mut item = MenuItemBuilder::with_id(id, label);
        if let Some(accelerator) = accelerator {
            item = item.accelerator(accelerator);
        }
        go = go.item(&item.build(app)?);
    }
    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .close_window()
        .build()?;
    let go = go.build()?;
    let menu = MenuBuilder::new(app).items(&[&app_menu, &edit, &view, &go, &window]);
    // The fixture identity alone gets a capture menu for native acceptance.
    #[cfg(all(feature = "test-fixtures", target_os = "macos"))]
    let menu = menu.item(
        &SubmenuBuilder::new(app, "Fixture")
            .item(
                &MenuItemBuilder::with_id("fixture:capture", "Capture Window")
                    .accelerator("CmdOrCtrl+Shift+9")
                    .build(app)?,
            )
            .item(
                &MenuItemBuilder::with_id("fixture:lock-store", "Lock Credential Store")
                    .build(app)?,
            )
            .item(
                &MenuItemBuilder::with_id("fixture:unlock-store", "Unlock Credential Store")
                    .build(app)?,
            )
            .build()?,
    );
    menu.build()
}

pub fn on_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    let id = event.id().as_ref();
    #[cfg(all(feature = "test-fixtures", target_os = "macos"))]
    if id == "fixture:capture" {
        if let Some(window) = app.get_webview_window(crate::MAIN_WINDOW) {
            crate::fixture_capture::capture(&window);
        }
        return;
    }
    #[cfg(all(feature = "test-fixtures", target_os = "macos"))]
    if id == "fixture:lock-store" || id == "fixture:unlock-store" {
        if let Some(store) = crate::adapters::FIXTURE_STORE.get() {
            if id == "fixture:lock-store" {
                store.lock_for_fixture();
            } else {
                store.unlock_for_fixture(crate::adapters::FIXTURE_KEYCHAIN_PASSWORD);
            }
        }
        return;
    }
    let step = match id {
        "zoom:in" => Some(Some(0.1)),
        "zoom:out" => Some(Some(-0.1)),
        "zoom:reset" => Some(None),
        _ => None,
    };
    if let Some(step) = step {
        // Zoom is applied natively; the renderer holds no webview permission.
        if let Some(webview) = app.get_webview_window(crate::MAIN_WINDOW) {
            app.state::<Host>().zoom(&webview, step);
        }
        return;
    }
    if let Some(command) = command_for(id) {
        app.state::<Host>().menu_command(command);
    }
}
