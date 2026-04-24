//! Engine-side I/O layer.
//!
//! This module introduces [`EngineIOController`] — a sibling trait to
//! [`EngineRenderer`](crate::EngineRenderer) that lets hosts plug a
//! platform-specific network stack into the engine without the engine
//! itself taking a dependency on any particular async runtime. The
//! pattern mirrors the renderer trait: one associated-type-free trait,
//! monomorphized at the call site, defaulted to a zero-cost `()` no-op
//! for headless usage and tests.
//!
//! ## What lives where
//!
//! - **Trait surface** — [`EngineIOController`]: HTTP request/response
//!   and WebSocket send/recv. All methods return an `impl Future +
//!   Send + '_` so the implementation is free to use tokio, async-std,
//!   a native runloop, or whatever else the host already runs. The
//!   engine does **not** run an executor; callers (the renderer
//!   thread, a future wasmtime host-function hook, a host-owned
//!   scheduler) are responsible for polling the returned futures.
//! - **Engine-owned layer** — [`IoLayer`]: a thin wrapper the engine
//!   stores on [`RuntimeState`](crate::RuntimeState) that combines a
//!   [`ResponseCache`] with the controller. The cache and the data
//!   URL decoder live on the engine side because they are
//!   host-independent: every platform needs them, and every platform
//!   would otherwise duplicate them. The controller is only consulted
//!   on a cache miss for a network URL.
//! - **Utilities** — [`decode_data_url`], [`UrlScheme::classify`]: pure
//!   sync helpers that don't need the controller and can be called
//!   from anywhere.
//!
//! ## Why async in the trait when the engine is sync
//!
//! The engine's `commit()` path is synchronous today, but embedding
//! networking is a future-pointing feature — the natural fit is
//! async. Defining the trait now with async signatures means hosts
//! implementing it do not have to later invert control flow once the
//! engine grows an executor or a poll hook. Paws guests that need
//! resources today still get served synchronously via data URLs and
//! cache hits; network fetches remain unimplemented at the `()` stub
//! and will be wired in when the executor story lands.
//!
//! ## Why the stubs return errors instead of `unimplemented!()`
//!
//! A panicking stub would crash the whole engine thread the first
//! time a guest asks for a network resource. Returning
//! [`IoError::NotImplemented`] keeps the engine resilient: guests see
//! a recoverable error and can fall back to embedded assets, matching
//! the way the `<img>` renderer silently leaves `UIImageView` empty
//! when the decode fails.
//!
//! ## A note on `-> impl Future + Send + '_` vs. `async fn`
//!
//! Every trait method here is declared with an explicit
//! `impl Future<Output = ...> + Send + '_` return type rather than
//! `async fn`. That is intentional and clippy's
//! `manual_async_fn` lint is suppressed for this module: the bare
//! `async fn` sugar in traits does **not** imply a `Send` bound on
//! the returned future, which would force every downstream caller
//! that stores the state across threads to write
//! `trait EngineIOController: Send where <Self::fut: Send>`. Spelling
//! out `Send` in the signature keeps the guarantee local to the
//! trait and matches the engine's `EngineRenderer: Send + 'static`
//! shape.

#![allow(clippy::manual_async_fn)]

use std::future::Future;
use std::sync::Arc;

use fnv::FnvHashMap;

// ── Error / result ────────────────────────────────────────────────────

/// Errors surfaced by the I/O layer. Kept as a typed enum so callers
/// can branch on recoverable vs. unrecoverable failures without
/// parsing strings. Transports that surface structured errors
/// (HTTP status codes, WebSocket close codes) carry those on the
/// success variant instead — `IoError` is reserved for "the request
/// could not complete".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoError {
    /// The controller does not implement this operation. The `()`
    /// stub returns this for every method; real implementations
    /// should only return it for transports they actively refuse
    /// (e.g. HTTP/3 on a platform without QUIC support).
    NotImplemented,

    /// The URL failed to parse or used an unexpected structure.
    InvalidUrl(String),

    /// The URL used a scheme this controller does not handle. The
    /// string is the offending scheme (e.g. `"ftp"`, `"gopher"`).
    UnsupportedScheme(String),

    /// A `data:` URL payload was malformed (bad base64 alphabet,
    /// truncated, missing `;base64` marker for a binary blob).
    DataUrlDecode,

    /// Generic transport-layer failure. The string is a
    /// human-readable description suitable for logging, not for
    /// programmatic dispatch — branch on other variants instead.
    TransportFailure(String),

    /// The operation targeted a WebSocket handle that is already
    /// closed or was never opened.
    ConnectionClosed,

    /// The operation did not complete within the caller-supplied
    /// timeout (via [`HttpRequest::timeout`]).
    TimedOut,
}

impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IoError::NotImplemented => {
                write!(f, "I/O controller does not implement this operation")
            }
            IoError::InvalidUrl(url) => write!(f, "invalid URL: {url}"),
            IoError::UnsupportedScheme(scheme) => write!(f, "unsupported URL scheme: {scheme}"),
            IoError::DataUrlDecode => write!(f, "malformed data: URL payload"),
            IoError::TransportFailure(msg) => write!(f, "transport failure: {msg}"),
            IoError::ConnectionClosed => write!(f, "websocket connection is closed"),
            IoError::TimedOut => write!(f, "I/O operation timed out"),
        }
    }
}

impl std::error::Error for IoError {}

/// Result alias used throughout the I/O layer.
pub type IoResult<T> = Result<T, IoError>;

// ── URL classification ─────────────────────────────────────────────────

/// Coarse classification of a URL scheme. The engine uses this to
/// short-circuit `data:` URLs without calling the controller and to
/// route HTTP-family schemes through the right transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlScheme {
    /// `data:...` — resolvable synchronously via [`decode_data_url`].
    Data,
    /// `http://`
    Http,
    /// `https://`
    Https,
    /// `http3://` — explicit HTTP/3 opt-in. Most hosts expose HTTP/3
    /// via the `https://` scheme with Alt-Svc negotiation instead;
    /// this variant is reserved for hosts that want to force the
    /// transport.
    Http3,
    /// `ws://`
    WebSocket,
    /// `wss://`
    WebSocketSecure,
    /// Any other scheme. Carries the scheme text (without the `:`).
    Other(String),
}

impl UrlScheme {
    /// Classifies the scheme portion of a URL. Returns
    /// [`IoError::InvalidUrl`] if the input has no `:` separator.
    pub fn classify(url: &str) -> IoResult<Self> {
        let (scheme, _rest) = url
            .split_once(':')
            .ok_or_else(|| IoError::InvalidUrl(url.to_string()))?;
        let scheme_lower = scheme.to_ascii_lowercase();
        Ok(match scheme_lower.as_str() {
            "data" => UrlScheme::Data,
            "http" => UrlScheme::Http,
            "https" => UrlScheme::Https,
            "http3" => UrlScheme::Http3,
            "ws" => UrlScheme::WebSocket,
            "wss" => UrlScheme::WebSocketSecure,
            _ => UrlScheme::Other(scheme_lower),
        })
    }

    /// Returns `true` if the scheme is resolvable without a network
    /// round-trip.
    pub fn is_local(&self) -> bool {
        matches!(self, UrlScheme::Data)
    }
}

// ── data: URL decoder ─────────────────────────────────────────────────

/// Decodes a `data:` URL to its raw bytes.
///
/// Accepts `data:<mediatype>?;base64,<payload>`. Whitespace inside
/// the base64 payload is tolerated (hand-authored inline PNGs often
/// wrap across lines). Non-base64 `data:` URLs (e.g.
/// `data:,Hello%20World`) and any other scheme are rejected —
/// callers should route those through [`EngineIOController`] or a
/// percent-encoding utility instead.
pub fn decode_data_url(url: &str) -> IoResult<Vec<u8>> {
    let rest = url
        .strip_prefix("data:")
        .ok_or_else(|| IoError::InvalidUrl(url.to_string()))?;
    let (meta, payload) = rest
        .split_once(',')
        .ok_or_else(|| IoError::InvalidUrl(url.to_string()))?;
    if !meta.split(';').any(|p| p.eq_ignore_ascii_case("base64")) {
        return Err(IoError::DataUrlDecode);
    }
    decode_base64(payload).ok_or(IoError::DataUrlDecode)
}

