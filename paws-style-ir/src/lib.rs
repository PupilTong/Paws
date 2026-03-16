#![recursion_limit = "256"]
#![no_std]
// We use no_std so this can be easily included in the macro and the engine or any wasm target without overhead.

extern crate alloc;

mod at_rule;
mod css_rule;
mod property;
mod property_value;
mod style_rule;
mod stylesheet;
pub mod values;

pub use at_rule::{ArchivedAtRuleBlockIR, ArchivedAtRuleIR, AtRuleBlockIR, AtRuleIR};
pub use css_rule::{ArchivedCssRuleIR, CssRuleIR};
pub use property::{
    ArchivedCssPropertyName, ArchivedCssToken, ArchivedCssUnit, ArchivedCssWideKeyword,
    ArchivedPropertyDeclarationIR, CssPropertyName, CssToken, CssUnit, CssWideKeyword,
    PropertyDeclarationIR,
};
pub use property_value::{ArchivedPropertyValueIR, PropertyValueIR};
pub use style_rule::{ArchivedStyleRuleIR, StyleRuleIR};
pub use stylesheet::{ArchivedStyleSheetIR, StyleSheetIR};

// Re-export all archived value types for engine consumption.
pub use values::{
    ArchivedBorderStyleIR, ArchivedBoxSizingIR, ArchivedClearIR, ArchivedDisplayIR,
    ArchivedFlexBasisIR, ArchivedFlexDirectionIR, ArchivedFlexWrapIR, ArchivedFloatIR,
    ArchivedGapIR, ArchivedInsetIR, ArchivedIntegerIR, ArchivedLengthPercentageIR,
    ArchivedMarginIR, ArchivedMaxSizeIR, ArchivedNonNegativeLPIR, ArchivedNonNegativeNumberIR,
    ArchivedObjectFitIR, ArchivedOverflowIR, ArchivedPositionIR, ArchivedSizeIR,
    ArchivedVisibilityIR, ArchivedZIndexIR, BorderStyleIR, BoxSizingIR, ClearIR, DisplayIR,
    FlexBasisIR, FlexDirectionIR, FlexWrapIR, FloatIR, GapIR, InsetIR, IntegerIR,
    LengthPercentageIR, MarginIR, MaxSizeIR, NonNegativeLPIR, NonNegativeNumberIR, ObjectFitIR,
    OverflowIR, PositionIR, SizeIR, VisibilityIR, ZIndexIR,
};
