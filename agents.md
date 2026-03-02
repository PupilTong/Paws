# Agent Instructions

This repository supports LLM-based assistants. The working language is English.

## Supported Agents

- Google Antigravity (via `.agent/` workflows)
- Claude (via `CLAUDE.md` at repo root)
- GitHub Copilot

## General Guidelines

- Keep changes minimal and focused.
- Follow existing style conventions and formatting.
- Prefer Rust 2021 edition and stable toolchain.
- Use cargo workspace conventions.
- Update or add tests when relevant.
- Agents should always run tests.
- Ensure error handling returns specific ErrorCodes where applicable (avoid string errors).
- Ensure operations are transactional/atomic where possible (check preconditions before mutation).
- For non-thread-safe non-cryptographic keys (like integers), use `FnvHashMap` instead of `HashMap`.
- No `println!`/`eprintln!`/`dbg!` in production code.
- All `unsafe` blocks must have a `// SAFETY:` comment explaining the invariant.
- On every code change, assess whether agents.md needs an update and update it when needed.

## Repository Structure

- `engine/`: core logic (DOM, Style, Layout). A pure Rust library with no host dependencies.
- `wasm-bridge/`: integration layer threading `wasmtime` and `engine` together.
- `view/`: UI/view layer. Exposes the frontend APIs.
- `view-macros/`: Contains the `css!` proc-macro for compile-time CSS evaluation.
- `paws-style-ir/`: Zero-copy intermediate representations (`rkyv`) for styles shared between the macro and runtime.

## Project Design Overview

- Cross-platform framework with a pure WASM VM for UI logic (`wasm-bridge`).
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
