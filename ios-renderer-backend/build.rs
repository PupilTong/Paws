use std::env;
use std::path::PathBuf;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    let mut config = cbindgen::Config::default();
    config.language = cbindgen::Language::C;
    config.include_guard = Some("IOS_RENDERER_BACKEND_H".to_string());
    config
        .export
        .rename
        .insert("Rect".to_string(), "RBRect".to_string());
    config
        .export
        .rename
        .insert("Size".to_string(), "RBSize".to_string());
    config
        .export
        .rename
        .insert("Color".to_string(), "RBColor".to_string());

    let out_dir = PathBuf::from(&crate_dir);

    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(config)
        .generate()
        .expect("Unable to generate C bindings")
        .write_to_file(out_dir.join("ios_renderer_backend.h"));
}
