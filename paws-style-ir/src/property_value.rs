//! Top-level property value dispatch enum.
//!
//! [`PropertyValueIR`] captures the semantic value of a CSS declaration at
//! compile time.  For properties that have been typed (sizing, box model,
//! flexbox, etc.) a specific variant carries a validated value.  For
//! unrecognised or not-yet-typed properties, [`PropertyValueIR::Raw`] stores
//! the token list for forward-compatible fallback.

use alloc::vec::Vec;
use rkyv::{Archive, Deserialize, Serialize};

use crate::values::{
    BorderStyleIR, BoxSizingIR, ClearIR, DisplayIR, FlexBasisIR, FlexDirectionIR, FlexWrapIR,
    FloatIR, GapIR, InsetIR, IntegerIR, MarginIR, MaxSizeIR, NonNegativeLPIR, NonNegativeNumberIR,
    ObjectFitIR, OverflowIR, PositionIR, SizeIR, VisibilityIR, ZIndexIR,
};
use crate::{CssToken, CssWideKeyword};

/// The typed value of a CSS property declaration.
///
/// Each variant corresponds to a group of CSS properties that share the same
/// value space.  The `css!()` macro produces these at compile time; the engine
/// consumes them via infallible enum-to-Stylo conversions at runtime.
#[derive(Archive, Deserialize, Serialize, Debug, PartialEq, Clone)]
#[rkyv(
    bytecheck(bounds(__C: rkyv::validation::ArchiveContext, __C::Error: rkyv::rancor::Source)),
    serialize_bounds(__S: rkyv::ser::Writer, __S: rkyv::ser::Allocator, __S::Error: rkyv::rancor::Source),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum PropertyValueIR {
    // ── Sizing ───────────────────────────────────────────────────
    /// `width`, `height`, `min-width`, `min-height`.
    Size(SizeIR),
    /// `max-width`, `max-height`.
    MaxSize(MaxSizeIR),

    // ── Box model ────────────────────────────────────────────────
    /// `margin-top`, `margin-right`, `margin-bottom`, `margin-left`.
    Margin(MarginIR),
    /// `padding-top`, `padding-right`, `padding-bottom`, `padding-left`.
    Padding(NonNegativeLPIR),

    // ── Border ───────────────────────────────────────────────────
    /// `border-top-style`, `border-right-style`, etc.
    BorderStyle(BorderStyleIR),

    // ── Positioning ──────────────────────────────────────────────
    /// `top`, `right`, `bottom`, `left`.
    Inset(InsetIR),
    /// `z-index`.
    ZIndex(ZIndexIR),

    // ── Display & box model keywords ─────────────────────────────
    /// `display`.
    Display(DisplayIR),
    /// `position`.
    Position(PositionIR),
    /// `box-sizing`.
    BoxSizing(BoxSizingIR),
    /// `float`.
    Float(FloatIR),
    /// `clear`.
    Clear(ClearIR),

    // ── Visual ───────────────────────────────────────────────────
    /// `visibility`.
    Visibility(VisibilityIR),
    /// `overflow-x`, `overflow-y`.
    Overflow(OverflowIR),
    /// `object-fit`.
    ObjectFit(ObjectFitIR),

    // ── Flexbox ──────────────────────────────────────────────────
    /// `flex-direction`.
    FlexDirection(FlexDirectionIR),
    /// `flex-wrap`.
    FlexWrap(FlexWrapIR),
    /// `flex-grow`.
    FlexGrow(NonNegativeNumberIR),
    /// `flex-shrink`.
    FlexShrink(NonNegativeNumberIR),
    /// `flex-basis`.
    FlexBasis(FlexBasisIR),
    /// `order`.
    Order(IntegerIR),

    // ── Gap ──────────────────────────────────────────────────────
    /// `column-gap`, `row-gap`.
    Gap(GapIR),

    // ── CSS-wide ─────────────────────────────────────────────────
    /// `inherit`, `initial`, `unset`, `revert`, `revert-layer`.
    CssWide(CssWideKeyword),

    // ── Fallback ─────────────────────────────────────────────────
    /// Unparsed / forward-compatible token list.
    Raw(#[rkyv(omit_bounds)] Vec<CssToken>),
}
