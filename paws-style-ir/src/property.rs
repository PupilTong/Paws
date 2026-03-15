use alloc::string::String;
use alloc::vec::Vec;
use rkyv::{Archive, Deserialize, Serialize};

/// CSS unit types for numeric values.
///
/// Uses `#[repr(u8)]` for compact rkyv serialization (1 byte vs heap-allocated String).
/// Unrecognized units at parse time cause the value to fall back to `CssComponentValue::Unparsed`.
#[derive(Archive, Deserialize, Serialize, Debug, PartialEq, Clone, Copy)]
#[rkyv(
    bytecheck(bounds(__C: rkyv::validation::ArchiveContext, __C::Error: rkyv::rancor::Source)),
    serialize_bounds(__S: rkyv::ser::Writer, __S: rkyv::ser::Allocator, __S::Error: rkyv::rancor::Source),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
#[repr(u8)]
pub enum CssUnit {
    // Absolute lengths
    Px,
    Cm,
    Mm,
    In,
    Pt,
    Pc,
    Q,
    // Relative lengths
    Em,
    Rem,
    Ex,
    Ch,
    // Viewport-relative
    Vh,
    Vw,
    Vmin,
    Vmax,
    Svh,
    Svw,
    Lvh,
    Lvw,
    Dvh,
    Dvw,
    // Container query units
    Cqw,
    Cqh,
    Cqi,
    Cqb,
    Cqmin,
    Cqmax,
    // Percentage
    Percent,
    // Grid
    Fr,
    // Angles
    Deg,
    Rad,
    Grad,
    Turn,
    // Time
    S,
    Ms,
    // Resolution
    Dpi,
    Dpcm,
    Dppx,
    // Unitless (bare number like `0` or line-height `1.5`)
    Unitless,
}

impl CssUnit {
    /// Converts a CSS unit string to a typed `CssUnit`.
    ///
    /// Returns `None` for unrecognized units, which should cause the
    /// declaration to fall back to `CssComponentValue::Unparsed`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "px" => Some(CssUnit::Px),
            "cm" => Some(CssUnit::Cm),
            "mm" => Some(CssUnit::Mm),
            "in" => Some(CssUnit::In),
            "pt" => Some(CssUnit::Pt),
            "pc" => Some(CssUnit::Pc),
            "q" | "Q" => Some(CssUnit::Q),
            "em" => Some(CssUnit::Em),
            "rem" => Some(CssUnit::Rem),
            "ex" => Some(CssUnit::Ex),
            "ch" => Some(CssUnit::Ch),
            "vh" => Some(CssUnit::Vh),
            "vw" => Some(CssUnit::Vw),
            "vmin" => Some(CssUnit::Vmin),
            "vmax" => Some(CssUnit::Vmax),
            "svh" => Some(CssUnit::Svh),
            "svw" => Some(CssUnit::Svw),
            "lvh" => Some(CssUnit::Lvh),
            "lvw" => Some(CssUnit::Lvw),
            "dvh" => Some(CssUnit::Dvh),
            "dvw" => Some(CssUnit::Dvw),
            "cqw" => Some(CssUnit::Cqw),
            "cqh" => Some(CssUnit::Cqh),
            "cqi" => Some(CssUnit::Cqi),
            "cqb" => Some(CssUnit::Cqb),
            "cqmin" => Some(CssUnit::Cqmin),
            "cqmax" => Some(CssUnit::Cqmax),
            "%" => Some(CssUnit::Percent),
            "fr" => Some(CssUnit::Fr),
            "deg" => Some(CssUnit::Deg),
            "rad" => Some(CssUnit::Rad),
            "grad" => Some(CssUnit::Grad),
            "turn" => Some(CssUnit::Turn),
            "s" => Some(CssUnit::S),
            "ms" => Some(CssUnit::Ms),
            "dpi" => Some(CssUnit::Dpi),
            "dpcm" => Some(CssUnit::Dpcm),
            "dppx" | "x" => Some(CssUnit::Dppx),
            "" => Some(CssUnit::Unitless),
            _ => None,
        }
    }

    /// Returns the canonical CSS string for this unit.
    pub fn as_str(&self) -> &'static str {
        match self {
            CssUnit::Px => "px",
            CssUnit::Cm => "cm",
            CssUnit::Mm => "mm",
            CssUnit::In => "in",
            CssUnit::Pt => "pt",
            CssUnit::Pc => "pc",
            CssUnit::Q => "q",
            CssUnit::Em => "em",
            CssUnit::Rem => "rem",
            CssUnit::Ex => "ex",
            CssUnit::Ch => "ch",
            CssUnit::Vh => "vh",
            CssUnit::Vw => "vw",
            CssUnit::Vmin => "vmin",
            CssUnit::Vmax => "vmax",
            CssUnit::Svh => "svh",
            CssUnit::Svw => "svw",
            CssUnit::Lvh => "lvh",
            CssUnit::Lvw => "lvw",
            CssUnit::Dvh => "dvh",
            CssUnit::Dvw => "dvw",
            CssUnit::Cqw => "cqw",
            CssUnit::Cqh => "cqh",
            CssUnit::Cqi => "cqi",
            CssUnit::Cqb => "cqb",
            CssUnit::Cqmin => "cqmin",
            CssUnit::Cqmax => "cqmax",
            CssUnit::Percent => "%",
            CssUnit::Fr => "fr",
            CssUnit::Deg => "deg",
            CssUnit::Rad => "rad",
            CssUnit::Grad => "grad",
            CssUnit::Turn => "turn",
            CssUnit::S => "s",
            CssUnit::Ms => "ms",
            CssUnit::Dpi => "dpi",
            CssUnit::Dpcm => "dpcm",
            CssUnit::Dppx => "dppx",
            CssUnit::Unitless => "",
        }
    }
}

