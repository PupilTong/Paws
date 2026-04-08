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
        // SAFETY: TDocument requires returning a reference with the lifetime of the document.
        // Since `self` is `&'a PawsElement` and `guard` is a field of PawsElement, the
        // reference is valid for 'a. The transmute extends the borrow lifetime to match
        // the trait's required lifetime, which is sound because the PawsElement (and its
        // guard) lives at least as long as the reference.
        unsafe { std::mem::transmute(&self.guard) }
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
