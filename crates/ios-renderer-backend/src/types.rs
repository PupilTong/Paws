//! Core types for the iOS renderer backend pipeline.
//!
//! All `#[repr(C)]` types are FFI-safe and appear in the generated C header
//! consumed by Swift via a bridging header.

// ── Geometry ──────────────────────────────────────────────────────────

/// Axis-aligned rectangle in screen-space coordinates.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Two-dimensional size.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

/// RGBA color with premultiplied alpha, components in `[0.0, 1.0]`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// Column-major 4×4 affine transform (matches `CATransform3D` layout).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform3D {
    pub m: [f32; 16],
}

impl Default for Transform3D {
    fn default() -> Self {
        #[rustfmt::skip]
        let m = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        Self { m }
    }
}

/// Flat, FFI-safe bundle of visual properties for a single `CALayer`.
///
/// `Option<Transform3D>` and `Option<Rect>` are encoded as a `bool` flag
/// (`has_transform`, `has_clip`) paired with the value so the struct stays
/// `#[repr(C)]`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerProps {
    pub frame: Rect,
    pub opacity: f32,
    pub background: Color,
    pub border_radius: f32,
    pub has_transform: bool,
    pub transform: Transform3D,
    pub has_clip: bool,
    pub clip: Rect,
}

// ── Input ─────────────────────────────────────────────────────────────

/// Unique identifier for a node in the layout tree.
pub type NodeId = u64;

/// Unique identifier for a composited layer.
pub type LayerId = u64;

/// Unique identifier for a scroll container.
pub type ScrollId = u64;

/// A node in the fully-computed layout tree produced by the engine.
///
/// The tree is owned by the caller and borrowed immutably during a single
/// `render_frame` pass.
#[derive(Clone, Debug)]
pub struct LayoutNode {
    pub id: NodeId,
    pub frame: Rect,
    pub children: Vec<LayoutNode>,
    pub scroll: Option<ScrollProps>,
    pub style: ComputedStyle,
    /// Incremented by the engine whenever this node changes.
    /// Used by `LayerTreeDiff` to skip unchanged subtrees in O(1).
    pub generation: u64,
}

/// Scroll-container metadata attached to a `LayoutNode`.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollProps {
    pub content_size: Size,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
}

/// CSS `overflow` axis value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    Visible,
    Hidden,
    Scroll,
    Auto,
}

/// Computed visual style for a single node, post-cascade.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedStyle {
    pub opacity: f32,
    pub transform: Option<Transform3D>,
    pub clip: Option<Rect>,
    pub background: Color,
    pub border_radius: f32,
    pub will_change: bool,
}

// ── Output ────────────────────────────────────────────────────────────

/// A single command that mutates the native layer tree on the Swift side.
///
/// Commands are emitted in painter's order (parents before children) and
/// written into a caller-allocated buffer via `rb_render_frame`.
#[repr(C)]
#[derive(Debug, Clone, PartialEq)]
pub enum LayerCmd {
    CreateLayer {
        id: LayerId,
        kind: LayerKind,
    },
    UpdateLayer {
        id: LayerId,
        props: LayerProps,
    },
    RemoveLayer {
        id: LayerId,
    },
    AttachScroll {
        id: LayerId,
        parent_id: LayerId,
        content_size: Size,
    },
    SetZOrder {
        id: LayerId,
        index: u32,
    },
    ReparentLayer {
        id: LayerId,
        new_parent: LayerId,
        parent_type: ParentKind,
    },
}

/// The kind of native view/layer to create on the Swift side.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    View,
    ScrollView,
    MetalLayer,
}

/// Describes whether a child is parented to a plain `CALayer` or a
/// `UIScrollView`'s content layer.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentKind {
    Layer,
    ScrollView,
}

// ── Diff ──────────────────────────────────────────────────────────────

/// The minimal set of commands needed to transform the previous frame's
/// layer tree into the current frame's layer tree.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LayerTreeDiff {
    pub created: Vec<LayerCmd>,
    pub updated: Vec<LayerCmd>,
    pub removed: Vec<LayerId>,
    pub reordered: Vec<LayerCmd>,
}

// ── Snapshot (internal) ───────────────────────────────────────────────

/// A single node in the layerized snapshot tree, used for diffing.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerizedNode {
    pub id: LayerId,
    pub kind: LayerKind,
    pub parent_id: Option<LayerId>,
    pub parent_kind: Option<ParentKind>,
    pub z_index: u32,
    pub props: LayerProps,
    pub scroll_content_size: Option<Size>,
    pub generation: u64,
    pub children: Vec<LayerizedNode>,
}

/// The complete layerized tree snapshot for a single frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerizedTree {
    pub root: Option<LayerizedNode>,
}