/// CSS-wide keywords that apply to all properties.
#[derive(Archive, Deserialize, Serialize, Debug, PartialEq, Clone, Copy)]
#[rkyv(
    bytecheck(bounds(__C: rkyv::validation::ArchiveContext, __C::Error: rkyv::rancor::Source)),
    serialize_bounds(__S: rkyv::ser::Writer, __S: rkyv::ser::Allocator, __S::Error: rkyv::rancor::Source),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
#[repr(u8)]
pub enum CssWideKeyword {
    Inherit,
    Initial,
    Unset,
    Revert,
    RevertLayer,
}

impl CssWideKeyword {
    /// Tries to parse a CSS-wide keyword from a string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "inherit" => Some(CssWideKeyword::Inherit),
            "initial" => Some(CssWideKeyword::Initial),
            "unset" => Some(CssWideKeyword::Unset),
            "revert" => Some(CssWideKeyword::Revert),
            "revert-layer" => Some(CssWideKeyword::RevertLayer),
            _ => None,
        }
    }

    /// Returns the CSS string for this keyword.
    pub fn as_str(&self) -> &'static str {
        match self {
            CssWideKeyword::Inherit => "inherit",
            CssWideKeyword::Initial => "initial",
            CssWideKeyword::Unset => "unset",
            CssWideKeyword::Revert => "revert",
            CssWideKeyword::RevertLayer => "revert-layer",
        }
    }
}

