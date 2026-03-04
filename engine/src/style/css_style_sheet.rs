use style::servo_arc::Arc;
use style::stylesheets::Stylesheet;

/// A wrapper around Stylo's [`Stylesheet`] for document-level CSS.
pub struct CSSStyleSheet {
    pub(crate) sheet: Arc<Stylesheet>,
}

impl CSSStyleSheet {
    pub fn new(sheet: Arc<Stylesheet>) -> Self {
        Self { sheet }
    }
}
