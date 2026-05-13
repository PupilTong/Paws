//! iOS renderer backend for Paws.
//!
//! The rendering pipeline runs on a background thread:
//! 1. WASM execution mutates the DOM via `RuntimeState`
//! 2. Stylo resolves CSS styles
//! 3. Taffy computes layout
//! 4. `ViewTree` generates minimal updating op-codes
//! 5. Op-codes are sent to Swift's main thread for UIKit execution
//!
//! The op-code buffer is a flat array of 32-byte slots passed via a
//! completion callback. Variable-length text content is passed alongside
//! in a separate string table buffer.

mod error;
pub(crate) mod ffi;
mod image;
mod ops;
mod renderer;
mod thread;

/// Test-only surface exposed for WPT-style end-to-end verification.
///
/// These re-exports let `paws-wpt` host-side tests drive the iOS
/// renderer's op-emission pipeline against a fully WASM-executed
/// fixture: the test runs the guest wasm under `paws_runner` with a
/// `Document<IosNodeState>`, then calls into `process_into_op_tags` to
/// run the ViewTree and inspect the emitted op tags. This is the
/// renderer-side equivalent of the engine-side `RuntimeState`
/// inspection in `paws-wpt :: dom_nodes`.
///
/// The wrapper is intentionally minimal — it returns only the flat op
/// tag sequence, not the raw byte slots. Tests assert on
/// presence/absence of specific tags (e.g. `SetClipsToBounds` for
/// Layer-kind clipping), which is the spec-level surface CSS Overflow
/// 3 §2.1 ultimately mandates ("clip descendants to the box's bounds")
/// without leaking the internal 32-byte op format.
pub mod test_support {
    use crate::ops::OpTag;
    use crate::renderer::ViewTree;
    use engine::dom::Document;
    use engine::{NodeId, ResourceResolver};

    /// Re-export of `IosNodeState` so `paws_runner::Runner::builder()
    /// .renderer(...)` callers can specify the iOS render state type
    /// without depending on the internal module path.
    pub use crate::renderer::IosNodeState;

    /// Runs the iOS renderer's `ViewTree::process` against `doc` and
    /// returns the sequence of op tag bytes that would be sent to the
    /// Swift side. The op tag byte values are the
    /// [`crate::ops::OpTag`] discriminants; tests compare against
    /// `OpTag::SetClipsToBounds as u8` etc.
    ///
    /// Skips the real iOS C callback (this is host-side test code).
    pub fn process_into_op_tags(
        doc: &mut Document<IosNodeState>,
        resources: &dyn ResourceResolver,
        root: Option<NodeId>,
    ) -> Vec<u8> {
        let mut tree = ViewTree::new();
        tree.process(doc, resources, root);
        let ops = tree.ops();
        (0..ops.op_count()).filter_map(|i| ops.tag_at(i)).collect()
    }

    /// `OpTag` byte for `SetClipsToBounds`, exposed for assertion
    /// against the slice returned by [`process_into_op_tags`].
    pub const OP_TAG_SET_CLIPS_TO_BOUNDS: u8 = OpTag::SetClipsToBounds as u8;

    /// `OpTag` byte for `DeclareLayer` — confirms a node is rendered
    /// as a `ViewKind::Layer` (CALayer-backed).
    pub const OP_TAG_DECLARE_LAYER: u8 = OpTag::DeclareLayer as u8;

    /// `OpTag` byte for `DeclareScrollView`. Useful to assert a node
    /// did NOT end up as a scroll container when the test expects
    /// Layer kind.
    pub const OP_TAG_DECLARE_SCROLL_VIEW: u8 = OpTag::DeclareScrollView as u8;
}

/// Shared test utilities used by `thread::tests` and `ffi::exports::tests`.
#[cfg(test)]
pub(crate) mod test_util {
    /// No-op completion callback for tests.
    pub(crate) extern "C" fn noop_completion(
        _ops: *const u8,
        _ops_len: usize,
        _strings: *const u8,
        _strings_len: usize,
        _ctx: *mut std::ffi::c_void,
    ) {
    }

    /// WAT module that creates a styled div and commits.
    ///
    /// Creates a `<div>` with `width: 100px`, appends it to the document root,
    /// and calls `__commit` to trigger the rendering pipeline.
    pub(crate) fn make_wat_module() -> &'static str {
        r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_inline_style" (func $style (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__commit" (func $commit (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "width\00")
  (data (i32.const 32) "100px\00")
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $id)))
    (drop (call $style (local.get $id) (i32.const 16) (i32.const 32)))
    (drop (call $commit))
    (i32.const 0)
  )
)
"#
    }
}