/// Typed CSS property name.
///
/// Enumerates commonly-used CSS longhand properties for efficient matching.
/// Custom properties (`--*`) use `Custom(String)`.
/// Standard properties not yet in this enum use `Other(String)`.
#[derive(Archive, Deserialize, Serialize, Debug, PartialEq, Clone)]
#[rkyv(
    bytecheck(bounds(__C: rkyv::validation::ArchiveContext, __C::Error: rkyv::rancor::Source)),
    serialize_bounds(__S: rkyv::ser::Writer, __S: rkyv::ser::Allocator, __S::Error: rkyv::rancor::Source),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum CssPropertyName {
    // Display & box model
    Display,
    BoxSizing,
    // Sizing
    Width,
    Height,
    MinWidth,
    MinHeight,
    MaxWidth,
    MaxHeight,
    // Margin
    MarginTop,
    MarginRight,
    MarginBottom,
    MarginLeft,
    // Padding
    PaddingTop,
    PaddingRight,
    PaddingBottom,
    PaddingLeft,
    // Border width
    BorderTopWidth,
    BorderRightWidth,
    BorderBottomWidth,
    BorderLeftWidth,
    // Border style
    BorderTopStyle,
    BorderRightStyle,
    BorderBottomStyle,
    BorderLeftStyle,
    // Border color
    BorderTopColor,
    BorderRightColor,
    BorderBottomColor,
    BorderLeftColor,
    // Border radius
    BorderTopLeftRadius,
    BorderTopRightRadius,
    BorderBottomLeftRadius,
    BorderBottomRightRadius,
    // Positioning
    Position,
    Top,
    Right,
    Bottom,
    Left,
    ZIndex,
    Float,
    Clear,
    // Flexbox
    FlexDirection,
    FlexWrap,
    FlexGrow,
    FlexShrink,
    FlexBasis,
    AlignItems,
    AlignSelf,
    AlignContent,
    JustifyContent,
    JustifyItems,
    JustifySelf,
    Order,
    // Grid
    GridTemplateColumns,
    GridTemplateRows,
    GridAutoFlow,
    GridAutoColumns,
    GridAutoRows,
    GridColumnStart,
    GridColumnEnd,
    GridRowStart,
    GridRowEnd,
    ColumnGap,
    RowGap,
    // Visual
    Color,
    BackgroundColor,
    Opacity,
    Overflow,
    OverflowX,
    OverflowY,
    Visibility,
    // Typography
    FontSize,
    FontWeight,
    FontFamily,
    FontStyle,
    LineHeight,
    TextAlign,
    TextDecoration,
    TextTransform,
    LetterSpacing,
    WordSpacing,
    WhiteSpace,
    VerticalAlign,
    // Aspect ratio
    AspectRatio,
    // Object fit
    ObjectFit,
    ObjectPosition,
    // Custom properties (--name)
    Custom(String),
    // Standard property not yet enumerated
    Other(String),
}