/// RFC 4648 base64 decode. Skips ASCII whitespace so multi-line
/// payloads decode cleanly; rejects out-of-alphabet bytes and
/// misaligned length.
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer: u32 = 0;
    let mut bits_collected: u32 = 0;
    let mut padding_seen: u32 = 0;
    let mut sextets_seen: u32 = 0;

    for byte in input.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        let sextet = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding_seen += 1;
                if padding_seen > 2 {
                    return None;
                }
                sextets_seen += 1;
                continue;
            }
            _ => return None,
        };
        if padding_seen != 0 {
            return None;
        }
        sextets_seen += 1;
        buffer = (buffer << 6) | sextet as u32;
        bits_collected += 6;
        if bits_collected >= 8 {
            bits_collected -= 8;
            out.push((buffer >> bits_collected) as u8);
            buffer &= (1 << bits_collected) - 1;
        }
    }
    if !sextets_seen.is_multiple_of(4) {
        return None;
    }
    Some(out)
}

// ── HTTP primitives ───────────────────────────────────────────────────

/// HTTP verb. Kept as an explicit enum so unknown verbs can't be
/// silently forwarded to the transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}

impl HttpMethod {
    /// Returns the canonical uppercase method name as used on the
    /// wire.
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Head => "HEAD",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Options => "OPTIONS",
        }
    }
}

/// Fully-specified HTTP request. Header order is preserved as a
/// `Vec` because some servers are sensitive to it; duplicates are
/// allowed (Set-Cookie, for example).
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// Optional per-request timeout. `None` means "use the
    /// controller's default".
    pub timeout: Option<std::time::Duration>,
}

impl HttpRequest {
    /// Convenience constructor for a simple `GET` with no headers or
    /// body.
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Get,
            url: url.into(),
            headers: Vec::new(),
            body: None,
            timeout: None,
        }
    }
}

/// HTTP response. The status code is the protocol-level code, not a
/// redirect-resolved final code — controllers that follow redirects
/// internally should set [`HttpResponse::url`] to the final URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Final URL after any controller-level redirect resolution.
    /// When no redirect happens this equals the request URL.
    pub url: String,
}

// ── WebSocket primitives ──────────────────────────────────────────────

/// Opaque handle identifying a live WebSocket connection owned by
/// the controller. Newtype over `u64` so callers can't mix it up
/// with DOM node ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WebSocketHandle(pub u64);

/// Frame sent over a WebSocket connection. The framing mirrors
/// RFC 6455: Text and Binary are application frames; Ping/Pong are
/// control frames the engine treats as opaque; Close carries the
/// usual 16-bit status code plus optional reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsFrame {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close {
        code: Option<u16>,
        reason: Option<String>,
    },
}

// ── Controller trait ──────────────────────────────────────────────────

/// Host-implemented network controller.
///
/// Each method returns an `impl Future + Send + '_` — the engine
/// holds `&mut self` across the await, so the returned future must
/// not outlive the controller. Implementations are expected to use
/// whatever async runtime the host already has (URLSession on iOS,
/// OkHttp on Android, tokio on a native host, etc.).
///
/// The default `()` impl returns [`IoError::NotImplemented`] for
/// every method. This lets the engine be constructed headlessly
/// (for tests, examples, or platforms that don't want networking
/// yet) without forcing downstream crates to stub a trait.
pub trait EngineIOController: Send + 'static {
    /// Issues a single HTTP, HTTPS, or HTTP/3 request. The
    /// transport is picked from the URL scheme: `http:` → HTTP/1.1
    /// or HTTP/2 per the controller's default, `https:` → same with
    /// TLS, `http3:` → force QUIC.
    fn http_request(
        &mut self,
        request: HttpRequest,
    ) -> impl Future<Output = IoResult<HttpResponse>> + Send + '_;

    /// Opens a WebSocket connection and returns its handle. The
    /// handle identifies the connection until [`websocket_close`] is
    /// called or the peer drops; subsequent
    /// [`websocket_send`]/[`websocket_recv`] calls use it.
    ///
    /// [`websocket_close`]: Self::websocket_close
    /// [`websocket_send`]: Self::websocket_send
    /// [`websocket_recv`]: Self::websocket_recv
    fn websocket_open(
        &mut self,
        url: &str,
    ) -> impl Future<Output = IoResult<WebSocketHandle>> + Send + '_;

    /// Sends a single frame on an open WebSocket. Errors if the
    /// handle is closed or unknown.
    fn websocket_send(
        &mut self,
        handle: WebSocketHandle,
        frame: WsFrame,
    ) -> impl Future<Output = IoResult<()>> + Send + '_;

    /// Reads the next inbound frame. Returns [`WsFrame::Close`] when
    /// the peer initiates a close; returns [`IoError::ConnectionClosed`]
    /// after the connection has already been torn down.
    fn websocket_recv(
        &mut self,
        handle: WebSocketHandle,
    ) -> impl Future<Output = IoResult<WsFrame>> + Send + '_;

    /// Initiates a graceful close on a WebSocket connection. The
    /// `code` and `reason` are forwarded as the RFC 6455 close
    /// payload.
    fn websocket_close(
        &mut self,
        handle: WebSocketHandle,
        code: Option<u16>,
        reason: Option<&str>,
    ) -> impl Future<Output = IoResult<()>> + Send + '_;

    /// Tears down any controller-owned connection pools and
    /// in-flight requests. Called when the engine is shutting down;
    /// a default implementation is provided as a no-op for
    /// controllers that hold no long-lived state.
    fn shutdown(&mut self) -> impl Future<Output = ()> + Send + '_ {
        async {}
    }
}

