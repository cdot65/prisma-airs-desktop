fn main() {
    // Register exactly the two application commands so they require an
    // explicit capability grant (no implicit allow for custom commands).
    let manifest =
        tauri_build::AppManifest::new().commands(&["desktop_dispatch", "desktop_observe"]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("tauri build configuration");
}
