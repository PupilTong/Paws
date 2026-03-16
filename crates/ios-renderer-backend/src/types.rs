// ── Types used by the API but originally implicitly assumed ────────────

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform3D {
    pub m: [f32; 16],
}

impl Default for Transform3D {
    fn default() -> Self {
        Self {
            m: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        }
    }
}

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

// ── Input ──────────────────────────────────────────────────────────────

pub type NodeId = u64;
pub type LayerId = u64;
pub type ScrollId = u64;

#[derive(Clone, Debug)]
pub struct LayoutNode {
    pub id: NodeId,
    pub frame: Rect,
    pub children: Vec<LayoutNode>,
    pub scroll: Option<ScrollProps>,
    pub style: ComputedStyle,
    /// Incremented by the engine whenever this node changes.
    /// Used by LayerTreeDiff to skip unchanged subtrees in O(1).
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollProps {
    pub content_size: Size,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    Visible,
    Hidden,
    Scroll,
    Auto,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComputedStyle {
    pub opacity: f32,
    pub transform: Option<Transform3D>,
    pub clip: Option<Rect>,
    pub background: Color,
    pub border_radius: f32,
    pub will_change: bool,
}

// ── Output ─────────────────────────────────────────────────────────────

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

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    View,
    ScrollView,
    MetalLayer,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentKind {
    Layer,
    ScrollView,
}

// ── Diff ───────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, PartialEq)]
pub struct LayerTreeDiff {
    pub created: Vec<LayerCmd>,
    pub updated: Vec<LayerCmd>,
    pub removed: Vec<LayerId>,
    pub reordered: Vec<LayerCmd>,
}

// ── Snapshot ───────────────────────────────────────────────────────────

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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerizedTree {
    pub root: Option<LayerizedNode>,
}
