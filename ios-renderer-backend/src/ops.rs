//! Op-code buffer for the iOS rendering pipeline.
//!
//! The background thread generates [`RenderOp`]s by walking the `LayoutBox`
//! tree. These are packed into a contiguous [`OpBuffer`] using fixed 32-byte
//! slots, then sent to the iOS main thread for execution against UIKit.
//!
//! ## Wire format
//!
//! Every op occupies exactly **32 bytes**: `[tag:1][payload:31]`.
//! Swift reads ops by striding `32 * i` and switching on the tag byte.
//!
//! Variable-length data (text content) is stored in a separate string table.
//! Ops reference strings by `(offset: u32, len: u32)` into this table.

/// Op-code tag values written as the first byte of each 32-byte slot.
///
/// The payload layout for each tag is documented inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum OpTag {
    /// Create a UIView.
    /// `[tag:1][node_id:8][parent_id:8][padding:15]`
    DeclareView = 0x01,

    /// Create a UIScrollView.
    /// Same payload as `DeclareView`.
    DeclareScrollView = 0x02,

    /// Create a CALayer.
    /// Same payload as `DeclareView`.
    DeclareLayer = 0x03,

    /// Set frame (position + size) for a View or ScrollView.
    /// `[tag:1][node_id:8][x:4][y:4][w:4][h:4][padding:7]`
    SetViewFrame = 0x04,

    /// Set frame for a CALayer.
    /// `[tag:1][node_id:8][x:4][y:4][w:4][h:4][padding:7]`
    SetLayerFrame = 0x05,

    /// Set background color.
    /// `[tag:1][node_id:8][r:4][g:4][b:4][a:4][padding:7]`
    SetBgColor = 0x06,

    /// Set clips-to-bounds.
    /// `[tag:1][node_id:8][clips:1][padding:22]`
    SetClipsToBounds = 0x07,

    /// Set scroll content size for a UIScrollView.
    /// `[tag:1][node_id:8][w:4][h:4][padding:15]`
    SetContentSize = 0x08,

    /// Detach a View or ScrollView from its superview.
    /// `[tag:1][node_id:8][padding:23]`
    DetachView = 0x09,

    /// Detach a CALayer from its superlayer.
    /// `[tag:1][node_id:8][padding:23]`
    DetachLayer = 0x0A,

    /// Release a UIView.
    /// `[tag:1][node_id:8][padding:23]`
    ReleaseView = 0x0B,

    /// Release a UIScrollView.
    /// `[tag:1][node_id:8][padding:23]`
    ReleaseScrollView = 0x0C,

    /// Release a CALayer.
    /// `[tag:1][node_id:8][padding:23]`
    ReleaseLayer = 0x0D,

    /// Re-attach a node to a (new) parent.
    /// `[tag:1][node_id:8][parent_id:8][kind:1][parent_kind:1][padding:13]`
    Attach = 0x0E,

    // ── Text ops ────────────────────────────────────────────────────
    /// Create a CATextLayer.
    /// `[tag:1][node_id:8][parent_id:8][padding:15]`
    DeclareText = 0x10,

    /// Set text content (references string table).
    /// `[tag:1][node_id:8][str_offset:4][str_len:4][padding:15]`
    SetTextContent = 0x11,

    /// Set font size and weight.
    /// `[tag:1][node_id:8][font_size:4][font_weight:4][padding:15]`
    SetTextFont = 0x12,

    /// Set text foreground color.
    /// `[tag:1][node_id:8][r:4][g:4][b:4][a:4][padding:7]`
    SetTextColor = 0x13,

    /// Detach a CATextLayer from its superlayer.
    /// `[tag:1][node_id:8][padding:23]`
    DetachText = 0x14,

    /// Release a CATextLayer.
    /// `[tag:1][node_id:8][padding:23]`
    ReleaseText = 0x15,
}

/// Slot size in bytes. Every op occupies exactly this many bytes.
pub(crate) const SLOT_SIZE: usize = 32;

