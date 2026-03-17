//! Typed CSS value enums for the IR layer.
//!
//! These types model CSS property values at a semantic level, inspired by
//! Stylo's `Generic*` value enums.  They are produced at compile time by the
//! `css!()` macro and consumed at runtime by the engine's IR → Stylo
//! conversion layer.
//!
//! All types derive `rkyv` traits for zero-copy deserialization.

use rkyv::{Archive, Deserialize, Serialize};

use crate::CssUnit;

// ─── rkyv derive helper ─────────────────────────────────────────────
// Shared bounds are factored into a macro to keep definitions concise.

macro_rules! rkyv_derive {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident $($rest:tt)*
    ) => {
        $(#[$meta])*
        #[derive(Archive, Deserialize, Serialize, Debug, PartialEq, Clone)]
        #[rkyv(
            bytecheck(bounds(__C: rkyv::validation::ArchiveContext, __C::Error: rkyv::rancor::Source)),
            serialize_bounds(__S: rkyv::ser::Writer, __S: rkyv::ser::Allocator, __S::Error: rkyv::rancor::Source),
            deserialize_bounds(__D::Error: rkyv::rancor::Source)
        )]
        $vis enum $name $($rest)*
    };
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident $($rest:tt)*
    ) => {
        $(#[$meta])*
        #[derive(Archive, Deserialize, Serialize, Debug, PartialEq, Clone)]
        #[rkyv(
            bytecheck(bounds(__C: rkyv::validation::ArchiveContext, __C::Error: rkyv::rancor::Source)),
            serialize_bounds(__S: rkyv::ser::Writer, __S: rkyv::ser::Allocator, __S::Error: rkyv::rancor::Source),
            deserialize_bounds(__D::Error: rkyv::rancor::Source)
        )]
        $vis struct $name $($rest)*
    };
}

// ─── Length / percentage building blocks ─────────────────────────────

rkyv_derive! {
    /// A length or percentage value (allows negative values).
    pub enum LengthPercentageIR {
        /// A length value with a typed unit.
        Length(f32, CssUnit),
        /// A percentage value (stored as the authored value, e.g. `50` for `50%`).
        Percentage(f32),
    }
}

rkyv_derive! {
    /// A non-negative length or percentage value.
    ///
    /// Constructed only via [`NonNegativeLPIR::new`] which enforces `>= 0`
    /// at compile time in the `css!()` macro.
    pub enum NonNegativeLPIR {
        /// A non-negative length value with a typed unit.
        Length(f32, CssUnit),
        /// A non-negative percentage value.
        Percentage(f32),
    }
}

impl NonNegativeLPIR {
    /// Creates a non-negative length-percentage, returning `None` if negative.
    pub fn new_length(val: f32, unit: CssUnit) -> Option<Self> {
        if val < 0.0 {
            None
        } else {
            Some(NonNegativeLPIR::Length(val, unit))
        }
    }

    /// Creates a non-negative percentage, returning `None` if negative.
    pub fn new_percentage(val: f32) -> Option<Self> {
        if val < 0.0 {
            None
        } else {
            Some(NonNegativeLPIR::Percentage(val))
        }
    }
}

// ─── Sizing ─────────────────────────────────────────────────────────

rkyv_derive! {
    /// Value for `width`, `height`, `min-width`, `min-height`.
    ///
    /// Mirrors Stylo's `GenericSize`.
    pub enum SizeIR {
        Auto,
        LengthPercentage(NonNegativeLPIR),
    }
}

rkyv_derive! {
    /// Value for `max-width`, `max-height`.
    ///
    /// Mirrors Stylo's `GenericMaxSize`.
    pub enum MaxSizeIR {
        None,
        LengthPercentage(NonNegativeLPIR),
    }
}

// ─── Box model ──────────────────────────────────────────────────────

rkyv_derive! {
    /// Value for `margin-*` properties.
    ///
    /// Mirrors Stylo's `GenericMargin`.
    pub enum MarginIR {
        Auto,
        LengthPercentage(LengthPercentageIR),
    }
}

rkyv_derive! {
    /// Value for `top`, `right`, `bottom`, `left` (inset properties).
    ///
    /// Mirrors Stylo's `GenericInset`.
    pub enum InsetIR {
        Auto,
        LengthPercentage(LengthPercentageIR),
    }
}

rkyv_derive! {
    /// Value for `column-gap`, `row-gap`.
    ///
    /// Mirrors Stylo's `GenericLengthPercentageOrNormal`.
    pub enum GapIR {
        Normal,
        LengthPercentage(NonNegativeLPIR),
    }
}

// ─── Border ─────────────────────────────────────────────────────────