/// No-op controller for tests and headless usage.
///
/// Every method returns [`IoError::NotImplemented`]. The engine
/// still resolves `data:` URLs and serves cache hits for platforms
/// built on the `()` controller — only network egress is disabled.
impl EngineIOController for () {
    fn http_request(
        &mut self,
        _request: HttpRequest,
    ) -> impl Future<Output = IoResult<HttpResponse>> + Send + '_ {
        async { Err(IoError::NotImplemented) }
    }

    fn websocket_open(
        &mut self,
        _url: &str,
    ) -> impl Future<Output = IoResult<WebSocketHandle>> + Send + '_ {
        async { Err(IoError::NotImplemented) }
    }

    fn websocket_send(
        &mut self,
        _handle: WebSocketHandle,
        _frame: WsFrame,
    ) -> impl Future<Output = IoResult<()>> + Send + '_ {
        async { Err(IoError::NotImplemented) }
    }

    fn websocket_recv(
        &mut self,
        _handle: WebSocketHandle,
    ) -> impl Future<Output = IoResult<WsFrame>> + Send + '_ {
        async { Err(IoError::NotImplemented) }
    }

    fn websocket_close(
        &mut self,
        _handle: WebSocketHandle,
        _code: Option<u16>,
        _reason: Option<&str>,
    ) -> impl Future<Output = IoResult<()>> + Send + '_ {
        async { Err(IoError::NotImplemented) }
    }
}

// ── Response cache ────────────────────────────────────────────────────

/// Engine-owned response body cache, keyed by full URL.
///
/// Stores `Arc<Vec<u8>>` so callers (the renderer, the style loader,
/// future `<img>` src resolution) can share a single copy of the
/// decoded bytes. The cache is unbounded today — future work will
/// add an LRU eviction policy, TTL via `Cache-Control`, and
/// conditional revalidation via `ETag`/`Last-Modified`. Keeping the
/// surface minimal until then prevents callers from building on
/// semantics we haven't committed to.
pub struct ResponseCache {
    entries: FnvHashMap<String, Arc<Vec<u8>>>,
}

impl ResponseCache {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self {
            entries: FnvHashMap::default(),
        }
    }

    /// Returns the cached bytes for `url`, or `None` on a miss.
    pub fn get(&self, url: &str) -> Option<Arc<Vec<u8>>> {
        self.entries.get(url).cloned()
    }

    /// Inserts bytes for `url`. Overwrites any prior entry.
    pub fn insert(&mut self, url: String, bytes: Arc<Vec<u8>>) {
        self.entries.insert(url, bytes);
    }

    /// Removes the entry for `url`, returning the old bytes if any.
    pub fn remove(&mut self, url: &str) -> Option<Arc<Vec<u8>>> {
        self.entries.remove(url)
    }

    /// Drops every entry.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Number of entries currently cached.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for ResponseCache {
    fn default() -> Self {
        Self::new()
    }
}

// ── I/O layer ─────────────────────────────────────────────────────────

