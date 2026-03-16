use crate::types::*;
use fnv::FnvHashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub struct ScrollNode {
    pub layer_id: LayerId,
    pub parent_scroll: Option<ScrollId>,
    pub content_size: Size,
    pub dirty: AtomicBool,
}

pub struct ScrollRegistry {
    nodes: FnvHashMap<ScrollId, ScrollNode>,
    offsets: FnvHashMap<ScrollId, AtomicU64>,
}

impl Default for ScrollRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollRegistry {
    pub fn new() -> Self {
        Self {
            nodes: FnvHashMap::default(),
            offsets: FnvHashMap::default(),
        }
    }

    pub fn insert(&mut self, id: ScrollId, node: ScrollNode, offset_x: f32, offset_y: f32) {
        self.nodes.insert(id, node);
        let packed = (offset_x.to_bits() as u64) << 32 | offset_y.to_bits() as u64;
        self.offsets.insert(id, AtomicU64::new(packed));
    }

    pub fn update_offset(&self, id: ScrollId, x: f32, y: f32) {
        if let Some(offset) = self.offsets.get(&id) {
            let packed = (x.to_bits() as u64) << 32 | y.to_bits() as u64;
            offset.store(packed, Ordering::Release);
        }
        if let Some(node) = self.nodes.get(&id) {
            node.dirty.store(true, Ordering::Release);
        }
    }

    pub fn get_offset(&self, id: ScrollId) -> (f32, f32) {
        if let Some(offset) = self.offsets.get(&id) {
            let packed = offset.load(Ordering::Acquire);
            let x_bits = (packed >> 32) as u32;
            let y_bits = (packed & 0xFFFFFFFF) as u32;
            (f32::from_bits(x_bits), f32::from_bits(y_bits))
        } else {
            (0.0, 0.0)
        }
    }

    pub fn ancestor_chain(&self, id: ScrollId) -> Vec<ScrollId> {
        let mut chain = Vec::new();
        let mut current = Some(id);

        while let Some(curr_id) = current {
            chain.push(curr_id);
            if let Some(node) = self.nodes.get(&curr_id) {
                current = node.parent_scroll;
            } else {
                break;
            }
        }

        chain
    }

    pub fn take_dirty(&self, id: ScrollId) -> bool {
        if let Some(node) = self.nodes.get(&id) {
            node.dirty.swap(false, Ordering::AcqRel)
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.offsets.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_concurrent_scroll_updates() {
        let mut reg = ScrollRegistry::new();
        reg.insert(
            1,
            ScrollNode {
                layer_id: 10,
                parent_scroll: None,
                content_size: Size {
                    width: 1000.0,
                    height: 1000.0,
                },
                dirty: AtomicBool::new(false),
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
            let mut _detected_updates = 0;
            for _ in 0..1000 {
                if reg_reader.take_dirty(1) {
                    let (x, y) = reg_reader.get_offset(1);
                    assert!(x >= 0.0);
                    assert!(y >= 0.0);
                    _detected_updates += 1;
                }
            }
        });

        writer.join().unwrap();
        reader.join().unwrap();
    }

    #[test]
    fn test_ancestor_chain() {
        let mut reg = ScrollRegistry::new();
        reg.insert(
            1,
            ScrollNode {
                layer_id: 10,
                parent_scroll: None,
                content_size: Size {
                    width: 0.,
                    height: 0.,
                },
                dirty: AtomicBool::new(false),
            },
            0.0,
            0.0,
        );
        reg.insert(
            2,
            ScrollNode {
                layer_id: 11,
                parent_scroll: Some(1),
                content_size: Size {
                    width: 0.,
                    height: 0.,
                },
                dirty: AtomicBool::new(false),
            },
            0.0,
            0.0,
        );
        reg.insert(
            3,
            ScrollNode {
                layer_id: 12,
                parent_scroll: Some(2),
                content_size: Size {
                    width: 0.,
                    height: 0.,
                },
                dirty: AtomicBool::new(false),
            },
            0.0,
            0.0,
        );

        assert_eq!(reg.ancestor_chain(3), vec![3, 2, 1]);
        assert_eq!(reg.ancestor_chain(2), vec![2, 1]);
        assert_eq!(reg.ancestor_chain(1), vec![1]);
    }
}
