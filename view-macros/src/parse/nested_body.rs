use super::at_rule::parse_at_rule_block;
use super::{collect_tokens_as_string, AtRulePrelude, BodyItem};
use cssparser::{
    AtRuleParser, DeclarationParser, Parser, ParserState, QualifiedRuleParser, RuleBodyItemParser,
    RuleBodyParser,
};
use paws_style_ir::{CssRuleIR, PropertyDeclarationIR, StyleRuleIR};

pub struct NestedBodyParser;

impl<'i> DeclarationParser<'i> for NestedBodyParser {
    type Declaration = BodyItem;
    type Error = ();

    fn parse_value<'t>(
        &mut self,
        name: cssparser::CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _state: &ParserState,
    ) -> Result<Self::Declaration, cssparser::ParseError<'i, Self::Error>> {
        let state = input.state();
        let mut ir_value = None;
        let token = input.next().ok().cloned();
        if let Some(token) = token {
            if input.is_exhausted() {
                match token {
                    cssparser::Token::Ident(ident) => {
                        ir_value = Some(paws_style_ir::CssPropertyIR::Keyword(ident.to_string()));
                    }
                    cssparser::Token::Dimension { value, unit, .. } => {
                        ir_value =
                            Some(paws_style_ir::CssPropertyIR::Unit(value, unit.to_string()));
                    }
                    cssparser::Token::Percentage { unit_value, .. } => {
                        ir_value = Some(paws_style_ir::CssPropertyIR::Unit(
                            unit_value * 100.0,
                            "%".to_string(),
                        ));
                    }
                    cssparser::Token::Number { value, .. } => {
                        ir_value = Some(paws_style_ir::CssPropertyIR::Unit(value, "".to_string()));
                    }
                    _ => {}
                }
            }
        }
        let value = if let Some(ir) = ir_value {
            ir
        } else {
            input.reset(&state);
            paws_style_ir::CssPropertyIR::Unparsed(collect_tokens_as_string(input))
        };

        Ok(BodyItem::Declaration(PropertyDeclarationIR {
            name: name.to_string(),
            value,
        }))
    }
}

impl<'i> QualifiedRuleParser<'i> for NestedBodyParser {
    type Prelude = String;
    type QualifiedRule = BodyItem;
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
        let mut declarations = Vec::new();
        let mut rules = Vec::new();
        let mut decl_parser = NestedBodyParser;
        let iter = RuleBodyParser::new(input, &mut decl_parser);
        for item in iter.flatten() {
            match item {
                BodyItem::Declaration(decl) => declarations.push(decl),
                BodyItem::Rule(rule) => rules.push(rule),
            }
        }
        Ok(BodyItem::Rule(CssRuleIR::Style(StyleRuleIR {
            selectors: prelude,
            declarations,
            rules,
        })))
    }
}

impl<'i> AtRuleParser<'i> for NestedBodyParser {
    type Prelude = AtRulePrelude;
    type AtRule = BodyItem;
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
        Ok(BodyItem::Rule(CssRuleIR::AtRule(paws_style_ir::AtRuleIR {
            name: prelude.name,
            prelude: prelude.prelude.clone(),
            block: Some(block),
        })))
    }

    fn rule_without_block(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
    ) -> Result<Self::AtRule, ()> {
        Ok(BodyItem::Rule(CssRuleIR::AtRule(paws_style_ir::AtRuleIR {
            name: prelude.name,
            prelude: prelude.prelude.clone(),
            block: None,
        })))
    }
}

impl<'i> RuleBodyItemParser<'i, BodyItem, ()> for NestedBodyParser {
    fn parse_declarations(&self) -> bool {
        true
    }
    fn parse_qualified(&self) -> bool {
        true
    }
}
