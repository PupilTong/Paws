use cssparser::{AtRuleParser, Parser, ParserState, QualifiedRuleParser, RuleBodyParser};
use paws_style_ir::{CssRuleIR, StyleRuleIR};

use super::at_rule::parse_at_rule_block;
use super::nested_body::NestedBodyParser;
use super::{collect_tokens_as_string, partition_body_items, AtRulePrelude};

pub struct StyleRuleParser;

impl<'i> QualifiedRuleParser<'i> for StyleRuleParser {
    type Prelude = String;
    type QualifiedRule = CssRuleIR;
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        Ok(collect_tokens_as_string(input))
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, cssparser::ParseError<'i, Self::Error>> {
        let mut decl_parser = NestedBodyParser;
        let iter = RuleBodyParser::new(input, &mut decl_parser);
        let mut items = Vec::new();

        for item in iter.flatten() {
            items.push(item);
        }

        let (declarations, rules) = partition_body_items(items);

        Ok(CssRuleIR::Style(StyleRuleIR {
            selectors: prelude,
            declarations,
            rules,
        }))
    }
}

impl<'i> AtRuleParser<'i> for StyleRuleParser {
    type Prelude = AtRulePrelude;
    type AtRule = CssRuleIR;
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        name: cssparser::CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        let prelude = collect_tokens_as_string(input);
        Ok(AtRulePrelude {
            name: name.to_string(),
            prelude,
        })
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, cssparser::ParseError<'i, Self::Error>> {
        let block = parse_at_rule_block(&prelude.name, input)?;
        Ok(CssRuleIR::AtRule(paws_style_ir::AtRuleIR {
            name: prelude.name,
            prelude: prelude.prelude.clone(),
            block: Some(block),
        }))
    }

    fn rule_without_block(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
    ) -> Result<Self::AtRule, ()> {
        Ok(CssRuleIR::AtRule(paws_style_ir::AtRuleIR {
            name: prelude.name,
            prelude: prelude.prelude.clone(),
            block: None,
        }))
    }
}