/// A contiguous buffer of 32-byte op-code slots with an auxiliary string table.
///
/// The buffer is reused across frames via [`clear`](OpBuffer::clear).
/// Variable-length text content is stored in the `strings` table and
/// referenced by (offset, length) from `SetTextContent` ops.
pub(crate) struct OpBuffer {
    data: Vec<u8>,
    strings: Vec<u8>,
}

impl OpBuffer {
    /// Creates an empty buffer.
    pub(crate) fn new() -> Self {
        Self {
            data: Vec::with_capacity(SLOT_SIZE * 64),
            strings: Vec::new(),
        }
    }

    /// Clears the buffer for reuse without deallocating.
    pub(crate) fn clear(&mut self) {
        self.data.clear();
        self.strings.clear();
    }

    /// Returns a pointer to the raw op bytes.
    pub(crate) fn as_ptr(&self) -> *const u8 {
        self.data.as_ptr()
    }

    /// Returns the total byte length of the ops buffer (always a multiple of [`SLOT_SIZE`]).
    pub(crate) fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns a pointer to the string table bytes.
    pub(crate) fn strings_ptr(&self) -> *const u8 {
        self.strings.as_ptr()
    }

    /// Returns the byte length of the string table.
    pub(crate) fn strings_len(&self) -> usize {
        self.strings.len()
    }

    /// Returns the number of ops in the buffer.
    #[cfg(test)]
    pub(crate) fn op_count(&self) -> usize {
        self.data.len() / SLOT_SIZE
    }

    // ── Declare ops ────────────────────────────────────────────────

    /// Emits a Declare op (View, ScrollView, or Layer) with parent info.
    pub(crate) fn push_declare(&mut self, kind: ViewKind, node_id: u64, parent_id: u64) {
        let tag = match kind {
            ViewKind::View => OpTag::DeclareView,
            ViewKind::ScrollView => OpTag::DeclareScrollView,
            ViewKind::Layer => OpTag::DeclareLayer,
            ViewKind::Text => OpTag::DeclareText,
        };
        let mut slot = [0u8; SLOT_SIZE];
        slot[0] = tag as u8;
        slot[1..9].copy_from_slice(&node_id.to_le_bytes());
        slot[9..17].copy_from_slice(&parent_id.to_le_bytes());
        self.data.extend_from_slice(&slot);
    }

    // ── Frame ops ──────────────────────────────────────────────────

    /// Emits a SetViewFrame, SetLayerFrame, or SetTextFrame op.
    ///
    /// Text nodes reuse `SetLayerFrame` since CATextLayer is a CALayer subclass.
    pub(crate) fn push_set_frame(
        &mut self,
        kind: ViewKind,
        node_id: u64,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        let tag = match kind {
            ViewKind::Layer | ViewKind::Text => OpTag::SetLayerFrame,
            _ => OpTag::SetViewFrame,
        };
        let mut slot = [0u8; SLOT_SIZE];
        slot[0] = tag as u8;
        slot[1..9].copy_from_slice(&node_id.to_le_bytes());
        slot[9..13].copy_from_slice(&x.to_le_bytes());
        slot[13..17].copy_from_slice(&y.to_le_bytes());
        slot[17..21].copy_from_slice(&w.to_le_bytes());
        slot[21..25].copy_from_slice(&h.to_le_bytes());
        self.data.extend_from_slice(&slot);
    }

    // ── Property ops ───────────────────────────────────────────────

    /// Emits a SetBgColor op.
    pub(crate) fn push_bg_color(&mut self, node_id: u64, r: f32, g: f32, b: f32, a: f32) {
        let mut slot = [0u8; SLOT_SIZE];
        slot[0] = OpTag::SetBgColor as u8;
        slot[1..9].copy_from_slice(&node_id.to_le_bytes());
        slot[9..13].copy_from_slice(&r.to_le_bytes());
        slot[13..17].copy_from_slice(&g.to_le_bytes());
        slot[17..21].copy_from_slice(&b.to_le_bytes());
        slot[21..25].copy_from_slice(&a.to_le_bytes());
        self.data.extend_from_slice(&slot);
    }

