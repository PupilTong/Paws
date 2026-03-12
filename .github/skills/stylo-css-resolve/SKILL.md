---
name: stylo-css-resolve
description: Resolve inline CSS via Stylo and serialize computed longhand values. Use for any style-related changes in engine/src/style.rs.
---

**Core APIs to Use**
- `ParserContext` + `CssRuleType::Style` + `parse_property_declaration_list` for inline styles.
- `RuleTree`, `CascadeLevel::same_tree_author_normal()`, `LayerOrder::style_attribute()` for cascade ordering.
- `apply_declarations` to compute `ComputedValues` from declaration blocks.
- `ToCss` + `CssWriter` + `CssStringWriter` for computed value serialization.

**DOM Shim Requirements**
- Keep minimal implementations of `TNode`, `TElement`, `TDocument`, `TShadowRoot`, and `selectors::Element`.
- Implement required trait methods with safe, inert defaults; avoid DOM state and mutation.

**Typed Units & Device Setup**
- Use `euclid::Size2D<CSSPixel>` and `Scale<CSSPixel, DevicePixel>` in `Device::new`.
- Provide a minimal `FontMetricsProvider` returning default ascent.

**Property Support**
- Currently serialized: `height`, `width`, `display`, `color`, `background-color`.
- Extend `serialize_computed_value()` with additional `LonghandId` matches as needed.

**Constraints**
- Inline-only cascade (no UA/user stylesheets or external rules).
- Avoid heavy DOM logic; keep deterministic behavior.

**Quality Gates**
- Run `cargo test --all` after changes; keep `fmt`/`clippy` green.