impl CssPropertyName {
    /// Parses a CSS property name string into the typed enum.
    pub fn parse(name: &str) -> Self {
        if let Some(stripped) = name.strip_prefix("--") {
            if stripped.is_empty() {
                return CssPropertyName::Other(String::from(name));
            }
            return CssPropertyName::Custom(String::from(name));
        }
        match name {
            "display" => CssPropertyName::Display,
            "box-sizing" => CssPropertyName::BoxSizing,
            "width" => CssPropertyName::Width,
            "height" => CssPropertyName::Height,
            "min-width" => CssPropertyName::MinWidth,
            "min-height" => CssPropertyName::MinHeight,
            "max-width" => CssPropertyName::MaxWidth,
            "max-height" => CssPropertyName::MaxHeight,
            "margin-top" => CssPropertyName::MarginTop,
            "margin-right" => CssPropertyName::MarginRight,
            "margin-bottom" => CssPropertyName::MarginBottom,
            "margin-left" => CssPropertyName::MarginLeft,
            "padding-top" => CssPropertyName::PaddingTop,
            "padding-right" => CssPropertyName::PaddingRight,
            "padding-bottom" => CssPropertyName::PaddingBottom,
            "padding-left" => CssPropertyName::PaddingLeft,
            "border-top-width" => CssPropertyName::BorderTopWidth,
            "border-right-width" => CssPropertyName::BorderRightWidth,
            "border-bottom-width" => CssPropertyName::BorderBottomWidth,
            "border-left-width" => CssPropertyName::BorderLeftWidth,
            "border-top-style" => CssPropertyName::BorderTopStyle,
            "border-right-style" => CssPropertyName::BorderRightStyle,
            "border-bottom-style" => CssPropertyName::BorderBottomStyle,
            "border-left-style" => CssPropertyName::BorderLeftStyle,
            "border-top-color" => CssPropertyName::BorderTopColor,
            "border-right-color" => CssPropertyName::BorderRightColor,
            "border-bottom-color" => CssPropertyName::BorderBottomColor,
            "border-left-color" => CssPropertyName::BorderLeftColor,
            "border-top-left-radius" => CssPropertyName::BorderTopLeftRadius,
            "border-top-right-radius" => CssPropertyName::BorderTopRightRadius,
            "border-bottom-left-radius" => CssPropertyName::BorderBottomLeftRadius,
            "border-bottom-right-radius" => CssPropertyName::BorderBottomRightRadius,
            "position" => CssPropertyName::Position,
            "top" => CssPropertyName::Top,
            "right" => CssPropertyName::Right,
            "bottom" => CssPropertyName::Bottom,
            "left" => CssPropertyName::Left,
            "z-index" => CssPropertyName::ZIndex,
            "float" => CssPropertyName::Float,
            "clear" => CssPropertyName::Clear,
            "flex-direction" => CssPropertyName::FlexDirection,
            "flex-wrap" => CssPropertyName::FlexWrap,
            "flex-grow" => CssPropertyName::FlexGrow,
            "flex-shrink" => CssPropertyName::FlexShrink,
            "flex-basis" => CssPropertyName::FlexBasis,
            "align-items" => CssPropertyName::AlignItems,
            "align-self" => CssPropertyName::AlignSelf,
            "align-content" => CssPropertyName::AlignContent,
            "justify-content" => CssPropertyName::JustifyContent,
            "justify-items" => CssPropertyName::JustifyItems,
            "justify-self" => CssPropertyName::JustifySelf,
            "order" => CssPropertyName::Order,
            "grid-template-columns" => CssPropertyName::GridTemplateColumns,
            "grid-template-rows" => CssPropertyName::GridTemplateRows,
            "grid-auto-flow" => CssPropertyName::GridAutoFlow,
            "grid-auto-columns" => CssPropertyName::GridAutoColumns,
            "grid-auto-rows" => CssPropertyName::GridAutoRows,
            "grid-column-start" => CssPropertyName::GridColumnStart,
            "grid-column-end" => CssPropertyName::GridColumnEnd,
            "grid-row-start" => CssPropertyName::GridRowStart,
            "grid-row-end" => CssPropertyName::GridRowEnd,
            "column-gap" => CssPropertyName::ColumnGap,
            "row-gap" => CssPropertyName::RowGap,
            "color" => CssPropertyName::Color,
            "background-color" => CssPropertyName::BackgroundColor,
            "opacity" => CssPropertyName::Opacity,
            "overflow" => CssPropertyName::Overflow,
            "overflow-x" => CssPropertyName::OverflowX,
            "overflow-y" => CssPropertyName::OverflowY,
            "visibility" => CssPropertyName::Visibility,
            "font-size" => CssPropertyName::FontSize,
            "font-weight" => CssPropertyName::FontWeight,
            "font-family" => CssPropertyName::FontFamily,
            "font-style" => CssPropertyName::FontStyle,
            "line-height" => CssPropertyName::LineHeight,
            "text-align" => CssPropertyName::TextAlign,
            "text-decoration" => CssPropertyName::TextDecoration,
            "text-transform" => CssPropertyName::TextTransform,
            "letter-spacing" => CssPropertyName::LetterSpacing,
            "word-spacing" => CssPropertyName::WordSpacing,
            "white-space" => CssPropertyName::WhiteSpace,
            "vertical-align" => CssPropertyName::VerticalAlign,
            "aspect-ratio" => CssPropertyName::AspectRatio,
            "object-fit" => CssPropertyName::ObjectFit,
            "object-position" => CssPropertyName::ObjectPosition,
            _ => CssPropertyName::Other(String::from(name)),
        }
    }

