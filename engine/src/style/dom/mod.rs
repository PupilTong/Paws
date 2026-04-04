//! Stylo trait implementations for `&PawsElement`.
//!
//! This module bridges Paws's DOM representation with Stylo's trait-based
//! element model. Each sub-module implements one or more Stylo/selectors traits.

mod document;
mod element;
mod node;
mod selector;

use crate::dom::PawsElement;

/// Iterator over the children of a `PawsElement`, yielding `&PawsElement` references.
pub struct ChildrenIterator<'a, S: Default + Send + 'static = ()> {
    node: &'a PawsElement<S>,
    index: usize,
}

impl<'a, S: Default + Send + 'static> Iterator for ChildrenIterator<'a, S> {
    type Item = &'a PawsElement<S>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.node.children.len() {
            let child_id = self.node.children[self.index];
            self.index += 1;
            Some(self.node.with(child_id))
        } else {
            None
        }
    }
}