    /// Emits a SetClipsToBounds op.
    pub(crate) fn push_clips(&mut self, node_id: u64, clips: bool) {
        let mut slot = [0u8; SLOT_SIZE];
        slot[0] = OpTag::SetClipsToBounds as u8;
        slot[1..9].copy_from_slice(&node_id.to_le_bytes());
        slot[9] = clips as u8;
        self.data.extend_from_slice(&slot);
    }

    /// Emits a SetContentSize op for scroll views.
    pub(crate) fn push_content_size(&mut self, node_id: u64, w: f32, h: f32) {
        let mut slot = [0u8; SLOT_SIZE];
        slot[0] = OpTag::SetContentSize as u8;
        slot[1..9].copy_from_slice(&node_id.to_le_bytes());
        slot[9..13].copy_from_slice(&w.to_le_bytes());
        slot[13..17].copy_from_slice(&h.to_le_bytes());
        self.data.extend_from_slice(&slot);
    }

    // ── Text property ops ──────────────────────────────────────────

    /// Appends text to the string table and emits a SetTextContent op.
    pub(crate) fn push_text_content(&mut self, node_id: u64, text: &str) {
        let offset = self.strings.len() as u32;
        let len = text.len() as u32;
        self.strings.extend_from_slice(text.as_bytes());

        let mut slot = [0u8; SLOT_SIZE];
        slot[0] = OpTag::SetTextContent as u8;
        slot[1..9].copy_from_slice(&node_id.to_le_bytes());
        slot[9..13].copy_from_slice(&offset.to_le_bytes());
        slot[13..17].copy_from_slice(&len.to_le_bytes());
        self.data.extend_from_slice(&slot);
    }

    /// Emits a SetTextFont op with font size and weight.
    pub(crate) fn push_text_font(&mut self, node_id: u64, font_size: f32, font_weight: f32) {
        let mut slot = [0u8; SLOT_SIZE];
        slot[0] = OpTag::SetTextFont as u8;
        slot[1..9].copy_from_slice(&node_id.to_le_bytes());
        slot[9..13].copy_from_slice(&font_size.to_le_bytes());
        slot[13..17].copy_from_slice(&font_weight.to_le_bytes());
        self.data.extend_from_slice(&slot);
    }

    /// Emits a SetTextColor op.
    pub(crate) fn push_text_color(&mut self, node_id: u64, r: f32, g: f32, b: f32, a: f32) {
        let mut slot = [0u8; SLOT_SIZE];
        slot[0] = OpTag::SetTextColor as u8;
        slot[1..9].copy_from_slice(&node_id.to_le_bytes());
        slot[9..13].copy_from_slice(&r.to_le_bytes());
        slot[13..17].copy_from_slice(&g.to_le_bytes());
        slot[17..21].copy_from_slice(&b.to_le_bytes());
        slot[21..25].copy_from_slice(&a.to_le_bytes());
        self.data.extend_from_slice(&slot);
    }

    // ── Detach / Release ops ───────────────────────────────────────

    /// Emits a Detach op (view, layer, or text).
    pub(crate) fn push_detach(&mut self, node_id: u64, kind: ViewKind) {
        let tag = match kind {
            ViewKind::Layer => OpTag::DetachLayer,
            ViewKind::Text => OpTag::DetachText,
            _ => OpTag::DetachView,
        };
        let mut slot = [0u8; SLOT_SIZE];
        slot[0] = tag as u8;
        slot[1..9].copy_from_slice(&node_id.to_le_bytes());
        self.data.extend_from_slice(&slot);
    }

