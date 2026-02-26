#![no_std]
// We use no_std so this can be easily included in the macro and the engine or any wasm target without overhead.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use rkyv::{Archive, Deserialize, Serialize};

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
pub struct StyleSheetIR {
    pub rules: Vec<StyleRuleIR>,
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
pub struct StyleRuleIR {
    pub selectors: String,
    pub declarations: Vec<PropertyDeclarationIR>,
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
pub struct PropertyDeclarationIR {
    pub name: String,
    pub value: String,
}
