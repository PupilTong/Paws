//! User-Agent stylesheet shipped with the Paws engine.
//!
//! This crate exists purely so the [`UA_STYLESHEET_IR`] byte blob can
//! be produced by the [`view_macros::css!()`] proc-macro without the
//! engine crate itself taking a dependency on `view-macros`. Everything
//! here is resolved at compile time — loading the rkyv-encoded IR at
//! runtime is a single deserialize, no CSS tokenizer involvement.
//!
//! The contents mirror Chrome's UA defaults on the document root:
//!
//! | Property       | Value          | Blink source                   |
//! |----------------|----------------|--------------------------------|
//! | `color`        | `#000000`      | `canvastext` initial (→ black) |
//! | `font-size`    | `16px`         | `medium` initial               |
//! | `font-family`  | `system-ui`    | platform default (canonicalised) |
//! | `font-weight`  | `400`          | `normal` initial               |
//!
//! Paws guests that mount on a bare `<div>` don't see the `html, body`
//! selectors match anything and fall back to Stylo's identical
//! `initial_values`; browser-shaped guests with explicit `<html>` /
//! `<body>` elements pick up the rules through the normal cascade.
//!
//! The engine installs this sheet with [`Origin::UserAgent`] at
//! `RuntimeState` construction via `add_parsed_stylesheet_with_origin`,
//! so every author sheet wins without `!important`.

/// Compile-time-parsed UA stylesheet, ready to feed into the engine's
/// `add_parsed_stylesheet_with_origin` helper.
pub static UA_STYLESHEET_IR: &[u8] = view_macros::css!(
    "html, body { \
        color: #000000; \
        font-size: 16px; \
        font-family: system-ui; \
        font-weight: 400; \
    }"
);
