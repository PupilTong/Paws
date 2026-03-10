use super::{AtRuleIR, StyleRuleIR};
use rkyv::{Archive, Deserialize, Serialize};

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[rkyv(
    bytecheck(bounds(__C: rkyv::validation::ArchiveContext, __C::Error: rkyv::rancor::Source)),
    serialize_bounds(__S: rkyv::ser::Writer, __S: rkyv::ser::Allocator, __S::Error: rkyv::rancor::Source),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum CssRuleIR {
    Style(StyleRuleIR),
    AtRule(AtRuleIR),
}