    /// Emits a Release op (view, scroll view, layer, or text).
    pub(crate) fn push_release(&mut self, node_id: u64, kind: ViewKind) {
        let tag = match kind {
            ViewKind::View => OpTag::ReleaseView,
            ViewKind::ScrollView => OpTag::ReleaseScrollView,
            ViewKind::Layer => OpTag::ReleaseLayer,
            ViewKind::Text => OpTag::ReleaseText,
        };
        let mut slot = [0u8; SLOT_SIZE];
        slot[0] = tag as u8;
        slot[1..9].copy_from_slice(&node_id.to_le_bytes());
        self.data.extend_from_slice(&slot);
    }

    /// Emits an Attach op to re-parent a node.
    pub(crate) fn push_attach(
        &mut self,
        node_id: u64,
        kind: ViewKind,
        parent_id: u64,
        parent_kind: ViewKind,
    ) {
        let mut slot = [0u8; SLOT_SIZE];
        slot[0] = OpTag::Attach as u8;
        slot[1..9].copy_from_slice(&node_id.to_le_bytes());
        slot[9..17].copy_from_slice(&parent_id.to_le_bytes());
        slot[17] = kind as u8;
        slot[18] = parent_kind as u8;
        self.data.extend_from_slice(&slot);
    }

    // ── Test helpers ───────────────────────────────────────────────

    /// Reads an op tag at the given slot index (for testing/debugging).
    #[cfg(test)]
    pub(crate) fn tag_at(&self, index: usize) -> Option<u8> {
        let offset = index * SLOT_SIZE;
        self.data.get(offset).copied()
    }

    /// Reads raw bytes at the given slot index (for testing/debugging).
    #[cfg(test)]
    pub(crate) fn slot_at(&self, index: usize) -> Option<&[u8]> {
        let offset = index * SLOT_SIZE;
        self.data.get(offset..offset + SLOT_SIZE)
    }

    /// Returns the string table bytes (for testing).
    #[cfg(test)]
    pub(crate) fn strings_data(&self) -> &[u8] {
        &self.strings
    }
}

