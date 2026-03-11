use super::AtRulePrelude;
use super::BodyItem;
use cssparser::{
    AtRuleParser, DeclarationParser, Parser, ParserState, QualifiedRuleParser, RuleBodyItemParser,
};

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
        Ok(BodyItem::Declaration(super::parse_declaration(name, input)))
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
