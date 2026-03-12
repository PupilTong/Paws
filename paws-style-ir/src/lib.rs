#![recursion_limit = "256"]
#![no_std]
// We use no_std so this can be easily included in the macro and the engine or any wasm target without overhead.

extern crate alloc;

mod at_rule;
mod css_rule;
mod property;
mod style_rule;
mod stylesheet;

pub use at_rule::{ArchivedAtRuleBlockIR, ArchivedAtRuleIR, AtRuleBlockIR, AtRuleIR};
pub use css_rule::{ArchivedCssRuleIR, CssRuleIR};
pub use property::{
    ArchivedCssPropertyIR, ArchivedCssPropertyName, ArchivedCssUnit, ArchivedCssWideKeyword,
    ArchivedPropertyDeclarationIR, CssPropertyIR, CssPropertyName, CssUnit, CssWideKeyword,
    PropertyDeclarationIR,
};
pub use style_rule::{ArchivedStyleRuleIR, StyleRuleIR};
pub use stylesheet::{ArchivedStyleSheetIR, StyleSheetIR};