    /// Returns the CSS property name string.
    pub fn as_str(&self) -> &str {
        match self {
            CssPropertyName::Display => "display",
            CssPropertyName::BoxSizing => "box-sizing",
            CssPropertyName::Width => "width",
            CssPropertyName::Height => "height",
            CssPropertyName::MinWidth => "min-width",
            CssPropertyName::MinHeight => "min-height",
            CssPropertyName::MaxWidth => "max-width",
            CssPropertyName::MaxHeight => "max-height",
            CssPropertyName::MarginTop => "margin-top",
            CssPropertyName::MarginRight => "margin-right",
            CssPropertyName::MarginBottom => "margin-bottom",
            CssPropertyName::MarginLeft => "margin-left",
            CssPropertyName::PaddingTop => "padding-top",
            CssPropertyName::PaddingRight => "padding-right",
            CssPropertyName::PaddingBottom => "padding-bottom",
            CssPropertyName::PaddingLeft => "padding-left",
            CssPropertyName::BorderTopWidth => "border-top-width",
            CssPropertyName::BorderRightWidth => "border-right-width",
            CssPropertyName::BorderBottomWidth => "border-bottom-width",
            CssPropertyName::BorderLeftWidth => "border-left-width",
            CssPropertyName::BorderTopStyle => "border-top-style",
            CssPropertyName::BorderRightStyle => "border-right-style",
            CssPropertyName::BorderBottomStyle => "border-bottom-style",
            CssPropertyName::BorderLeftStyle => "border-left-style",
            CssPropertyName::BorderTopColor => "border-top-color",
            CssPropertyName::BorderRightColor => "border-right-color",
            CssPropertyName::BorderBottomColor => "border-bottom-color",
            CssPropertyName::BorderLeftColor => "border-left-color",
            CssPropertyName::BorderTopLeftRadius => "border-top-left-radius",
            CssPropertyName::BorderTopRightRadius => "border-top-right-radius",
            CssPropertyName::BorderBottomLeftRadius => "border-bottom-left-radius",
            CssPropertyName::BorderBottomRightRadius => "border-bottom-right-radius",
            CssPropertyName::Position => "position",
            CssPropertyName::Top => "top",
            CssPropertyName::Right => "right",
            CssPropertyName::Bottom => "bottom",
            CssPropertyName::Left => "left",
            CssPropertyName::ZIndex => "z-index",
            CssPropertyName::Float => "float",
            CssPropertyName::Clear => "clear",
            CssPropertyName::FlexDirection => "flex-direction",
            CssPropertyName::FlexWrap => "flex-wrap",
            CssPropertyName::FlexGrow => "flex-grow",
            CssPropertyName::FlexShrink => "flex-shrink",
            CssPropertyName::FlexBasis => "flex-basis",
            CssPropertyName::AlignItems => "align-items",
            CssPropertyName::AlignSelf => "align-self",
            CssPropertyName::AlignContent => "align-content",
            CssPropertyName::JustifyContent => "justify-content",
            CssPropertyName::JustifyItems => "justify-items",
            CssPropertyName::JustifySelf => "justify-self",
            CssPropertyName::Order => "order",
            CssPropertyName::GridTemplateColumns => "grid-template-columns",
            CssPropertyName::GridTemplateRows => "grid-template-rows",
            CssPropertyName::GridAutoFlow => "grid-auto-flow",
            CssPropertyName::GridAutoColumns => "grid-auto-columns",
            CssPropertyName::GridAutoRows => "grid-auto-rows",
            CssPropertyName::GridColumnStart => "grid-column-start",
            CssPropertyName::GridColumnEnd => "grid-column-end",
            CssPropertyName::GridRowStart => "grid-row-start",
            CssPropertyName::GridRowEnd => "grid-row-end",
            CssPropertyName::ColumnGap => "column-gap",
            CssPropertyName::RowGap => "row-gap",
            CssPropertyName::Color => "color",
            CssPropertyName::BackgroundColor => "background-color",
            CssPropertyName::Opacity => "opacity",
            CssPropertyName::Overflow => "overflow",
            CssPropertyName::OverflowX => "overflow-x",
            CssPropertyName::OverflowY => "overflow-y",
            CssPropertyName::Visibility => "visibility",
            CssPropertyName::FontSize => "font-size",
            CssPropertyName::FontWeight => "font-weight",
            CssPropertyName::FontFamily => "font-family",
            CssPropertyName::FontStyle => "font-style",
            CssPropertyName::LineHeight => "line-height",
            CssPropertyName::TextAlign => "text-align",
            CssPropertyName::TextDecoration => "text-decoration",
            CssPropertyName::TextTransform => "text-transform",
            CssPropertyName::LetterSpacing => "letter-spacing",
            CssPropertyName::WordSpacing => "word-spacing",
            CssPropertyName::WhiteSpace => "white-space",
            CssPropertyName::VerticalAlign => "vertical-align",
            CssPropertyName::AspectRatio => "aspect-ratio",
            CssPropertyName::ObjectFit => "object-fit",
            CssPropertyName::ObjectPosition => "object-position",
            CssPropertyName::Custom(s) | CssPropertyName::Other(s) => s.as_str(),
        }
    }
}

