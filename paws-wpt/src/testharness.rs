//! Rust analog of the [`testharness.js`][1] subset we use in
//! translated WPT tests.
//!
//! The vocabulary is intentionally minimal: only the assertions
//! actually reached for by current translations. Add helpers when a
//! translation needs one — not pre-emptively.
//!
//! Each helper panics on failure. The cargo test runner reports the
//! panic as a test failure, which is sufficient for the current
//! pass/fail signal. A structured `wptreport.json`-shaped emitter can
//! be layered on later if we ever want a wpt.fyi-style dashboard.
//!
//! [1]: https://web-platform-tests.org/writing-tests/testharness-api.html

use std::fmt::Debug;

/// Asserts `actual == expected`. Mirrors testharness.js
/// `assert_equals(actual, expected, description)`.
#[track_caller]
pub fn assert_equals<T>(actual: T, expected: T, description: &str)
where
    T: PartialEq + Debug,
{
    if actual != expected {
        panic!("FAIL [{description}]: assert_equals — expected {expected:?}, got {actual:?}");
    }
}

/// Asserts that `actual` is `true`. Mirrors testharness.js `assert_true`.
#[track_caller]
pub fn assert_true(actual: bool, description: &str) {
    if !actual {
        panic!("FAIL [{description}]: assert_true — expected true, got false");
    }
}

/// Asserts that `actual` is `false`. Mirrors testharness.js `assert_false`.
#[track_caller]
pub fn assert_false(actual: bool, description: &str) {
    if actual {
        panic!("FAIL [{description}]: assert_false — expected false, got true");
    }
}

/// Asserts that two slices are element-wise equal. Mirrors testharness.js
/// `assert_array_equals`.
#[track_caller]
pub fn assert_array_equals<T>(actual: &[T], expected: &[T], description: &str)
where
    T: PartialEq + Debug,
{
    if actual != expected {
        panic!("FAIL [{description}]: assert_array_equals — expected {expected:?}, got {actual:?}");
    }
}

/// HTML namespace URI — the namespace assigned by `document.createElement`
/// in an HTML document. Spec: <https://dom.spec.whatwg.org/#dom-document-createelement>.
pub const HTML_NS: &str = "http://www.w3.org/1999/xhtml";

/// SVG namespace URI. Used by elements created via `createElementNS`
/// with the SVG namespace.
pub const SVG_NS: &str = "http://www.w3.org/2000/svg";

/// MathML namespace URI.
pub const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";

/// XML namespace URI.
pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// XMLNS namespace URI.
pub const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";
