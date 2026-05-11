<div align="center">

# Paws

**Browser-grade UI on native apps — driven by WASM, painted by the OS.**

[![CodSpeed](https://img.shields.io/endpoint?url=https://codspeed.io/badge.json)](https://codspeed.io/PupilTong/Paws?utm_source=badge)
[![License: MPL-2.0](https://img.shields.io/badge/license-MPL--2.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange?logo=rust)](rust-toolchain.toml)
[![WASM Component Model](https://img.shields.io/badge/wasm-component--model-654FF0)](wit/paws.wit)

</div>

---

## What is Paws?

Paws is a cross-platform UI framework with one unusual constraint:
**the native host is not allowed to mutate the UI tree.** Every element
created, every style applied, every event handled — all of it is driven
by a sandboxed WebAssembly guest. The host just renders pixels.

That gives you three properties at once:

- **Web semantics, no browser.** Stylo (Firefox's CSS engine) does the
  cascade. Taffy does the layout. So `:hover`, specificity, flexbox,
  inheritance — they all behave the way the web spec says they should.
- **One UI binary, many platforms.** Your app compiles to a single
  `wasm32-wasip2` component. iOS paints it with UIKit. A future wgpu
  backend will paint it with shaders. Same component, different paint.
- **A hard isolation boundary.** Guest code touches the DOM only through
  the [WIT-defined host interface](wit/paws.wit). No raw pointers, no
  shared memory tricks, no "just this once" FFI escape hatches.

You can write guests by hand, or write them as **Yew components** —
Paws ships a vendored Yew fork that targets the Paws host instead of
the browser DOM. React-ish ergonomics, native rendering.

---

## The Architecture

```
   ┌─────────────────────────────────────────────────────────┐
   │  Your UI code  (Rust → wasm32-wasip2 component)         │
   │  • plain rust-wasm-binding, or                          │
   │  • Yew components via the vendored fork                 │
   └────────────────────────┬────────────────────────────────┘
                            │  WIT contract  (wit/paws.wit)
                            │  dom · events · shadow · stylesheet · resources
                            ▼
   ┌─────────────────────────────────────────────────────────┐
   │  wasmtime-engine   — component-model linker + host impls│
   ├─────────────────────────────────────────────────────────┤
   │  engine            — DOM tree (Slab<PawsElement>)       │
   │                      Style  (Stylo cascade + RuleTree)  │
   │                      Layout (Taffy box model)           │
   │                      Hit-test, events, resources        │
   └────────────────────────┬────────────────────────────────┘
                            │  LayoutBox stream
                            ▼
   ┌─────────────────────────────────────────────────────────┐
   │  Renderer backend  — iOS (UIKit, today) · wgpu (next)   │
   └─────────────────────────────────────────────────────────┘
```

The arrow only points one way. The host never reaches up.

---

## Using Paws with an AI Agent

This repo is set up to be driven by an AI coding assistant. Drop one of
these prompts into Claude Code, Gemini, or Copilot — the agent already
has `CLAUDE.md`, `agents.md`, and the `.agent/` workflows to ground its
behavior. No need to memorize cargo invocations.

### Orienting

Paste these on a fresh checkout to get up to speed.

```
Give me a tour of this repo. Start from wit/paws.wit and walk outward —
where does a guest's create_element call end up, and what turns it into
something on screen?
```

```
Explain how Stylo is plugged in. Which file owns the cascade, and what
does the engine's DOM shim have to implement for Stylo to be happy?
```

```
Show me the smallest possible Paws guest. Then explain every line.
```

### Building & checking

The boring stuff. Let the agent handle the flags.

```
Run the full check matrix (fmt, clippy, tests) the way CI runs it.
Fix anything that breaks and explain what you changed.
```

```
Run the benchmarks under CodSpeed and tell me if anything regressed
on this branch versus main.
```

```
Capture WASM guest coverage end-to-end and report which guest crates
have the weakest coverage.
```

### Authoring UI

```
Add a new guest example called example-card-list that renders a
scrollable list of styled cards using inline styles. Register it the
way the other examples are registered, and confirm it builds.
```

```
Write a Yew component in examples/yew/ that toggles a "dark mode"
class on the root and shows a button to flip it. Use the existing
yew counter example as a template.
```

```
The host doesn't expose set-text-content yet. Add it end-to-end:
update wit/paws.wit, implement it on RuntimeState, wire host_impl,
and expose a safe wrapper in rust-wasm-binding. Add a test.
```

### Running on iOS

```
Build the iOS example app, install it on the booted simulator, launch
it, take a screenshot, and tell me whether the basic-element example
renders as a blue rectangle.
```

```
The iOS app shows a black screen for the yew-counter example. Run it
in the simulator, grab the device logs, and tell me what's wrong.
```

> The `ios-simulator-debug` skill knows the Rust-to-Swift FFI pipeline
> end-to-end — agents will use it automatically when iOS work comes up.

### Performance work

```
Pick the slowest benchmark in the suite, profile it, and propose one
concrete optimization. Don't implement it yet — show me the plan first.
```

> The `codspeed-optimize` skill is the right entry point — point your
> agent at a benchmark name and let it iterate.

---

## Crate Map

| Crate | What it does |
|---|---|
| [`engine/`](engine) | Pure-Rust DOM, Stylo-backed style resolution, Taffy layout, hit-test |
| [`wasmtime-engine/`](wasmtime-engine) | Component-model linker, host-trait impls, runtime state |
| [`rust-wasm-binding/`](rust-wasm-binding) | Guest-side library; wraps WIT imports + the `paws_main!` macro |
| [`wit/`](wit) | The WIT schema — source of truth for the host/guest contract |
| [`view-macros/`](view-macros) | `css!()` proc macro: compile-time CSS → rkyv IR |
| [`paws-style-ir/`](paws-style-ir) | Zero-copy style IR shared between the macro and the runtime |
| [`ios-renderer-backend/`](ios-renderer-backend) | UIKit bridge, cbindgen header, `PawsRenderer` Swift Package |
| [`ios-example-app/`](ios-example-app) | Xcode project demoing WASM → engine → UIKit |
| [`examples/`](examples) | Plain-binding guests (`example-*`) + Yew guests (`yew/example-yew-*`) |
| [`yew/`](yew) | Vendored Yew fork that targets the Paws host instead of the browser |

---

## Running It Yourself

Everything runs from the `Paws/` workspace root with a stable Rust
toolchain. CI runs the first three. The rest are situational.

| Goal | Command |
|---|---|
| Run all tests | `cargo test --all` |
| Check formatting | `cargo fmt --check` |
| Lint with warnings as errors | `cargo clippy --all-targets --all-features -D warnings` |
| Build a guest example | `cargo build -p example-basic-element --target wasm32-wasip2 --release` |
| Run a benchmark | `cargo codspeed build && cargo codspeed run` |
| iOS app | Open [`ios-example-app/`](ios-example-app) in Xcode and run on a simulator |

If you'd rather not memorize any of this, see the prompt section above.

---

## How the Agents Stay in Sync

Three assistants are wired into this repo and they all read from the
same playbook:

- **Claude** reads [`CLAUDE.md`](../CLAUDE.md) and [`agents.md`](agents.md)
- **Gemini (Google Antigravity)** reads `.agent/workflows/` and [`agents.md`](agents.md)
- **GitHub Copilot** reads [`agents.md`](agents.md)

`agents.md` is the canonical document. If you tweak coding standards,
tweak them there — not in the per-assistant overlay — and every agent
picks up the change on the next run.

---

## License

[Mozilla Public License 2.0](LICENSE) — the same license as Stylo and
Servo, which Paws inherits from.
