use stylo_atoms::Atom;
use taffy::NodeId;

/// W3C DOM event phase constants.
///
/// Maps to `Event.eventPhase` in the DOM specification:
/// `NONE = 0`, `CAPTURING_PHASE = 1`, `AT_TARGET = 2`, `BUBBLING_PHASE = 3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EventPhase {
    None = 0,
    Capturing = 1,
    AtTarget = 2,
    Bubbling = 3,
}

/// Core event object per the WHATWG DOM specification.
///
/// Created for each `dispatchEvent` call and stored in
/// [`RuntimeState::current_event`](crate::RuntimeState) during dispatch so
/// WASM guest code can read and mutate flags via host functions.
#[derive(Debug)]
pub struct Event {
    /// The event type name (e.g. "click", "focus", "custom").
    pub event_type: Atom,

    /// The original target of the event.
    pub target: Option<NodeId>,

    /// The node currently being processed during dispatch.
    pub current_target: Option<NodeId>,

    /// The current dispatch phase.
    pub event_phase: EventPhase,

    /// Whether this event bubbles up through the DOM tree.
    pub bubbles: bool,

    /// Whether this event can be canceled via `preventDefault()`.
    pub cancelable: bool,

    /// Whether this event crosses shadow DOM boundaries.
    pub composed: bool,

    /// Timestamp (milliseconds). Currently set to 0.0; future work will
    /// use a monotonic clock.
    pub time_stamp: f64,

    /// `true` if dispatched by the user agent (not by script).
    pub is_trusted: bool,

    // ── Internal W3C flags ──
    /// Set by `stopPropagation()`. Prevents further propagation to
    /// ancestor/descendant nodes, but remaining listeners on the current
    /// node still fire.
    pub stop_propagation_flag: bool,

    /// Set by `stopImmediatePropagation()`. Halts all remaining listeners,
    /// including those on the current node.
    pub stop_immediate_propagation_flag: bool,

    /// Set by `preventDefault()` (only if `cancelable` is true and the
    /// listener is not passive).
    pub canceled_flag: bool,

    /// `true` while the event is being dispatched.
    pub dispatch_flag: bool,

    /// `true` after the event has been initialized via the constructor.
    pub initialized_flag: bool,

    /// `true` while the currently-invoking listener is passive. Set before
    /// each listener invocation, cleared after.
    pub in_passive_listener: bool,
}

impl Event {
    /// Creates a new event with the given properties.
    ///
    /// All internal flags start as `false`. `is_trusted` defaults to
    /// `false`; only the user agent sets it to `true` for trusted events.
    pub fn new(event_type: Atom, bubbles: bool, cancelable: bool, composed: bool) -> Self {
        Self {
            event_type,
            target: None,
            current_target: None,
            event_phase: EventPhase::None,
            bubbles,
            cancelable,
            composed,
            time_stamp: 0.0,
            is_trusted: false,

            stop_propagation_flag: false,
            stop_immediate_propagation_flag: false,
            canceled_flag: false,
            dispatch_flag: false,
            initialized_flag: true,
            in_passive_listener: false,
        }
    }

    /// Returns `true` if `preventDefault()` was successfully called.
    pub fn default_prevented(&self) -> bool {
        self.canceled_flag
    }
}