rkyv_derive! {
    /// Value for `border-*-style` properties.
    #[repr(u8)]
    #[derive(Copy)]
    pub enum BorderStyleIR {
        None,
        Hidden,
        Solid,
        Double,
        Dotted,
        Dashed,
        Groove,
        Ridge,
        Inset,
        Outset,
    }
}

// ─── Display & box model keywords ───────────────────────────────────

rkyv_derive! {
    /// Value for the `display` property.
    #[repr(u8)]
    #[derive(Copy)]
    pub enum DisplayIR {
        Block,
        Inline,
        InlineBlock,
        None,
        Flex,
        Grid,
        Table,
        InlineFlex,
        InlineGrid,
        InlineTable,
        TableRow,
        TableCell,
        TableColumn,
        TableRowGroup,
        TableHeaderGroup,
        TableFooterGroup,
        TableColumnGroup,
        TableCaption,
        Contents,
    }
}

rkyv_derive! {
    /// Value for the `position` property.
    #[repr(u8)]
    #[derive(Copy)]
    pub enum PositionIR {
        Static,
        Relative,
        Absolute,
        Fixed,
        Sticky,
    }
}

rkyv_derive! {
    /// Value for the `box-sizing` property.
    #[repr(u8)]
    #[derive(Copy)]
    pub enum BoxSizingIR {
        ContentBox,
        BorderBox,
    }
}

rkyv_derive! {
    /// Value for the `float` property.
    #[repr(u8)]
    #[derive(Copy)]
    pub enum FloatIR {
        None,
        Left,
        Right,
        InlineStart,
        InlineEnd,
    }
}

rkyv_derive! {
    /// Value for the `clear` property.
    #[repr(u8)]
    #[derive(Copy)]
    pub enum ClearIR {
        None,
        Left,
        Right,
        Both,
        InlineStart,
        InlineEnd,
    }
}

// ─── Visual ─────────────────────────────────────────────────────────

rkyv_derive! {
    /// Value for the `visibility` property.
    #[repr(u8)]
    #[derive(Copy)]
    pub enum VisibilityIR {
        Visible,
        Hidden,
        Collapse,
    }
}

rkyv_derive! {
    /// Value for `overflow-x`, `overflow-y`.
    #[repr(u8)]
    #[derive(Copy)]
    pub enum OverflowIR {
        Visible,
        Hidden,
        Scroll,
        Auto,
        Clip,
    }
}

rkyv_derive! {
    /// Value for the `object-fit` property.
    #[repr(u8)]
    #[derive(Copy)]
    pub enum ObjectFitIR {
        Fill,
        Contain,
        Cover,
        None,
        ScaleDown,
    }
}

// ─── Flexbox ────────────────────────────────────────────────────────

rkyv_derive! {
    /// Value for the `flex-direction` property.
    #[repr(u8)]
    #[derive(Copy)]
    pub enum FlexDirectionIR {
        Row,
        RowReverse,
        Column,
        ColumnReverse,
    }
}

rkyv_derive! {
    /// Value for the `flex-wrap` property.
    #[repr(u8)]
    #[derive(Copy)]
    pub enum FlexWrapIR {
        Nowrap,
        Wrap,
        WrapReverse,
    }
}

rkyv_derive! {
    /// Value for the `flex-basis` property.
    pub enum FlexBasisIR {
        Content,
        Size(SizeIR),
    }
}

// ─── Numeric ────────────────────────────────────────────────────────

rkyv_derive! {
    /// A non-negative `<number>` value (used by `flex-grow`, `flex-shrink`).
    ///
    /// Validated `>= 0` at construction time.
    #[derive(Copy)]
    pub struct NonNegativeNumberIR(pub f32);
}

impl NonNegativeNumberIR {
    /// Creates a non-negative number, returning `None` if negative.
    pub fn new(val: f32) -> Option<Self> {
        if val < 0.0 {
            None
        } else {
            Some(NonNegativeNumberIR(val))
        }
    }
}

rkyv_derive! {
    /// A CSS `<integer>` value (used by `order`).
    ///
    /// Validated for integrality at construction time.
    #[derive(Copy)]
    pub struct IntegerIR(pub i32);
}

impl IntegerIR {
    /// Creates an integer from an f32, returning `None` if it has a fractional part.
    pub fn from_f32(val: f32) -> Option<Self> {
        if val.fract() != 0.0 {
            None
        } else {
            Some(IntegerIR(val as i32))
        }
    }
}

// ─── Z-index ────────────────────────────────────────────────────────

rkyv_derive! {
    /// Value for the `z-index` property.
    pub enum ZIndexIR {
        Auto,
        Integer(IntegerIR),
    }
}
