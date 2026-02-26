extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, LitStr};

use rkyv::rancor::Error;
use rkyv::to_bytes;

use cssparser::{
    AtRuleParser, DeclarationParser, Parser, ParserInput, ParserState, QualifiedRuleParser,
    RuleBodyItemParser, RuleBodyParser, StyleSheetParser, ToCss,
};
use paws_style_ir::{PropertyDeclarationIR, StyleRuleIR, StyleSheetIR};

struct StyleRuleParser;

impl<'i> QualifiedRuleParser<'i> for StyleRuleParser {
    type Prelude = String;
    type QualifiedRule = StyleRuleIR;
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        let mut selectors = String::new();
        while let Ok(token) = input.next() {
            selectors.push_str(&token.to_css_string());
        }
        Ok(selectors.trim().to_string())
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, cssparser::ParseError<'i, Self::Error>> {
        let mut decl_parser = PropParser;
        let iter = RuleBodyParser::new(input, &mut decl_parser);
        let mut declarations = Vec::new();

        for decl in iter.flatten() {
            declarations.push(decl);
        }

        Ok(StyleRuleIR {
            selectors: prelude,
            declarations,
        })
    }
}

impl<'i> AtRuleParser<'i> for StyleRuleParser {
    type Prelude = ();
    type AtRule = StyleRuleIR;
    type Error = ();
}

struct PropParser;

impl<'i> DeclarationParser<'i> for PropParser {
    type Declaration = PropertyDeclarationIR;
    type Error = ();

    fn parse_value<'t>(
        &mut self,
        name: cssparser::CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _state: &ParserState,
    ) -> Result<Self::Declaration, cssparser::ParseError<'i, Self::Error>> {
        let mut value = String::new();
        while let Ok(token) = input.next() {
            value.push_str(&token.to_css_string());
        }
        Ok(PropertyDeclarationIR {
            name: name.to_string(),
            value: value.trim().to_string(),
        })
    }
}

impl<'i> QualifiedRuleParser<'i> for PropParser {
    type Prelude = ();
    type QualifiedRule = PropertyDeclarationIR;
    type Error = ();
}

impl<'i> AtRuleParser<'i> for PropParser {
    type Prelude = ();
    type AtRule = PropertyDeclarationIR;
    type Error = ();
}

impl<'i> RuleBodyItemParser<'i, PropertyDeclarationIR, ()> for PropParser {
    fn parse_declarations(&self) -> bool {
        true
    }
    fn parse_qualified(&self) -> bool {
        false
    }
}

#[proc_macro]
pub fn css(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);
    let css_str = lit.value();

    let mut input = ParserInput::new(&css_str);
    let mut parser = Parser::new(&mut input);

    let mut rule_parser = StyleRuleParser;
    let iter = StyleSheetParser::new(&mut parser, &mut rule_parser);

    let mut rules = Vec::new();
    for rule in iter.flatten() {
        rules.push(rule);
    }

    let stylesheet = StyleSheetIR { rules };

    // Encode to checking byte format using rkyv
    let bytes = to_bytes::<Error>(&stylesheet).expect("failed to serialize stylesheet");

    let byte_tokens: Vec<_> = bytes.iter().map(|b| quote! { #b }).collect();
    let len = bytes.len();

    let expanded = quote! {
        {
            #[repr(C, align(8))]
            struct Aligned([u8; #len]);
            static ALIGNED: Aligned = Aligned([ #(#byte_tokens),* ]);
            &ALIGNED.0
        }
    };

    expanded.into()
}