/// A CSS component value — the building block of CSS property values.
///
/// Follows the CSS Syntax specification's component value model. A property
/// value is a list of component values that may include functions (which
/// recursively contain component values).
#[derive(Archive, Deserialize, Serialize, Debug, PartialEq, Clone)]
#[rkyv(
    bytecheck(bounds(__C: rkyv::validation::ArchiveContext, __C::Error: rkyv::rancor::Source)),
    serialize_bounds(__S: rkyv::ser::Writer, __S: rkyv::ser::Allocator, __S::Error: rkyv::rancor::Source),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum CssComponentValue {
    /// A CSS-wide keyword (inherit, initial, unset, revert, revert-layer).
    CssWide(CssWideKeyword),
    /// An identifier token (e.g., "red", "auto", "flex", "block").
    Ident(String),
    /// A numeric value with a typed unit (includes bare numbers as `Unitless`).
    Number(f32, CssUnit),
    /// A quoted string value (e.g., `"Helvetica Neue"`).
    QuotedString(String),
    /// A hash token without the leading `#` (e.g., `ff0000` from `#ff0000`).
    Hash(String),
    /// A delimiter character (e.g., `+`, `-`, `*`, `/`).
    Delimiter(char),
    /// A comma separator.
    Comma,
    /// A function call with its name and argument component values.
    ///
    /// Examples: `rgb(255, 0, 0)`, `calc(100% - 20px)`, `var(--x)`.
    Function(String, #[rkyv(omit_bounds)] Vec<CssComponentValue>),
    /// Fallback for values that could not be parsed into structured form.
    Unparsed(String),
}

/// A single CSS property declaration in the intermediate representation.
#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[rkyv(
    bytecheck(bounds(__C: rkyv::validation::ArchiveContext, __C::Error: rkyv::rancor::Source)),
    serialize_bounds(__S: rkyv::ser::Writer, __S: rkyv::ser::Allocator, __S::Error: rkyv::rancor::Source),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct PropertyDeclarationIR {
    pub name: CssPropertyName,
    #[rkyv(omit_bounds)]
    pub value: Vec<CssComponentValue>,
    pub important: bool,
}

impl ArchivedCssPropertyName {
    /// Returns the CSS property name string for the archived variant.
    pub fn as_str(&self) -> &str {
        match self {
            ArchivedCssPropertyName::Display => "display",
            ArchivedCssPropertyName::BoxSizing => "box-sizing",
            ArchivedCssPropertyName::Width => "width",
            ArchivedCssPropertyName::Height => "height",
            ArchivedCssPropertyName::MinWidth => "min-width",
            ArchivedCssPropertyName::MinHeight => "min-height",
            ArchivedCssPropertyName::MaxWidth => "max-width",
            ArchivedCssPropertyName::MaxHeight => "max-height",
            ArchivedCssPropertyName::MarginTop => "margin-top",
            ArchivedCssPropertyName::MarginRight => "margin-right",
            ArchivedCssPropertyName::MarginBottom => "margin-bottom",
            ArchivedCssPropertyName::MarginLeft => "margin-left",
            ArchivedCssPropertyName::PaddingTop => "padding-top",
            ArchivedCssPropertyName::PaddingRight => "padding-right",
            ArchivedCssPropertyName::PaddingBottom => "padding-bottom",
            ArchivedCssPropertyName::PaddingLeft => "padding-left",
            ArchivedCssPropertyName::BorderTopWidth => "border-top-width",
            ArchivedCssPropertyName::BorderRightWidth => "border-right-width",
            ArchivedCssPropertyName::BorderBottomWidth => "border-bottom-width",
            ArchivedCssPropertyName::BorderLeftWidth => "border-left-width",
            ArchivedCssPropertyName::BorderTopStyle => "border-top-style",
            ArchivedCssPropertyName::BorderRightStyle => "border-right-style",
            ArchivedCssPropertyName::BorderBottomStyle => "border-bottom-style",
            ArchivedCssPropertyName::BorderLeftStyle => "border-left-style",
            ArchivedCssPropertyName::BorderTopColor => "border-top-color",
            ArchivedCssPropertyName::BorderRightColor => "border-right-color",
            ArchivedCssPropertyName::BorderBottomColor => "border-bottom-color",
            ArchivedCssPropertyName::BorderLeftColor => "border-left-color",
            ArchivedCssPropertyName::BorderTopLeftRadius => "border-top-left-radius",
            ArchivedCssPropertyName::BorderTopRightRadius => "border-top-right-radius",
            ArchivedCssPropertyName::BorderBottomLeftRadius => "border-bottom-left-radius",
            ArchivedCssPropertyName::BorderBottomRightRadius => "border-bottom-right-radius",
            ArchivedCssPropertyName::Position => "position",
            ArchivedCssPropertyName::Top => "top",
            ArchivedCssPropertyName::Right => "right",
            ArchivedCssPropertyName::Bottom => "bottom",
            ArchivedCssPropertyName::Left => "left",
            ArchivedCssPropertyName::ZIndex => "z-index",
            ArchivedCssPropertyName::Float => "float",
            ArchivedCssPropertyName::Clear => "clear",
            ArchivedCssPropertyName::FlexDirection => "flex-direction",
            ArchivedCssPropertyName::FlexWrap => "flex-wrap",
            ArchivedCssPropertyName::FlexGrow => "flex-grow",
            ArchivedCssPropertyName::FlexShrink => "flex-shrink",
            ArchivedCssPropertyName::FlexBasis => "flex-basis",
            ArchivedCssPropertyName::AlignItems => "align-items",
            ArchivedCssPropertyName::AlignSelf => "align-self",
            ArchivedCssPropertyName::AlignContent => "align-content",
            ArchivedCssPropertyName::JustifyContent => "justify-content",
            ArchivedCssPropertyName::JustifyItems => "justify-items",
            ArchivedCssPropertyName::JustifySelf => "justify-self",
            ArchivedCssPropertyName::Order => "order",
            ArchivedCssPropertyName::GridTemplateColumns => "grid-template-columns",
            ArchivedCssPropertyName::GridTemplateRows => "grid-template-rows",
            ArchivedCssPropertyName::GridAutoFlow => "grid-auto-flow",
            ArchivedCssPropertyName::GridAutoColumns => "grid-auto-columns",
            ArchivedCssPropertyName::GridAutoRows => "grid-auto-rows",
            ArchivedCssPropertyName::GridColumnStart => "grid-column-start",
            ArchivedCssPropertyName::GridColumnEnd => "grid-column-end",
            ArchivedCssPropertyName::GridRowStart => "grid-row-start",
            ArchivedCssPropertyName::GridRowEnd => "grid-row-end",
            ArchivedCssPropertyName::ColumnGap => "column-gap",
            ArchivedCssPropertyName::RowGap => "row-gap",
            ArchivedCssPropertyName::Color => "color",
            ArchivedCssPropertyName::BackgroundColor => "background-color",
            ArchivedCssPropertyName::Opacity => "opacity",
            ArchivedCssPropertyName::Overflow => "overflow",
            ArchivedCssPropertyName::OverflowX => "overflow-x",
            ArchivedCssPropertyName::OverflowY => "overflow-y",
            ArchivedCssPropertyName::Visibility => "visibility",
            ArchivedCssPropertyName::FontSize => "font-size",
            ArchivedCssPropertyName::FontWeight => "font-weight",
            ArchivedCssPropertyName::FontFamily => "font-family",
            ArchivedCssPropertyName::FontStyle => "font-style",
            ArchivedCssPropertyName::LineHeight => "line-height",
            ArchivedCssPropertyName::TextAlign => "text-align",
            ArchivedCssPropertyName::TextDecoration => "text-decoration",
            ArchivedCssPropertyName::TextTransform => "text-transform",
            ArchivedCssPropertyName::LetterSpacing => "letter-spacing",
            ArchivedCssPropertyName::WordSpacing => "word-spacing",
            ArchivedCssPropertyName::WhiteSpace => "white-space",
            ArchivedCssPropertyName::VerticalAlign => "vertical-align",
            ArchivedCssPropertyName::AspectRatio => "aspect-ratio",
            ArchivedCssPropertyName::ObjectFit => "object-fit",
            ArchivedCssPropertyName::ObjectPosition => "object-position",
            ArchivedCssPropertyName::Custom(s) | ArchivedCssPropertyName::Other(s) => s.as_str(),
        }
    }
}