/// Discriminant for the type of UIKit object backing a node.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub(crate) enum ViewKind {
    /// A plain `UIView`.
    #[default]
    View = 0,
    /// A `UIScrollView`.
    ScrollView = 1,
    /// A standalone `CALayer`.
    Layer = 2,
    /// A `CATextLayer` for rendering text.
    Text = 3,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_buffer() {
        let buf = OpBuffer::new();
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.op_count(), 0);
        assert_eq!(buf.strings_len(), 0);
    }

    #[test]
    fn test_push_declare_view() {
        let mut buf = OpBuffer::new();
        buf.push_declare(ViewKind::View, 42, u64::MAX);
        assert_eq!(buf.op_count(), 1);
        assert_eq!(buf.len(), SLOT_SIZE);
        assert_eq!(buf.tag_at(0), Some(OpTag::DeclareView as u8));

        // Verify node_id
        let slot = buf.slot_at(0).unwrap();
        let node_id = u64::from_le_bytes(slot[1..9].try_into().unwrap());
        assert_eq!(node_id, 42);

        // Verify parent_id
        let parent_id = u64::from_le_bytes(slot[9..17].try_into().unwrap());
        assert_eq!(parent_id, u64::MAX);
    }

    #[test]
    fn test_push_set_frame() {
        let mut buf = OpBuffer::new();
        buf.push_set_frame(ViewKind::View, 10, 1.0, 2.0, 100.0, 50.0);
        assert_eq!(buf.op_count(), 1);
        assert_eq!(buf.tag_at(0), Some(OpTag::SetViewFrame as u8));

        let slot = buf.slot_at(0).unwrap();
        let x = f32::from_le_bytes(slot[9..13].try_into().unwrap());
        let y = f32::from_le_bytes(slot[13..17].try_into().unwrap());
        let w = f32::from_le_bytes(slot[17..21].try_into().unwrap());
        let h = f32::from_le_bytes(slot[21..25].try_into().unwrap());
        assert_eq!((x, y, w, h), (1.0, 2.0, 100.0, 50.0));
    }

    #[test]
    fn test_push_layer_frame() {
        let mut buf = OpBuffer::new();
        buf.push_set_frame(ViewKind::Layer, 5, 0.0, 0.0, 50.0, 50.0);
        assert_eq!(buf.tag_at(0), Some(OpTag::SetLayerFrame as u8));
    }

    #[test]
    fn test_push_bg_color() {
        let mut buf = OpBuffer::new();
        buf.push_bg_color(7, 1.0, 0.0, 0.5, 1.0);
        assert_eq!(buf.tag_at(0), Some(OpTag::SetBgColor as u8));

        let slot = buf.slot_at(0).unwrap();
        let r = f32::from_le_bytes(slot[9..13].try_into().unwrap());
        let a = f32::from_le_bytes(slot[21..25].try_into().unwrap());
        assert_eq!(r, 1.0);
        assert_eq!(a, 1.0);
    }

    #[test]
    fn test_push_clips() {
        let mut buf = OpBuffer::new();
        buf.push_clips(3, true);
        assert_eq!(buf.tag_at(0), Some(OpTag::SetClipsToBounds as u8));
        let slot = buf.slot_at(0).unwrap();
        assert_eq!(slot[9], 1);
    }

    #[test]
    fn test_push_detach_and_release() {
        let mut buf = OpBuffer::new();
        buf.push_detach(1, ViewKind::View);
        buf.push_release(1, ViewKind::View);
        buf.push_detach(2, ViewKind::Layer);
        buf.push_release(2, ViewKind::Layer);
        buf.push_release(3, ViewKind::ScrollView);
        buf.push_detach(4, ViewKind::Text);
        buf.push_release(4, ViewKind::Text);
        assert_eq!(buf.op_count(), 7);
        assert_eq!(buf.tag_at(0), Some(OpTag::DetachView as u8));
        assert_eq!(buf.tag_at(1), Some(OpTag::ReleaseView as u8));
        assert_eq!(buf.tag_at(2), Some(OpTag::DetachLayer as u8));
        assert_eq!(buf.tag_at(3), Some(OpTag::ReleaseLayer as u8));
        assert_eq!(buf.tag_at(4), Some(OpTag::ReleaseScrollView as u8));
        assert_eq!(buf.tag_at(5), Some(OpTag::DetachText as u8));
        assert_eq!(buf.tag_at(6), Some(OpTag::ReleaseText as u8));
    }

    #[test]
    fn test_clear_resets_buffer() {
        let mut buf = OpBuffer::new();
        buf.push_declare(ViewKind::View, 1, u64::MAX);
        buf.push_set_frame(ViewKind::View, 1, 0.0, 0.0, 100.0, 100.0);
        buf.push_text_content(1, "hello");
        assert_eq!(buf.op_count(), 3);
        assert!(buf.strings_len() > 0);

        buf.clear();
        assert_eq!(buf.op_count(), 0);
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.strings_len(), 0);
    }

    #[test]
    fn test_multiple_ops_in_sequence() {
        let mut buf = OpBuffer::new();
        buf.push_declare(ViewKind::View, 1, u64::MAX);
        buf.push_set_frame(ViewKind::View, 1, 0.0, 0.0, 320.0, 480.0);
        buf.push_declare(ViewKind::Layer, 2, 1);
        buf.push_set_frame(ViewKind::Layer, 2, 10.0, 10.0, 50.0, 50.0);
        buf.push_bg_color(2, 1.0, 0.0, 0.0, 1.0);

        assert_eq!(buf.op_count(), 5);
        assert_eq!(buf.len(), 5 * SLOT_SIZE);

        // Verify each tag
        assert_eq!(buf.tag_at(0), Some(OpTag::DeclareView as u8));
        assert_eq!(buf.tag_at(1), Some(OpTag::SetViewFrame as u8));
        assert_eq!(buf.tag_at(2), Some(OpTag::DeclareLayer as u8));
        assert_eq!(buf.tag_at(3), Some(OpTag::SetLayerFrame as u8));
        assert_eq!(buf.tag_at(4), Some(OpTag::SetBgColor as u8));
    }

    #[test]
    fn test_push_attach() {
        let mut buf = OpBuffer::new();
        buf.push_attach(5, ViewKind::Layer, 1, ViewKind::View);
        assert_eq!(buf.tag_at(0), Some(OpTag::Attach as u8));

        let slot = buf.slot_at(0).unwrap();
        let node_id = u64::from_le_bytes(slot[1..9].try_into().unwrap());
        let parent_id = u64::from_le_bytes(slot[9..17].try_into().unwrap());
        assert_eq!(node_id, 5);
        assert_eq!(parent_id, 1);
        assert_eq!(slot[17], ViewKind::Layer as u8);
        assert_eq!(slot[18], ViewKind::View as u8);
    }

    // ── Text op tests ──────────────────────────────────────────────

    #[test]
    fn test_push_declare_text() {
        let mut buf = OpBuffer::new();
        buf.push_declare(ViewKind::Text, 10, 5);
        assert_eq!(buf.tag_at(0), Some(OpTag::DeclareText as u8));

        let slot = buf.slot_at(0).unwrap();
        let node_id = u64::from_le_bytes(slot[1..9].try_into().unwrap());
        let parent_id = u64::from_le_bytes(slot[9..17].try_into().unwrap());
        assert_eq!(node_id, 10);
        assert_eq!(parent_id, 5);
    }

    #[test]
    fn test_push_text_content() {
        let mut buf = OpBuffer::new();
        buf.push_text_content(7, "Hello Paws");
        assert_eq!(buf.tag_at(0), Some(OpTag::SetTextContent as u8));

        let slot = buf.slot_at(0).unwrap();
        let offset = u32::from_le_bytes(slot[9..13].try_into().unwrap());
        let len = u32::from_le_bytes(slot[13..17].try_into().unwrap());
        assert_eq!(offset, 0);
        assert_eq!(len, 10);

        // Verify string table
        let text = std::str::from_utf8(&buf.strings_data()[0..10]).unwrap();
        assert_eq!(text, "Hello Paws");
    }

    #[test]
    fn test_push_text_content_multiple() {
        let mut buf = OpBuffer::new();
        buf.push_text_content(1, "aaa");
        buf.push_text_content(2, "bbbbb");

        // Second string starts at offset 3
        let slot = buf.slot_at(1).unwrap();
        let offset = u32::from_le_bytes(slot[9..13].try_into().unwrap());
        let len = u32::from_le_bytes(slot[13..17].try_into().unwrap());
        assert_eq!(offset, 3);
        assert_eq!(len, 5);
    }

    #[test]
    fn test_push_text_font() {
        let mut buf = OpBuffer::new();
        buf.push_text_font(5, 16.0, 700.0);
        assert_eq!(buf.tag_at(0), Some(OpTag::SetTextFont as u8));

        let slot = buf.slot_at(0).unwrap();
        let size = f32::from_le_bytes(slot[9..13].try_into().unwrap());
        let weight = f32::from_le_bytes(slot[13..17].try_into().unwrap());
        assert_eq!(size, 16.0);
        assert_eq!(weight, 700.0);
    }

    #[test]
    fn test_push_text_color() {
        let mut buf = OpBuffer::new();
        buf.push_text_color(3, 0.0, 0.0, 0.0, 1.0);
        assert_eq!(buf.tag_at(0), Some(OpTag::SetTextColor as u8));

        let slot = buf.slot_at(0).unwrap();
        let a = f32::from_le_bytes(slot[21..25].try_into().unwrap());
        assert_eq!(a, 1.0);
    }

    #[test]
    fn test_text_frame_uses_layer_tag() {
        let mut buf = OpBuffer::new();
        buf.push_set_frame(ViewKind::Text, 10, 0.0, 0.0, 80.0, 20.0);
        assert_eq!(
            buf.tag_at(0),
            Some(OpTag::SetLayerFrame as u8),
            "text frame should use SetLayerFrame since CATextLayer is a CALayer"
        );
    }
}
