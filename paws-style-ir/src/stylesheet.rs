use super::CssRuleIR;
use alloc::vec::Vec;
use rkyv::{Archive, Deserialize, Serialize};

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
pub struct StyleSheetIR {
    pub rules: Vec<CssRuleIR>,
}