/// Engine-owned façade over the controller.
///
/// `IoLayer` bundles the [`ResponseCache`] and the
/// [`EngineIOController`] together so the rest of the engine has a
/// single entry point for resource resolution. The layer also hosts
/// the synchronous fast paths (data URL decoding, cache hits) that
/// should never require driving a future — every cache lookup that
/// can answer immediately does, and only true network misses fall
/// through to the async controller.
///
/// Callers are responsible for polling futures returned from
/// [`Self::controller_mut`] because the engine does not own an
/// executor.
pub struct IoLayer<I: EngineIOController = ()> {
    controller: I,
    cache: ResponseCache,
}

impl<I: EngineIOController> IoLayer<I> {
    /// Wraps a controller in a fresh layer with an empty cache.
    pub fn new(controller: I) -> Self {
        Self {
            controller,
            cache: ResponseCache::new(),
        }
    }

    /// Immutable access to the cache.
    pub fn cache(&self) -> &ResponseCache {
        &self.cache
    }

    /// Mutable access to the cache. Exposed so callers that own the
    /// async path can insert the result of a completed network
    /// fetch.
    pub fn cache_mut(&mut self) -> &mut ResponseCache {
        &mut self.cache
    }

    /// Immutable access to the controller.
    pub fn controller(&self) -> &I {
        &self.controller
    }

    /// Mutable access to the controller. Callers obtain the
    /// controller here to call its async methods and `.await` the
    /// returned futures against their own executor.
    pub fn controller_mut(&mut self) -> &mut I {
        &mut self.controller
    }

    /// Synchronous fetch that succeeds only when the URL can be
    /// resolved without touching the controller: a `data:` URL or
    /// a cache hit. Returns `Ok(None)` for a network miss so the
    /// caller can decide whether to kick off an async fetch.
    ///
    /// `data:` URLs decoded here are inserted into the cache —
    /// repeated lookups for the same URL are `O(1)` after the first.
    pub fn fetch_sync(&mut self, url: &str) -> IoResult<Option<Arc<Vec<u8>>>> {
        if let Some(hit) = self.cache.get(url) {
            return Ok(Some(hit));
        }
        match UrlScheme::classify(url)? {
            UrlScheme::Data => {
                let bytes = Arc::new(decode_data_url(url)?);
                self.cache.insert(url.to_string(), bytes.clone());
                Ok(Some(bytes))
            }
            _ => Ok(None),
        }
    }

    /// Consumes the layer and returns its parts. Used when the
    /// engine is being torn down and the controller needs an
    /// explicit drop on the host side.
    pub fn into_parts(self) -> (I, ResponseCache) {
        (self.controller, self.cache)
    }
}

