extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, LitStr};

use rkyv::rancor::Error;
use rkyv::to_bytes;

use cssparser::{Parser, ParserInput, StyleSheetParser};
use paws_style_ir::StyleSheetIR;

mod parse;
use parse::stylesheet::StyleRuleParser;

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
