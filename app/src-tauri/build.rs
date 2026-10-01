/// Le manifeste Windows (contrôles communs v6, requis par les boîtes de dialogue) est intégré par l'éditeur de liens à
/// tous les exécutables, tests compris : intégré par `tauri-build`, il ne l'est qu'au binaire de l'application, et les
/// tests s'arrêtent alors avec STATUS_ENTRYPOINT_NOT_FOUND.
fn main() {
    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)).expect("tauri-build a échoué");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
