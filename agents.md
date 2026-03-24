# Agent Instructions

This repository supports LLM-based assistants. The working language is English.

## Supported Agents

- Google Antigravity (via `.agent/` workflows)
- Claude (via `CLAUDE.md` at repo root)
- GitHub Copilot

## General Guidelines

- **Rebase onto `origin/main` before starting any work** to avoid merge conflicts and stale base commits.
- Keep changes minimal and focused.
- Follow existing style conventions and formatting.
- Prefer Rust 2021 edition and stable toolchain.
- Use cargo workspace conventions.
- Update or add tests when relevant.
- Agents should always run tests.
- **Test behavior, not implementation.** Tests should exercise real-world CSS scenarios end-to-end (e.g., compile a stylesheet with `css!()`, feed it through the pipeline, and assert on computed style values) rather than testing internal helper functions or matching on intermediate data structures. Coverage must come from realistic usage paths.
- Ensure error handling returns specific ErrorCodes where applicable (avoid string errors).
- Ensure operations are transactional/atomic where possible (check preconditions before mutation).
- For non-thread-safe non-cryptographic keys (like integers), use `FnvHashMap` instead of `HashMap`.
- No `println!`/`eprintln!`/`dbg!` in production code.
- All `unsafe` blocks must have a `// SAFETY:` comment explaining the invariant.
- Prefer to use `pub(crate)` or keep fields/methods private by default unless they explicitly need to be public.
- On every code change, assess whether agents.md needs an update and update it when needed.
- After finishing all work, verify that agents.md is still accurate and up to date.

## Formatting

- CI runs `cargo fmt --check` with the **stable** toolchain. Local nightly rustfmt may produce different output.
- Always run `cargo fmt --check` (not just `cargo fmt`) before committing to catch divergences. If your local formatter disagrees with CI, manually adjust to match the stable rustfmt style:
  - Short method chains that fit on one line should stay on one line (e.g. `if self.doc.get_node(id).is_none() {`).
  - Short `if/else` with single expressions should use multi-line block style, not single-line `if x { A } else { B }`.
  - Function signatures that fit within the line width should stay on one line.

## WASM Host Functions

- All WASM host functions use **snake_case** names with a `__` prefix (e.g. `__create_element`, `__append_element`).
- Host functions registered in `wasmtime-engine/src/wasm.rs` must have matching `extern "C"` declarations in `rust-wasm-binding/src/lib.rs`.
- When adding a new host function, update all three layers: `engine/` (core logic) → `wasmtime-engine/` (linker registration) → `rust-wasm-binding/` (FFI + safe wrapper).
- WAT test strings in `wasmtime-engine/src/lib.rs`, benchmarks, and iOS files must also use the same function names.

## Repository Structure

- `engine/`: core logic (DOM, Style, Layout). A pure Rust library with no host dependencies.
- `wasmtime-engine/`: integration layer threading `wasmtime` and `engine` together.
- `rust-wasm-binding/`: no_std Rust FFI binding for WASM guests. Wraps all host functions and re-exports `css!()` macro.
- `view-macros/`: Contains the `css!` proc-macro for compile-time CSS evaluation.
- `paws-style-ir/`: Zero-copy intermediate representations (`rkyv`) for styles shared between the macro and runtime.
- `ios-renderer-backend/`: iOS rendering backend — bridges engine `LayoutBox` output to UIKit via C FFI. Rust owns and controls UIView, UILabel, UITextView, UIScrollView, and CALayer through opaque pointer handles. Includes a Swift Package (`PawsRenderer`) with `PawsRendererInstance` wrapper and `PawsRendererView`. Depends on `engine` and `wasmtime-engine`. Uses cbindgen for header generation.
- `ios-example-app/`: Example iOS app (Xcode project) demonstrating WASM → engine → renderer → UIKit pipeline.

## Project Design Overview

- Cross-platform framework with a pure WASM VM for UI logic (`wasmtime-engine`).
- Stylo provides web-standard CSS behavior and style computation.
- Taffy provides box layout.
- LynxJS native elements provide actual rendering.

### System Split

1. **Developer-side compilation**
	- Flexible WASM and/or WASM AOT compilation.
	- Batch update capabilities for the WASM-based UI framework.
	- Runs on developer machines to build optimized artifacts.

2. **Engine runtime (Android/iOS integration)**
	- Engine code merged and packaged with Android/iOS apps.
	- Provides style computation, layout, and rendering.

## CI Expectations

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -D warnings`
- `cargo test --all`

## How to Run

- `cargo test --all`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -D warnings`
