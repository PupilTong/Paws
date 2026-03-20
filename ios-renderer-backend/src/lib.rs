//! iOS renderer backend — incremental layer command pipeline.
//!
//! Consumes a fully computed [`types::LayoutNode`] tree from the engine
//! and produces a minimal [`types::LayerCmd`] stream delivered via
//! `extern "C"` FFI to a Swift "dumb renderer" that drives native iOS
//! `UIView` / `CALayer` / `UIScrollView`.
//!
//! # Pipeline stages
//!
//! 1. **Cull** — viewport + prefetch region filtering
//! 2. **Layerize** — determine which nodes need their own native layer
//! 3. **Flatten** — bottom-up merge into nearest qualifying ancestor
//! 4. **Diff** — compare against previous frame, emit minimal delta
//! 5. **Emit** — write commands into caller-allocated buffer

pub(crate) mod convert;
pub(crate) mod cull;
pub(crate) mod diff;
pub mod ffi;
pub(crate) mod flatten;
pub(crate) mod layerize;
pub(crate) mod pipeline;
pub(crate) mod scroll;
pub mod types;
