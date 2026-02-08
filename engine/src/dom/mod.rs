pub mod document;
pub mod element;
pub mod handle;
pub mod node;
pub mod text;

use crate::runtime::RuntimeState;
use std::cell::RefCell;

pub use document::Document;
pub use element::ElementData;
// Stylo compatibility aliases
pub use element::ElementData as Element;
pub use handle::NodeHandle as ElementHandle;
pub use handle::NodeHandle;
pub use node::{Node, NodeData};
pub use text::TextNodeData;

thread_local! {
    /// Thread-local storage to allow Stylo traits (on Copy handles) to access the RuntimeState.
    pub static CONTEXT: RefCell<Option<&'static RuntimeState>> = const { RefCell::new(None) };
}

/// Helper to execute a closure with the RuntimeState in context.
pub fn with_context<F, R>(state: &RuntimeState, f: F) -> R
where
    F: FnOnce() -> R,
{
    // SAFETY: We are temporarily extending the lifetime of state to static.
    // We strictly unset it after the closure, and we are in a single-threaded environment.
    let static_state =
        unsafe { std::mem::transmute::<&RuntimeState, &'static RuntimeState>(state) };
    CONTEXT.with(|c| *c.borrow_mut() = Some(static_state));
    let result = f();
    CONTEXT.with(|c| *c.borrow_mut() = None);
    result
}
