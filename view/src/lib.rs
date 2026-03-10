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

        match &ir.rules[0] {
            paws_style_ir::CssRuleIR::Style(s) => {
                assert_eq!(s.selectors, "div");
                assert_eq!(s.declarations.len(), 2);
                assert_eq!(s.declarations[0].name, "color");
                if let paws_style_ir::CssPropertyIR::Keyword(val) = &s.declarations[0].value {
                    assert_eq!(val, "red");
                } else {
                    panic!("Expected Keyword value for declaration 0");
                }
            }
            _ => panic!("Expected Style rule"),
        }

        match &ir.rules[1] {
            paws_style_ir::CssRuleIR::Style(s) => {
                assert_eq!(s.selectors, ".classy");
            }
            _ => panic!("Expected Style rule"),
        }
    }
}
