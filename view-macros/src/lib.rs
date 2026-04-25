extern crate proc_macro;

use std::path::PathBuf;

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

/// Reads an image file at compile time and emits `(&'static [u8], &'static str)`
/// where the first element is the raw bytes and the second is the MIME type
/// inferred from the file extension.
///
/// Path resolution is relative to the invoking crate's `CARGO_MANIFEST_DIR`.
/// Compared to embedding base64 `data:` URLs, this avoids ~33% binary bloat
/// and skips a runtime base64 decode on every lookup.
///
/// Supported extensions: `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.svg`,
/// `.bmp`. Unknown extensions fail at compile time.
///
/// The emitted code uses `include_bytes!` so cargo automatically tracks
/// the file as a build dependency; no manual `rerun-if-changed` plumbing
/// is required.
#[proc_macro]
pub fn inline_image(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);
    let raw_path = lit.value();
    let span = lit.span();

    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => dir,
        Err(_) => {
            return syn::Error::new(
                span,
                "inline_image! requires CARGO_MANIFEST_DIR to be set (it is set by cargo \
                 automatically during builds)",
            )
            .to_compile_error()
            .into();
        }
    };

    let full_path = PathBuf::from(&manifest_dir).join(&raw_path);
    let full_path_display = full_path.display().to_string();

    // Verify the file exists at macro expansion time so the error cites
    // the inline_image! call site rather than a deeper include_bytes!
    // failure with a less-useful span.
    if let Err(err) = std::fs::metadata(&full_path) {
        return syn::Error::new(
            span,
            format!("inline_image! could not open {full_path_display}: {err}"),
        )
        .to_compile_error()
        .into();
    }

    let mime = match mime_for_path(&full_path) {
        Some(m) => m,
        None => {
            return syn::Error::new(
                span,
                format!(
                    "inline_image! does not recognise the extension of `{raw_path}`; \
                     supported extensions are: png, jpg, jpeg, gif, webp, svg, bmp"
                ),
            )
            .to_compile_error()
            .into();
        }
    };

    let expanded = quote! {
        {
            const BYTES: &[u8] = include_bytes!(#full_path_display);
            (BYTES, #mime)
        }
    };

    expanded.into()
}

/// Maps a file extension to a MIME type. Returns `None` for unknown
/// extensions so the macro can surface a clear compile error.
fn mime_for_path(path: &std::path::Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        _ => return None,
    })
}
