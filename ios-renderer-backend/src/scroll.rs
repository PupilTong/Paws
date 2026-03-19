//! Scroll offset registry with lock-free atomic reads.
//!
//! Swift calls [`ScrollRegistry::update_offset`] from
//! `UIScrollViewDelegate.scrollViewDidScroll` on the main thread.
//! The render pipeline reads offsets on the rayon pool via
//! [`ScrollRegistry::get_offset`] with `Acquire` ordering.

use crate::types::*;
use fnv::FnvHashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Internal metadata for a single scroll container.
///
/// Fields besides `dirty` are stored for the Swift consumer and for
/// `ancestor_chain` traversal; they are not read by the Rust pipeline
/// directly.
pub(crate) struct ScrollNode {
    #[allow(dead_code)]
    pub(crate) layer_id: LayerId,
    pub(crate) parent_scroll: Option<ScrollId>,
    #[allow(dead_code)]
    pub(crate) content_size: Size,
    pub(crate) dirty: AtomicBool,
}

/// Thread-safe registry of scroll containers and their current offsets.
///
/// Offsets are packed into [`AtomicU64`] values (high 32 bits = x, low 32
/// bits = y) so that reads and writes are a single atomic operation with
/// no locking.
pub struct ScrollRegistry {
    nodes: FnvHashMap<ScrollId, ScrollNode>,
    /// Packed f32 pair: high 32 bits = `x.to_bits()`, low 32 bits = `y.to_bits()`.
    offsets: FnvHashMap<ScrollId, AtomicU64>,
}

impl Default for ScrollRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            nodes: FnvHashMap::default(),
            offsets: FnvHashMap::default(),
        }
    }

    /// Register a scroll container with its initial offset.
    #[allow(dead_code)]
    pub fn insert(
        &mut self,
        id: ScrollId,
        layer_id: LayerId,
        parent_scroll: Option<ScrollId>,
        content_size: Size,
        offset_x: f32,
        offset_y: f32,
    ) {
        self.nodes.insert(
            id,
            ScrollNode {
                layer_id,
                parent_scroll,
                content_size,
                dirty: AtomicBool::new(false),
            },
        );
        let packed = pack_offset(offset_x, offset_y);
        self.offsets.insert(id, AtomicU64::new(packed));
    }

    /// Update the scroll offset for `id`.
    ///
    /// Called from Swift on `scrollViewDidScroll:` — may arrive on the main
    /// thread while the rayon pool is reading offsets concurrently.
    pub fn update_offset(&self, id: ScrollId, x: f32, y: f32) {
        if let Some(offset) = self.offsets.get(&id) {
            offset.store(pack_offset(x, y), Ordering::Release);
        }
        if let Some(node) = self.nodes.get(&id) {
            node.dirty.store(true, Ordering::Release);
        }
    }

    /// Read the current offset for `id` (Acquire ordering).
    pub fn get_offset(&self, id: ScrollId) -> (f32, f32) {
        if let Some(offset) = self.offsets.get(&id) {
            unpack_offset(offset.load(Ordering::Acquire))
        } else {
            (0.0, 0.0)
        }
    }

    /// Walk from `id` up to the root scroll container, returning the chain.
    #[allow(dead_code)]
    pub fn ancestor_chain(&self, id: ScrollId) -> Vec<ScrollId> {
        let mut chain = Vec::new();
        let mut current = Some(id);
        while let Some(curr_id) = current {
            chain.push(curr_id);
            current = self.nodes.get(&curr_id).and_then(|n| n.parent_scroll);
        }
        chain
    }

    /// Atomically read and clear the dirty flag. Returns the previous value.
    pub fn take_dirty(&self, id: ScrollId) -> bool {
        self.nodes
            .get(&id)
            .is_some_and(|n| n.dirty.swap(false, Ordering::AcqRel))
    }

    /// Remove all registered scroll containers.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.offsets.clear();
    }
}

/// Pack two `f32` values into a single `u64`.
fn pack_offset(x: f32, y: f32) -> u64 {
    (x.to_bits() as u64) << 32 | y.to_bits() as u64
}

/// Unpack two `f32` values from a single `u64`.
fn unpack_offset(packed: u64) -> (f32, f32) {
    let x_bits = (packed >> 32) as u32;
    let y_bits = (packed & 0xFFFF_FFFF) as u32;
    (f32::from_bits(x_bits), f32::from_bits(y_bits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn pack_unpack_roundtrip() {
        let (x, y) = (42.5, -100.25);
        let packed = pack_offset(x, y);
        let (rx, ry) = unpack_offset(packed);
        assert_eq!(rx, x);
        assert_eq!(ry, y);
    }

    #[test]
    fn concurrent_scroll_updates() {
        let mut reg = ScrollRegistry::new();
        reg.insert(
            1,
            10,
            None,
            Size {
                width: 1000.0,
                height: 1000.0,
            },
            0.0,
            0.0,
        );

        let reg = Arc::new(reg);

        let reg_writer = reg.clone();
        let writer = thread::spawn(move || {
            for i in 1..=100 {
                reg_writer.update_offset(1, i as f32, (i * 2) as f32);
            }
        });

        let reg_reader = reg.clone();
        let reader = thread::spawn(move || {
            for _ in 0..1000 {
                if reg_reader.take_dirty(1) {
                    let (x, y) = reg_reader.get_offset(1);
                    assert!(x >= 0.0);
                    assert!(y >= 0.0);
                }
            }
        });

        writer.join().unwrap();
        reader.join().unwrap();
    }

    #[test]
    fn ancestor_chain() {
        let mut reg = ScrollRegistry::new();
        let sz = Size {
            width: 0.0,
            height: 0.0,
        };
        reg.insert(1, 10, None, sz, 0.0, 0.0);
        reg.insert(2, 11, Some(1), sz, 0.0, 0.0);
        reg.insert(3, 12, Some(2), sz, 0.0, 0.0);

        assert_eq!(reg.ancestor_chain(3), vec![3, 2, 1]);
        assert_eq!(reg.ancestor_chain(2), vec![2, 1]);
        assert_eq!(reg.ancestor_chain(1), vec![1]);
    }

    #[test]
    fn take_dirty_clears_flag() {
        let mut reg = ScrollRegistry::new();
        reg.insert(
            1,
            10,
            None,
            Size {
                width: 100.0,
                height: 100.0,
            },
            0.0,
            0.0,
        );

        assert!(!reg.take_dirty(1));
        reg.update_offset(1, 5.0, 10.0);
        assert!(reg.take_dirty(1));
        assert!(!reg.take_dirty(1));
    }
}
