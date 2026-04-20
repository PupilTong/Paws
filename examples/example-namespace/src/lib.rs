//! Exercises `create_element_ns` and `get_namespace_uri` host functions.
//!
//! Creates an SVG root, a child SVG element, and a MathML element, then reads
//! back the namespace URIs to verify the host stored them correctly. All of
//! these are appended to the document root.
//!
//! Also exercises the following error and edge-case paths so the FFI wrappers
//! and the host-side linker branches are fully covered:
//!   * `get_namespace_uri` on a text node → `Ok(None)`
//!   * `get_namespace_uri` on an invalid (out-of-range) id → `Err`
//!   * `get_namespace_uri` on a negative id → host-side `id < 0` guard

use rust_wasm_binding::*;

const SVG_NS: &str = "http://www.w3.org/2000/svg";
const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            // Create a namespaced SVG element
            let svg_id = create_element_ns(SVG_NS, "svg")?;
            append_element(0, svg_id)?;

            // Create a child SVG element
            let circle_id = create_element_ns(SVG_NS, "circle")?;
            append_element(svg_id, circle_id)?;

            // Create a MathML element
            let math_id = create_element_ns(MATHML_NS, "math")?;
            append_element(0, math_id)?;

            // Create a regular HTML element for comparison
            let div_id = create_element("div")?;
            append_element(0, div_id)?;

            // Create a text node for the `Ok(None)` / "no namespace" path
            let text_id = create_text_node("hello")?;
            append_element(0, text_id)?;

            // Read back namespace URIs via get_namespace_uri

            // SVG root should report SVG namespace
            match get_namespace_uri(svg_id)? {
                Some(uri) if uri == SVG_NS => { /* ok */ }
                _ => return Err(-101),
            }

            // Circle should also report SVG namespace
            match get_namespace_uri(circle_id)? {
                Some(uri) if uri == SVG_NS => { /* ok */ }
                _ => return Err(-103),
            }

            // Math element should report MathML namespace
            match get_namespace_uri(math_id)? {
                Some(uri) if uri == MATHML_NS => { /* ok */ }
                _ => return Err(-105),
            }

            // Regular div should report HTML namespace (set implicitly by create_element)
            match get_namespace_uri(div_id)? {
                Some(_) => { /* ok — any HTML namespace is fine */ }
                None => return Err(-106),
            }

            // Text node has no QualName → host returns None → wrapper returns Ok(None)
            match get_namespace_uri(text_id) {
                Ok(None) => { /* ok */ }
                _ => return Err(-107),
            }

            // Invalid (out-of-range) node id → host returns InvalidChild → wrapper Err
            match get_namespace_uri(9999) {
                Err(_) => { /* ok */ }
                Ok(_) => return Err(-108),
            }

            // Negative node id → host-side `if id < 0` early guard → wrapper Err
            match get_namespace_uri(-1) {
                Err(_) => { /* ok */ }
                Ok(_) => return Err(-109),
            }

            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
