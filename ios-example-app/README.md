# ios-example-app

A minimal iOS app that demonstrates the Paws rendering pipeline end-to-end:
Rust `LayoutNode` tree → 5-stage pipeline → `LayerCmd` stream → live UIKit views.

## Prerequisites

- macOS with Xcode 15+
- Rust toolchain with iOS targets:
  ```bash
  rustup target add aarch64-apple-ios aarch64-apple-ios-sim
  ```

## Build & Run

### 1. Build Rust staticlibs

The Xcode project includes a build phase script that runs `cargo build`
automatically. However, you can also build manually:

```bash
# For iOS Simulator (Apple Silicon)
cd Paws
cargo build -p ios-example -p ios-renderer-backend --target aarch64-apple-ios-sim --release

# For device
cargo build -p ios-example -p ios-renderer-backend --target aarch64-apple-ios --release
```

### 2. Open in Xcode

```bash
open ios-example-app/ios-example-app.xcodeproj
```

### 3. Run

Select an iOS Simulator or device target and press **Cmd+R**.

The app displays a scrollable list of 20 colored rows, all rendered
through the Rust pipeline. Scrolling is handled by `UIScrollView` with
offsets forwarded to the Rust `ScrollRegistry` via `example_update_scroll`.

## Architecture

```
┌─────────────────────────────────────────┐
│  Swift (ios-example-app)                │
│  RendererViewController                 │
│    → CADisplayLink fires each frame     │
│    → RendererBridge.tick()              │
│    → LayerApplicator.apply(cmds)        │
│    → UIScrollViewDelegate → bridge      │
└──────────────┬──────────────────────────┘
               │ FFI (extern "C")
┌──────────────▼──────────────────────────┐
│  Rust (ios-example crate)               │
│  example_create / example_tick          │
│    → builds LayoutNode tree             │
│    → calls rb_submit_layout             │
│    → calls rb_render_frame              │
└──────────────┬──────────────────────────┘
               │
┌──────────────▼──────────────────────────┐
│  Rust (ios-renderer-backend)            │
│  Cull → Layerize → Flatten → Diff → Emit│
│  Produces LayerCmd stream               │
└─────────────────────────────────────────┘
```

## Testing (Rust only, no Xcode needed)

```bash
cargo test -p ios-example
```
