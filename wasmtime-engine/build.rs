use std::path::{Path, PathBuf};
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

/// Yew-based test fixtures under `Paws/examples/yew/`. Source lives in
/// the Paws repo; each crate is a member of the yew submodule's
/// workspace (see `yew/Cargo.toml`) so `yew/packages/yew`'s
/// `workspace = true` dependencies resolve. Built artifacts land in
/// `yew/target/` (shared so yew itself only compiles once).
const YEW_EXAMPLES: &[&str] = &[
    "example-yew-counter",
    // Ported from tests-archive/integration/use_state.rs
    "example-yew-use-state-counter",
    "example-yew-multi-state-setters",
    "example-yew-use-state-eq",
    "example-yew-ub-deref",
    "example-yew-stale-read",
    "example-yew-child-rerender",
];

const WASM_TARGET: &str = "wasm32-wasip1-threads";

/// Shared coverage configuration for guest WASM builds.
struct CoverageConfig {
    enabled: bool,
    /// Optional toolchain override (e.g. `"nightly-2026-04-07"`). When
    /// `None`, the active toolchain from `rust-toolchain.toml` is used.
    toolchain: Option<String>,
    /// RUSTFLAGS value to pass to child cargo processes when enabled.
    /// Empty string when `enabled` is false.
    rustflags: String,
}

/// Builds a single WASM guest crate and copies its `.wasm` output into
/// `out_dir`, returning `(rust_name, copied_wasm_path)`.
///
/// - `crate_dir` is where `cargo` runs (the example's own workspace root).
/// - `wasm_src_dir` is where the `.wasm` output lands. For standalone
///   examples this is `<crate_dir>/target/<target>/release/`; for yew
///   examples it is `<workspace>/yew/target/<target>/release/`.
/// - `package` is `Some(name)` when the invocation needs `-p <name>`
///   (yew's multi-package workspace), `None` otherwise.
fn build_wasm_example(
    name: &str,
    crate_dir: &Path,
    wasm_src_dir: &Path,
    target: &str,
    package: Option<&str>,
    out_dir: &Path,
    coverage: &CoverageConfig,
) -> (String, PathBuf) {
    let mut cmd = Command::new("cargo");
    if let Some(toolchain) = &coverage.toolchain {
        cmd.arg(format!("+{toolchain}"));
    }
    cmd.arg("build")
        .arg("--target")
        .arg(target)
        .arg("--release")
        .current_dir(crate_dir);
    if let Some(pkg) = package {
        cmd.arg("-p").arg(pkg);
    }
    if coverage.enabled {
        cmd.arg("--features").arg("coverage");
        // Cargo sets CARGO_ENCODED_RUSTFLAGS='' for build scripts; child
        // cargo processes prefer it over RUSTFLAGS. Remove it so our
        // coverage RUSTFLAGS takes effect.
        cmd.env("RUSTFLAGS", &coverage.rustflags);
        cmd.env_remove("CARGO_ENCODED_RUSTFLAGS");
        // `-Cinstrument-coverage` + LTO on wasm produces
        // "data symbols must live in a data section: __covrec_..." —
        // LLVM can't relocate coverage records across LTO boundaries for
        // the wasm backend. Override profile.release.lto for coverage
        // builds so the per-crate profile config (e.g. yew's) doesn't
        // re-enable it.
        cmd.env("CARGO_PROFILE_RELEASE_LTO", "false");
        // yew's workspace profile sets opt-level="z" + codegen-units=1,
        // which under -Cinstrument-coverage can emit coverage records
        // with empty function names (llvm-cov then fails with
        // "malformed instrumentation profile data: function name is
        // empty"). Override to a less aggressive config for coverage
        // builds so llvm-cov can read the records back.
        cmd.env("CARGO_PROFILE_RELEASE_OPT_LEVEL", "1");
        cmd.env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "16");
    }
    let status = cmd
        .status()
        .unwrap_or_else(|e| panic!("failed to run cargo build for {name}: {e}"));
    assert!(status.success(), "cargo build failed for {name}");

    let rust_name = name.replace('-', "_");
    let wasm_filename = format!("{rust_name}.wasm");
    let wasm_src = wasm_src_dir.join(&wasm_filename);
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
    (rust_name, wasm_dst)
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.parent().expect("workspace root");
    let examples_dir = workspace_root.join("examples");
    let yew_examples_dir = examples_dir.join("yew");
    let yew_dir = workspace_root.join("yew");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // When PAWS_WASM_COVERAGE=1, compile guest WASM with LLVM coverage
    // instrumentation via minicov. `-Cinstrument-coverage` embeds the
    // `__llvm_covmap` section directly into the linked `.wasm` artifact,
    // which `llvm-cov export` reads back to produce lcov data.
    let coverage_enabled = env::var("PAWS_WASM_COVERAGE").is_ok();
    let coverage_toolchain = env::var("PAWS_WASM_COVERAGE_TOOLCHAIN").ok();
    let coverage_rustflags = if coverage_enabled {
        let existing = env::var("RUSTFLAGS").unwrap_or_default();
        let coverage_flags = "-Cinstrument-coverage -Zno-profiler-runtime";
        if existing.is_empty() {
            coverage_flags.to_string()
        } else {
            format!("{existing} {coverage_flags}")
        }
    } else {
        String::new()
    };
    let coverage = CoverageConfig {
        enabled: coverage_enabled,
        toolchain: coverage_toolchain,
        rustflags: coverage_rustflags,
    };

    println!("cargo:rerun-if-changed={}", examples_dir.display());
    println!(
        "cargo:rerun-if-changed={}",
        workspace_root
            .join("rust-wasm-binding")
            .join("src")
            .display()
    );
    println!("cargo:rerun-if-env-changed=PAWS_WASM_COVERAGE");
    println!("cargo:rerun-if-env-changed=PAWS_WASM_COVERAGE_TOOLCHAIN");

    let mut wasm_paths: Vec<(String, PathBuf)> = Vec::new();

    // Build standalone examples (each has its own target/ directory)
    for name in EXAMPLES {
        let crate_dir = examples_dir.join(name);
        if !crate_dir.exists() {
            panic!("example crate not found: {}", crate_dir.display());
        }
        let wasm_src_dir = crate_dir.join("target").join(WASM_TARGET).join("release");
        wasm_paths.push(build_wasm_example(
            name,
            &crate_dir,
            &wasm_src_dir,
            WASM_TARGET,
            None,
            &out_dir,
            &coverage,
        ));
    }

    // Build yew examples. Each crate is a member of the yew submodule's
    // workspace, so we run `cargo build` from the yew workspace root
    // with `-p <name>` and pick up the artifact from yew/target/.
    let yew_wasm_src_dir = yew_dir.join("target").join(WASM_TARGET).join("release");
    for name in YEW_EXAMPLES {
        let crate_dir = yew_examples_dir.join(name);
        if !crate_dir.exists() {
            panic!("yew example crate not found: {}", crate_dir.display());
        }
        wasm_paths.push(build_wasm_example(
            name,
            &yew_dir,
            &yew_wasm_src_dir,
            WASM_TARGET,
            Some(name),
            &out_dir,
            &coverage,
        ));
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
