use alloc::string::String;
use alloc::vec::Vec;
use rkyv::{Archive, Deserialize, Serialize};

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq, Clone)]
#[rkyv(
    bytecheck(bounds(__C: rkyv::validation::ArchiveContext, __C::Error: rkyv::rancor::Source)),
    serialize_bounds(__S: rkyv::ser::Writer, __S: rkyv::ser::Allocator, __S::Error: rkyv::rancor::Source),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum CssPropertyIR {
    Keyword(String),
    Unit(f32, String),
    Sum(Vec<(f32, String)>),
    Unparsed(String),
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[rkyv(
    bytecheck(bounds(__C: rkyv::validation::ArchiveContext, __C::Error: rkyv::rancor::Source)),
    serialize_bounds(__S: rkyv::ser::Writer, __S: rkyv::ser::Allocator, __S::Error: rkyv::rancor::Source),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct PropertyDeclarationIR {
    pub name: String,
    pub value: CssPropertyIR,
}
