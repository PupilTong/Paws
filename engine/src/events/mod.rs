//! W3C DOM Event System — core infrastructure.
//!
//! Implements the EventTarget interface (listener management), the Event
//! object, and the three-phase dispatch algorithm (capture → at-target →
//! bubble) per the [WHATWG DOM Standard](https://dom.spec.whatwg.org/).
//!
//! Hit testing, gesture recognition, native event sources, and UI-specific
//! event types (MouseEvent, KeyboardEvent, etc.) are left for future work.

pub mod dispatch;
pub mod event;
pub mod listener;

pub use event::{Event, EventPhase};
pub use listener::{EventListenerEntry, ListenerOptions};
