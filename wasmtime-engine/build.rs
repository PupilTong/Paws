use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

/// Examples under `Paws/examples/` — each is its own mini-workspace.
const EXAMPLES: &[&str] = &[
    "example-basic-element",
    "example-styled-element",
    "example-nested-elements",
    "example-stylesheet-cascade",
    "example-parsed-stylesheet",
    "example-attributes",
    "example-destroy-rebuild",
    "example-commit-full",
    "example-namespace",
    "example-event-dispatch",
];

/// Examples under `Paws/yew/examples/` — part of the yew workspace.
/// These produce WASM binaries inside `yew/target/` instead of their
/// own `target/` directory. Built for `wasm32-wasip1` (not the
/// `-threads` variant) because wasi-libc's pthread-based TLS in the
/// `-threads` target requires a wasi-threads host implementation that
/// we don't yet provide. The non-threads variant uses static TLS and
/// the same `rust-wasm-binding` FFI.
const YEW_EXAMPLES: &[&str] = &["example-yew-counter"];
const YEW_WASM_TARGET: &str = "wasm32-wasip1";

const WASM_TARGET: &str = "wasm32-wasip1-threads";

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.parent().expect("workspace root");
    let examples_dir = workspace_root.join("examples");
    let yew_examples_dir = workspace_root.join("yew").join("examples");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Rerun if example sources or the binding crate change
    println!("cargo:rerun-if-changed={}", examples_dir.display());
    println!("cargo:rerun-if-changed={}", yew_examples_dir.display());
    println!(
        "cargo:rerun-if-changed={}",
        workspace_root
            .join("rust-wasm-binding")
            .join("src")
            .display()
    );

    let mut wasm_paths: Vec<(String, PathBuf)> = Vec::new();

    // Build standalone examples (each has its own target/ directory)
    for name in EXAMPLES {
        let crate_dir = examples_dir.join(name);
        if !crate_dir.exists() {
            panic!("example crate not found: {}", crate_dir.display());
        }

        let status = Command::new("cargo")
            .arg("build")
            .arg("--target")
            .arg(WASM_TARGET)
            .arg("--release")
            .current_dir(&crate_dir)
            .status()
            .unwrap_or_else(|e| panic!("failed to run cargo build for {name}: {e}"));

        assert!(status.success(), "cargo build failed for {name}");

        let wasm_filename = format!("{}.wasm", name.replace('-', "_"));
        let wasm_src = crate_dir
            .join("target")
            .join(WASM_TARGET)
            .join("release")
            .join(&wasm_filename);

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

        wasm_paths.push((name.replace('-', "_"), wasm_dst));
    }

    // Build yew examples (part of the yew workspace, output in yew/target/).
    // Skip gracefully if the yew submodule isn't checked out (e.g. in CI
    // without --recurse-submodules).
    for name in YEW_EXAMPLES {
        let crate_dir = yew_examples_dir.join(name);
        if !crate_dir.exists() {
            eprintln!(
                "cargo:warning=skipping yew example {name}: \
                 {crate_dir:?} not found (submodule not checked out?)"
            );
            continue;
        }

        let status = Command::new("cargo")
            .arg("build")
            .arg("--target")
            .arg(YEW_WASM_TARGET)
            .arg("--release")
            .arg("-p")
            .arg(name)
            .current_dir(&crate_dir)
            .status()
            .unwrap_or_else(|e| panic!("failed to run cargo build for {name}: {e}"));

        assert!(status.success(), "cargo build failed for {name}");

        // yew workspace examples produce output in yew/target/
        let wasm_filename = format!("{}.wasm", name.replace('-', "_"));
        let wasm_src = workspace_root
            .join("yew")
            .join("target")
            .join(YEW_WASM_TARGET)
            .join("release")
            .join(&wasm_filename);

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

        wasm_paths.push((name.replace('-', "_"), wasm_dst));
    }

    // Generate a Rust file that maps example names to file paths
    let generated = out_dir.join("wasm_examples.rs");
    let mut code = String::new();
    code.push_str("/// Returns the path to a compiled example WASM file.\n");
    code.push_str("pub fn example_wasm_path(name: &str) -> &'static str {\n");
    code.push_str("    match name {\n");
    for (rust_name, path) in &wasm_paths {
        code.push_str(&format!(
            "        \"{}\" => \"{}\",\n",
            rust_name,
            path.display()
        ));
    }
    code.push_str("        other => panic!(\"unknown example: {other}\"),\n");
    code.push_str("    }\n");
    code.push_str("}\n");
    fs::write(&generated, &code).expect("write wasm_examples.rs");
}
