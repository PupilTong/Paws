use super::AtRulePrelude;
use super::{collect_tokens_as_string, BodyItem};
use cssparser::{
    AtRuleParser, DeclarationParser, Parser, ParserState, QualifiedRuleParser, RuleBodyItemParser,
};
use paws_style_ir::PropertyDeclarationIR;

pub struct DeclOnlyParser;

impl<'i> DeclarationParser<'i> for DeclOnlyParser {
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

impl<'i> QualifiedRuleParser<'i> for DeclOnlyParser {
    type Prelude = String;
    type QualifiedRule = BodyItem;
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        Err(input.new_custom_error(()))
    }

    fn parse_block<'t>(
        &mut self,
        _prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, cssparser::ParseError<'i, Self::Error>> {
        Err(input.new_custom_error(()))
    }
}

impl<'i> AtRuleParser<'i> for DeclOnlyParser {
    type Prelude = AtRulePrelude;
    type AtRule = BodyItem;
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        _name: cssparser::CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        Err(input.new_custom_error(()))
    }

    fn parse_block<'t>(
        &mut self,
        _prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, cssparser::ParseError<'i, Self::Error>> {
        Err(input.new_custom_error(()))
    }

    fn rule_without_block(
        &mut self,
        _prelude: Self::Prelude,
        _start: &ParserState,
    ) -> Result<Self::AtRule, ()> {
        Err(())
    }
}

impl<'i> RuleBodyItemParser<'i, BodyItem, ()> for DeclOnlyParser {
    fn parse_declarations(&self) -> bool {
        true
    }
    fn parse_qualified(&self) -> bool {
        false
    }
}
