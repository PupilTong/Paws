fn main() {
    // Skip cbindgen during test builds — the header is only needed for Swift integration.
    if std::env::var("CARGO_CFG_TEST").is_ok() {
        return;
    }

    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

    let config = cbindgen::Config::from_file("cbindgen.toml")
        .unwrap_or_else(|_| cbindgen::Config::default());

    if let Ok(bindings) = cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
    {
        bindings.write_to_file("Sources/PawsRendererFFI/include/ios_renderer_backend.h");
    }
}
