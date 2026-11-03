//! Native macOS menus. Selections become typed [`MenuCommand`]s delivered on
//! the shell observation channel; menus never carry authority themselves.

use desktop_core::features::{self, FeatureId};
use desktop_core::wire::{MenuCommand, Theme};
use tauri::menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::Host;

/// The Go menu is the presentation's catalogue (`features::catalogue`):
/// one Rust inventory feeds the menu, the sidebar and the palette, so the
/// shortcuts the sidebar shows are the accelerators the menu defines (C28).
fn go_items() -> Vec<(String, FeatureId, String, Option<String>)> {
    features::catalogue()
        .into_iter()
        .map(|entry| {
            (
                format!("go:{}", feature_name(entry.id)),
                entry.id,
                entry.label,
                entry.shortcut.map(|n| format!("CmdOrCtrl+{n}")),
            )
        })
        .collect()
}

/// A catalogue label as a menu title: the menu library reads a lone `&` as a
/// mnemonic marker and drops it from the title (macOS shows "Sessions  Reports"
/// for "Sessions & Reports"), so an ampersand is doubled to stay visible.
fn menu_title(label: &str) -> String {
    label.replace('&', "&&")
}

/// The wire name of a feature (`runtime`, `apiClient`, ...): the same spelling
/// the renderer and the catalogue use.
fn feature_name(id: FeatureId) -> String {
    match serde_json::to_value(id) {
        Ok(serde_json::Value::String(name)) => name,
        _ => unreachable!("FeatureId serializes to a string"),
    }
}

fn feature_for(name: &str) -> Option<FeatureId> {
    serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
}

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
    if id == "save" {
        return Some(MenuCommand::SaveDocument);
    }
    if id == "tenants" {
        return Some(MenuCommand::TenantSwitcher);
    }
    if id == "settings" {
        return Some(MenuCommand::Navigate {
            feature: FeatureId::Settings,
        });
    }
    if let Some(feature) = id.strip_prefix("go:").and_then(feature_for) {
        return Some(MenuCommand::Navigate { feature });
    }
    APPEARANCE
        .iter()
        .find(|(item, ..)| *item == id)
        .map(|(_, theme, _)| MenuCommand::SetAppearance { theme: *theme })
}

/// The single Home-first native menu, installed at startup.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let app_menu = SubmenuBuilder::new(app, "Prisma AIRS Desktop")
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
        // Not the predefined item: AppKit's terminate: would end the app
        // without the API Client close guard.
        .item(
            &MenuItemBuilder::with_id("quit", "Quit Prisma AIRS Desktop")
                .accelerator("CmdOrCtrl+Q")
                .build(app)?,
        )
        .build()?;
    // WKWebView receives Command shortcuts only through menu items.
    let file = SubmenuBuilder::new(app, "File")
        .item(
            &MenuItemBuilder::with_id("save", "Save")
                .accelerator("CmdOrCtrl+S")
                .build(app)?,
        )
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
    let mut go = SubmenuBuilder::new(app, "Go").item(
        &MenuItemBuilder::with_id("tenants", "Switch Tenant…")
            .accelerator("CmdOrCtrl+T")
            .build(app)?,
    );
    go = go.separator();
    for (id, _, label, accelerator) in go_items() {
        let mut item = MenuItemBuilder::with_id(id, menu_title(&label));
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
    let menu = MenuBuilder::new(app).items(&[&app_menu, &file, &edit, &view, &go, &window]);
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
            .item(&MenuItemBuilder::with_id("fixture:will-sleep", "Post Will Sleep").build(app)?)
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
    #[cfg(all(feature = "test-fixtures", target_os = "macos"))]
    if id == "fixture:will-sleep" {
        crate::power::post_will_sleep();
        return;
    }
    if id == "quit" {
        crate::request_quit(app);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ampersand_in_a_catalogue_label_stays_visible_in_the_menu_title() {
        assert_eq!(menu_title("Sessions & Reports"), "Sessions && Reports");
        assert!(
            go_items()
                .iter()
                .any(|(_, _, label, _)| label == "Sessions & Reports"),
            "the catalogue label itself is untouched"
        );
    }

    #[test]
    fn go_items_follow_the_presentation_catalogue() {
        let v2 = go_items();
        assert_eq!(
            v2[0],
            (
                "go:home".to_owned(),
                FeatureId::Home,
                "Home".to_owned(),
                Some("CmdOrCtrl+1".to_owned())
            )
        );
        assert_eq!(v2[1].0, "go:inbox");
        assert_eq!(v2[6].3.as_deref(), Some("CmdOrCtrl+7"));
        assert!(
            v2[7..].iter().all(|(_, _, _, a)| a.is_none()),
            "non-sidebar entries have no accelerator"
        );
        assert_eq!(
            command_for("go:apiClient"),
            Some(MenuCommand::Navigate {
                feature: FeatureId::ApiClient
            })
        );
        assert_eq!(
            command_for("go:home"),
            Some(MenuCommand::Navigate {
                feature: FeatureId::Home
            })
        );
        assert_eq!(command_for("go:nope"), None);
        assert_eq!(command_for("tenants"), Some(MenuCommand::TenantSwitcher));
    }
}