impl<I: EngineIOController + Default> Default for IoLayer<I> {
    fn default() -> Self {
        Self::new(I::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Data URL decode ────────────────────────────────────────────

    #[test]
    fn decode_data_url_accepts_base64_payload() {
        // "Paws" → UGF3cw==
        let bytes = decode_data_url("data:text/plain;base64,UGF3cw==").unwrap();
        assert_eq!(bytes, b"Paws");
    }

    #[test]
    fn decode_data_url_tolerates_whitespace_in_payload() {
        let bytes = decode_data_url("data:image/png;base64,UGF3\ncw==").unwrap();
        assert_eq!(bytes, b"Paws");
    }

    #[test]
    fn decode_data_url_rejects_non_data_scheme() {
        assert!(matches!(
            decode_data_url("https://example.com/a.png"),
            Err(IoError::InvalidUrl(_))
        ));
    }

    #[test]
    fn decode_data_url_rejects_missing_base64_marker() {
        assert_eq!(
            decode_data_url("data:text/plain,hello"),
            Err(IoError::DataUrlDecode)
        );
    }

    #[test]
    fn decode_data_url_rejects_out_of_alphabet_payload() {
        assert_eq!(
            decode_data_url("data:image/png;base64,!!!!"),
            Err(IoError::DataUrlDecode)
        );
    }

    // ── URL classification ─────────────────────────────────────────

    #[test]
    fn classify_url_covers_well_known_schemes() {
        assert_eq!(UrlScheme::classify("data:,x"), Ok(UrlScheme::Data));
        assert_eq!(UrlScheme::classify("https://a/b"), Ok(UrlScheme::Https));
        assert_eq!(UrlScheme::classify("http://a"), Ok(UrlScheme::Http));
        assert_eq!(UrlScheme::classify("http3://a"), Ok(UrlScheme::Http3));
        assert_eq!(UrlScheme::classify("ws://a"), Ok(UrlScheme::WebSocket));
        assert_eq!(
            UrlScheme::classify("wss://a"),
            Ok(UrlScheme::WebSocketSecure)
        );
        assert_eq!(
            UrlScheme::classify("ftp://a"),
            Ok(UrlScheme::Other("ftp".to_string()))
        );
    }

    #[test]
    fn classify_url_rejects_inputs_without_colon() {
        assert!(matches!(
            UrlScheme::classify("no-scheme-here"),
            Err(IoError::InvalidUrl(_))
        ));
    }

    #[test]
    fn is_local_true_only_for_data() {
        assert!(UrlScheme::Data.is_local());
        assert!(!UrlScheme::Https.is_local());
        assert!(!UrlScheme::WebSocket.is_local());
    }

    // ── () stub controller ─────────────────────────────────────────

    /// Polls a future to completion on a trivial
    /// `block_on`-style loop. The engine itself has no executor;
    /// these tests only need to drive trivially-ready futures
    /// produced by the `()` stub, so a busy-polled waker is fine.
    fn poll_now<F: Future>(mut fut: F) -> F::Output {
        use std::pin::Pin;
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        fn raw_waker() -> RawWaker {
            fn noop(_: *const ()) {}
            fn clone(_: *const ()) -> RawWaker {
                raw_waker()
            }
            static VT: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
            RawWaker::new(std::ptr::null(), &VT)
        }

        // SAFETY: the vtable only uses no-op functions, so the waker
        // never dereferences the null data pointer.
        let waker = unsafe { Waker::from_raw(raw_waker()) };
        let mut ctx = Context::from_waker(&waker);

        // SAFETY: `fut` is a local owned value — pinning it to the
        // stack for the duration of the loop is safe.
        let mut fut = unsafe { Pin::new_unchecked(&mut fut) };
        loop {
            if let Poll::Ready(val) = fut.as_mut().poll(&mut ctx) {
                return val;
            }
        }
    }

    #[test]
    fn unit_controller_rejects_http_request() {
        let mut controller: () = ();
        let result = poll_now(controller.http_request(HttpRequest::get("https://example.com")));
        assert_eq!(result, Err(IoError::NotImplemented));
    }

    #[test]
    fn unit_controller_rejects_websocket_open() {
        let mut controller: () = ();
        let result = poll_now(controller.websocket_open("wss://example.com/socket"));
        assert_eq!(result, Err(IoError::NotImplemented));
    }

    #[test]
    fn unit_controller_rejects_websocket_send_recv_close() {
        let mut controller: () = ();
        let handle = WebSocketHandle(42);
        assert_eq!(
            poll_now(controller.websocket_send(handle, WsFrame::Text("hi".to_string()))),
            Err(IoError::NotImplemented)
        );
        assert_eq!(
            poll_now(controller.websocket_recv(handle)),
            Err(IoError::NotImplemented)
        );
        assert_eq!(
            poll_now(controller.websocket_close(handle, Some(1000), Some("bye"))),
            Err(IoError::NotImplemented)
        );
    }

    #[test]
    fn unit_controller_shutdown_is_noop() {
        let mut controller: () = ();
        poll_now(controller.shutdown());
    }

    // ── ResponseCache ──────────────────────────────────────────────

    #[test]
    fn response_cache_round_trip() {
        let mut cache = ResponseCache::new();
        assert!(cache.is_empty());
        cache.insert("https://a.test/1".to_string(), Arc::new(b"one".to_vec()));
        cache.insert("https://a.test/2".to_string(), Arc::new(b"two".to_vec()));
        assert_eq!(cache.len(), 2);
        assert_eq!(
            cache.get("https://a.test/1").as_deref(),
            Some(&b"one".to_vec())
        );
        assert_eq!(
            cache.remove("https://a.test/2").as_deref(),
            Some(&b"two".to_vec())
        );
        assert_eq!(cache.len(), 1);
        cache.clear();
        assert!(cache.is_empty());
    }

    // ── IoLayer ────────────────────────────────────────────────────

    #[test]
    fn io_layer_fetch_sync_resolves_data_urls_and_caches() {
        let mut layer: IoLayer<()> = IoLayer::new(());
        let url = "data:text/plain;base64,UGF3cw==";
        let first = layer.fetch_sync(url).unwrap();
        assert_eq!(
            first.as_deref().map(|v| v.as_slice()),
            Some(b"Paws".as_slice())
        );
        // Second call is served from the cache — clobber the
        // decoder by asserting identity via Arc pointer equality
        // would be nice, but pointer-equality on freshly-decoded
        // bytes is enough signal that we didn't re-decode if the
        // cache length didn't grow.
        assert_eq!(layer.cache().len(), 1);
        let second = layer.fetch_sync(url).unwrap();
        assert_eq!(
            second.as_deref().map(|v| v.as_slice()),
            Some(b"Paws".as_slice())
        );
        assert_eq!(layer.cache().len(), 1);
    }

    #[test]
    fn io_layer_fetch_sync_returns_none_for_network_urls() {
        let mut layer: IoLayer<()> = IoLayer::new(());
        let result = layer.fetch_sync("https://example.com/image.png").unwrap();
        assert!(
            result.is_none(),
            "network URLs are never resolvable via fetch_sync"
        );
        assert!(layer.cache().is_empty());
    }

    #[test]
    fn io_layer_fetch_sync_surfaces_manual_cache_inserts() {
        let mut layer: IoLayer<()> = IoLayer::new(());
        let url = "https://example.com/image.png";
        layer
            .cache_mut()
            .insert(url.to_string(), Arc::new(b"cached".to_vec()));
        let result = layer.fetch_sync(url).unwrap();
        assert_eq!(
            result.as_deref().map(|v| v.as_slice()),
            Some(b"cached".as_slice()),
            "a cached network URL should be served synchronously"
        );
    }

    #[test]
    fn io_layer_fetch_sync_propagates_data_url_decode_errors() {
        let mut layer: IoLayer<()> = IoLayer::new(());
        assert_eq!(
            layer.fetch_sync("data:image/png;base64,!!!!"),
            Err(IoError::DataUrlDecode)
        );
    }

    /// Compile-time check that a custom controller fits the trait
    /// surface. The body only needs to exist — it never runs.
    #[allow(dead_code)]
    fn _controller_is_object_safe_for_generics<I: EngineIOController>(_io: IoLayer<I>) {}

    #[test]
    fn custom_controller_plugs_into_io_layer() {
        struct CountingController {
            http_calls: u32,
        }
        impl EngineIOController for CountingController {
            fn http_request(
                &mut self,
                _request: HttpRequest,
            ) -> impl Future<Output = IoResult<HttpResponse>> + Send + '_ {
                self.http_calls += 1;
                async {
                    Ok(HttpResponse {
                        status: 200,
                        headers: Vec::new(),
                        body: b"ok".to_vec(),
                        url: String::new(),
                    })
                }
            }
            fn websocket_open(
                &mut self,
                _url: &str,
            ) -> impl Future<Output = IoResult<WebSocketHandle>> + Send + '_ {
                async { Err(IoError::NotImplemented) }
            }
            fn websocket_send(
                &mut self,
                _handle: WebSocketHandle,
                _frame: WsFrame,
            ) -> impl Future<Output = IoResult<()>> + Send + '_ {
                async { Err(IoError::NotImplemented) }
            }
            fn websocket_recv(
                &mut self,
                _handle: WebSocketHandle,
            ) -> impl Future<Output = IoResult<WsFrame>> + Send + '_ {
                async { Err(IoError::NotImplemented) }
            }
            fn websocket_close(
                &mut self,
                _handle: WebSocketHandle,
                _code: Option<u16>,
                _reason: Option<&str>,
            ) -> impl Future<Output = IoResult<()>> + Send + '_ {
                async { Err(IoError::NotImplemented) }
            }
        }

        let mut layer = IoLayer::new(CountingController { http_calls: 0 });
        let fut = layer
            .controller_mut()
            .http_request(HttpRequest::get("https://a.test/"));
        let response = poll_now(fut).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(layer.controller().http_calls, 1);
    }
}
