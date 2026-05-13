//! Compiles the standalone wasm32-wasip2 fixtures under
//! `paws-wpt/fixtures/` and emits a lookup table mapping each
//! fixture's rust name to its compiled `.wasm` path.
//!
//! Mirrors `examples/build.rs` — they share the same shape because
//! both build pipelines drive standalone-workspace guest crates that
//! depend on Yew + rust-wasm-binding via path. The two scripts are
//! kept separate (rather than abstracted) so the `examples/` and
//! `paws-wpt/` directories are independent: changing one doesn't
//! force a rebuild of the other.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

/// Each entry is the directory name under `paws-wpt/fixtures/`. The
/// directory must contain a standalone wasm32-wasip2 cdylib crate
/// whose package name matches the directory name.
const FIXTURES: &[&str] = &[
    "dom-nodes-document-create-element",
    "css-overflow-layer-clipping",
];

const WASM_TARGET: &str = "wasm32-wasip2";

fn main() {
    // build.rs always runs on the host, but $TARGET reflects what the
    // *consumer* is being built for. When paws-wpt is pulled in as a
    // wasm32 dep (it is not today, but a future fixture might), skip
    // the recursive fixture build to avoid an infinite cargo loop.
    let consumer_target = env::var("TARGET").unwrap_or_default();
    if consumer_target.starts_with("wasm32") {
        emit_empty_lookup();
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.parent().expect("workspace root");
    let fixtures_dir = manifest_dir.join("fixtures");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    println!("cargo:rerun-if-changed={}", fixtures_dir.display());
    println!(
        "cargo:rerun-if-changed={}",
        workspace_root
            .join("rust-wasm-binding")
            .join("src")
            .display()
    );

    // Share `yew/target/` across fixture builds so the yew path-dep
    // only compiles once across the whole fixture set, mirroring
    // `examples/build.rs`'s yew handling.
    let yew_target_dir = workspace_root.join("yew").join("target");
    let yew_wasm_src_dir = yew_target_dir.join(WASM_TARGET).join("release");

    let mut wasm_paths: Vec<(String, PathBuf)> = Vec::new();

    for name in FIXTURES {
        let crate_dir = fixtures_dir.join(name);
        if !crate_dir.exists() {
            panic!("fixture crate not found: {}", crate_dir.display());
        }
        let mut command = Command::new("cargo");
        command
            .arg("build")
            .arg("--target")
            .arg(WASM_TARGET)
            .arg("--release")
            .current_dir(&crate_dir)
            .env("CARGO_TARGET_DIR", &yew_target_dir);
        // Disable LTO unconditionally — yew's workspace profile turns
        // it on aggressively, which strips symbol names that
        // wasmtime/coverage tooling later relies on.
        command.env("CARGO_PROFILE_RELEASE_LTO", "false");

        let status = command
            .status()
            .unwrap_or_else(|e| panic!("failed to run cargo build for {name}: {e}"));
        assert!(status.success(), "cargo build failed for fixture {name}");

        let rust_name = name.replace('-', "_");
        let wasm_filename = format!("{rust_name}.wasm");
        let wasm_src = yew_wasm_src_dir.join(&wasm_filename);
        assert!(
            wasm_src.exists(),
            "expected wasm output not found: {}",
            wasm_src.display()
        );

        let wasm_dst = out_dir.join(&wasm_filename);
        fs::copy(&wasm_src, &wasm_dst).unwrap_or_else(|e| {
            panic!(
                "failed to copy {} → {}: {e}",
                wasm_src.display(),
                wasm_dst.display()
            )
        });
        wasm_paths.push((rust_name, wasm_dst));
    }

    write_lookup(&out_dir, &wasm_paths);
}

fn emit_empty_lookup() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    write_lookup(&out_dir, &[]);
}

fn write_lookup(out_dir: &Path, wasm_paths: &[(String, PathBuf)]) {
    let generated = out_dir.join("wpt_fixtures.rs");
    let mut code = String::new();
    code.push_str("/// Returns the path to a compiled WPT fixture wasm.\n");
    code.push_str("///\n");
    code.push_str("/// Panics if `name` does not match a fixture compiled by build.rs.\n");
    code.push_str("pub fn fixture_wasm_path(name: &str) -> &'static str {\n");
    code.push_str("    match name {\n");
    for (rust_name, path) in wasm_paths {
        code.push_str(&format!(
            "        \"{}\" => \"{}\",\n",
            rust_name,
            path.display()
        ));
    }
    code.push_str("        other => panic!(\"unknown WPT fixture: {other}\"),\n");
    code.push_str("    }\n");
    code.push_str("}\n");
    fs::write(&generated, &code).expect("write wpt_fixtures.rs");
}
