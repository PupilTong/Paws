use super::decl_only::DeclOnlyParser;
use super::nested_body::NestedBodyParser;
use super::partition_body_items;
use cssparser::{Parser, RuleBodyParser};
use paws_style_ir::AtRuleBlockIR;

pub fn is_declaration_at_rule(name: &str) -> bool {
    matches!(name, "font-face" | "property" | "counter-style" | "page")
}

pub fn parse_at_rule_block<'i, 't>(
    name: &str,
    input: &mut Parser<'i, 't>,
) -> Result<AtRuleBlockIR, cssparser::ParseError<'i, ()>> {
    if is_declaration_at_rule(name) {
        let mut parser = DeclOnlyParser;
        let iter = RuleBodyParser::new(input, &mut parser);
        let mut items = Vec::new();
        for item in iter.flatten() {
            items.push(item);
        }
        let (decls, _) = partition_body_items(items);
        Ok(AtRuleBlockIR::Declarations(decls))
    } else {
        let mut parser = NestedBodyParser;
        let iter = RuleBodyParser::new(input, &mut parser);
        let mut items = Vec::new();
        for item in iter.flatten() {
            items.push(item);
        }
        let (decls, rules) = partition_body_items(items);
        if decls.is_empty() {
            Ok(AtRuleBlockIR::Rules(rules))
        } else if rules.is_empty() {
            Ok(AtRuleBlockIR::Declarations(decls))
        } else {
            let mut all_rules = Vec::new();
            all_rules.push(paws_style_ir::CssRuleIR::Style(
                paws_style_ir::StyleRuleIR {
                    selectors: String::new(),
                    declarations: decls,
                    rules: Vec::new(),
                },
            ));
            all_rules.extend(rules);
            Ok(AtRuleBlockIR::Rules(all_rules))
        }
    }
}
