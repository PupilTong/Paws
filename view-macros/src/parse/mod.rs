use paws_style_ir::{CssRuleIR, PropertyDeclarationIR};

pub mod at_rule;
pub mod decl_only;
pub mod nested_body;
pub mod stylesheet;

pub enum BodyItem {
    Declaration(PropertyDeclarationIR),
    Rule(CssRuleIR),
}

pub struct AtRulePrelude {
    pub name: String,
    pub prelude: String,
}

pub fn collect_tokens_as_string<'i, 't>(input: &mut cssparser::Parser<'i, 't>) -> String {
    let position = input.position();
    while input.next().is_ok() {}
    input.slice_from(position).trim().to_string()
}

pub fn partition_body_items(items: Vec<BodyItem>) -> (Vec<PropertyDeclarationIR>, Vec<CssRuleIR>) {
    let mut decls = Vec::new();
    let mut rules = Vec::new();
    for item in items {
        match item {
            BodyItem::Declaration(d) => decls.push(d),
            BodyItem::Rule(r) => rules.push(r),
        }
    }
    (decls, rules)
}
