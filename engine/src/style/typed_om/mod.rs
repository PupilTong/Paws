//! CSS Typed OM implementation.
//!
//! Provides typed access to computed CSS property values following the
//! [CSS Typed OM spec](https://drafts.css-houdini.org/css-typed-om/).
//!
//! The primary entry point is [`StylePropertyMapReadOnly`], a live handle
//! returned by `Document::computed_style_map()`. Read operations lazily
//! trigger style resolution when the DOM tree is dirty.

mod map;
#[cfg(test)]
mod tests;
mod types;

pub use map::StylePropertyMapReadOnly;
pub use types::{CSSKeywordValue, CSSStyleValue, CSSUnitValue};
