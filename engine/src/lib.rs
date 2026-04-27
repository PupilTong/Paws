//! Engine crate: core DOM, style, and layout.

pub mod dom;
pub mod events;
pub mod hit_test;
pub mod io;
pub mod layout;
pub mod resources;
mod runtime;
mod style;

pub use hit_test::hit_test_at_point;
pub use io::{
    decode_data_url, EngineIOController, HttpMethod, HttpRequest, HttpResponse, IoError, IoLayer,
    IoResult, UrlScheme, WebSocketHandle, WsFrame,
};
pub use layout::{compute_layout_in_place, paint_order_children};
pub use resources::{
    BlobEntry, BlobRegistry, CachePolicy, CachePolicyProvider, CachedEntry, EvictionPolicy,
    Freshness, LruBytesEviction, ResourceManager,
};
pub use runtime::{
    EngineRenderer, HostErrorCode, NoopResourceResolver, RenderState, ResourceResolver,
    RuntimeState,
};
pub use style::typed_om::{CSSKeywordValue, CSSStyleValue, CSSUnitValue, StylePropertyMapReadOnly};
pub use style::StyleProfilingSnapshot;
pub use taffy::NodeId;
