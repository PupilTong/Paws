//! `TDocument` and `TShadowRoot` implementations for `&PawsElement`.

use crate::runtime::RenderState;
use style::dom::{TDocument, TNode, TShadowRoot};
use style::shared_lock::SharedRwLock;

use crate::dom::PawsElement;

impl<'a, S: RenderState> TDocument for &'a PawsElement<S> {
    type ConcreteNode = &'a PawsElement<S>;

    fn as_node(&self) -> Self::ConcreteNode {
        self
    }

    fn is_html_document(&self) -> bool {
        true
    }
    fn quirks_mode(&self) -> style::context::QuirksMode {
        style::context::QuirksMode::NoQuirks
    }

    fn shared_lock(&self) -> &SharedRwLock {
        &self.guard
    }
}

impl<'a, S: RenderState> TShadowRoot for &'a PawsElement<S> {
    type ConcreteNode = &'a PawsElement<S>;

    fn as_node(&self) -> Self::ConcreteNode {
        self
    }
    fn host(&self) -> <Self::ConcreteNode as TNode>::ConcreteElement {
        // Technically host requires a separate host tracking, simplified to `self.parent`
        self.parent.map(|id| self.with(id)).unwrap()
    }
    fn style_data<'b>(&self) -> Option<&'b style::stylist::CascadeData>
    where
        Self: 'b,
    {
        self.shadow_cascade_data.as_deref()
    }
}
