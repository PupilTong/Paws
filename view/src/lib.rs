#![no_std]

pub use view_macros::css;

#[link(wasm_import_module = "paws")]
extern "C" {
    fn paws_add_parsed_stylesheet(ptr: *const u8, len: usize);
}

/// Applies a pre-parsed CSS stylesheet to the engine globally.
pub fn apply_css(css_bytes: &[u8]) {
    unsafe {
        paws_add_parsed_stylesheet(css_bytes.as_ptr(), css_bytes.len());
    }
}

#[cfg(test)]
mod tests {
    use view_macros::css;

    #[test]
    fn test_css_macro_outputs_bytes() {
        let stylesheet_bytes = css!(
            r#"
            div {
                color: red;
                display: flex;
            }
            .classy {
                font-size: 16px;
            }
            "#
        );

        // We check that it evaluates to a non-empty byte slice
        assert!(
            !stylesheet_bytes.is_empty(),
            "CSS macro should generate byte slice"
        );

        // Verify rkyv decoding works (integration sanity check)
        let ir =
            rkyv::from_bytes::<paws_style_ir::StyleSheetIR, rkyv::rancor::Error>(stylesheet_bytes)
                .unwrap();
        assert_eq!(ir.rules.len(), 2);
        assert_eq!(ir.rules[0].selectors, "div");
        assert_eq!(ir.rules[0].declarations.len(), 2);
        assert_eq!(ir.rules[0].declarations[0].name, "color");
        assert_eq!(ir.rules[0].declarations[0].value, "red");

        assert_eq!(ir.rules[1].selectors, ".classy");
    }
}
