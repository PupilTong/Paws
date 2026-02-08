# Agent Instructions

This repository supports LLM-based assistants. The working language is English.

## Supported Agents

- Google Antigravity
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
- On every code change, assess whether agents.md needs an update and update it when needed.

## Repository Structure

- `engine/`: core logic and integration demos.
- `view/`: UI/view layer (currently placeholder).

## Project Design Overview

- Cross-platform framework with a pure WASM VM for UI logic.
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
