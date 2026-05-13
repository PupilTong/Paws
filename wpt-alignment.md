# WPT Alignment Status

This file tracks Paws's alignment with the [W3C Web Platform Tests (WPT)](https://github.com/web-platform-tests/wpt). It is the source of truth for "how much of the spec do we conform to today" and is read by both humans and AI agents.

## How Paws relates to WPT

- **WPT is a spec corpus, not a runtime target.** Paws does not — and will not — embed a JavaScript engine, so we cannot execute upstream WPT test files. Instead we treat WPT as the canonical description of correct DOM/CSS behavior and translate individual tests into Rust.
- **The Paws Yew fork is the system under test.** Translated tests mount fixtures as Yew components (`html! { ... }`), exercise them through the Paws engine, and assert on engine state. This is the same API surface developers write against, so green tests directly imply "Yew on Paws behaves like Yew on a real browser for this case."
- **Only the Yew flavor.** We do *not* maintain a parallel set of direct `rust_wasm_binding` translations. The Yew variant is the one that matters for the developer-experience promise.

## The reference clone

The upstream WPT repository is cloned at `wpt-reference/` (gitignored). Treat it as **read-only** — it is the spec corpus, not code we ship.

To get the clone:

```sh
git clone --depth 1 --filter=blob:none --sparse https://github.com/web-platform-tests/wpt.git wpt-reference
```

Pin the snapshot per branch by recording the commit hash in your PR description when you start translating against a fresh clone.

Current pinned snapshot: [`e04cee8384c069f6bb7dd54f920ef9395a5e22f5`](https://github.com/web-platform-tests/wpt/commit/e04cee8384c069f6bb7dd54f920ef9395a5e22f5) (pinned 2026-05-10).

## Status summary

| Status | Count |
| --- | --- |
| Translated (Yew flavor passing) | 5 |
| In progress | 0 |
| Skipped (with reason) | 27 |
| Not started | n/a |

Update these counts whenever the per-spec sections change, and copy the table into the PR description.

## Status legend

| Status | Meaning |
| --- | --- |
| `translated` | A Yew-flavor Rust test exists and is green in CI |
| `in-progress` | A Rust test exists but does not yet pass (blocked on engine work) |
| `skipped` | Intentionally not translated; the `Reason` column says why |
| _absent_ | Not yet looked at |

## Per-spec status

Each section corresponds to a top-level WPT directory. Add a row under the relevant section when you translate, start, or skip a test.

### dom/nodes/

| WPT path | Paws Rust test | Status | Reason / notes |
| --- | --- | --- | --- |
| `Document-createElement.html` | `paws-wpt :: dom_nodes :: create_element_div_in_html_document` | translated | HTML-document `<div />` slice only. Mixed-case / invalid-name / XML-doc subtests are out of scope under "Yew flavor only" — see fixture docs at `paws-wpt/fixtures/dom-nodes-document-create-element/src/lib.rs` |

### dom/events/

| WPT path | Paws Rust test | Status | Reason / notes |
| --- | --- | --- | --- |
| _none yet_ | | | |

### dom/lists/

| WPT path | Paws Rust test | Status | Reason / notes |
| --- | --- | --- | --- |
| _none yet_ | | | |

### css/css-cascade/

| WPT path | Paws Rust test | Status | Reason / notes |
| --- | --- | --- | --- |
| _none yet_ | | | |

### css/css-flexbox/

| WPT path | Paws Rust test | Status | Reason / notes |
| --- | --- | --- | --- |
| _none yet_ | | | |

### css/css-overflow/

Scope context: this PR honours `overflow: hidden | clip` on `ViewKind::Layer` (CALayer-backed) nodes by emitting `SetClipsToBounds` ops that Swift maps onto `CALayer.masksToBounds`. The directly-relevant upstream tests are catalogued below. Tests whose primary surface area is a separate engine capability (programmatic scroll, hit-test, paint reftests, `overflow-clip-margin`, SVG, writing modes, transforms) are listed as `skipped` with the gating feature noted.

| WPT path | Paws Rust test | Status | Reason / notes |
| --- | --- | --- | --- |
| `parsing/overflow-computed.html` (longhand subset: `overflow-x`/`-y` × {visible, hidden, scroll, auto, clip}) | `paws-wpt :: css_overflow :: overflow_longhand_computed_values_match_spec` | translated | Each `test_computed_value("overflow-{x,y}", v)` for the 5 spec keywords is verified by reading the computed style off a `<div>` whose class selector applied the longhand via `css!()`. Routes through the engine's bespoke `ir_to_overflow` keyword table at `engine/src/style/ir_convert/keyword.rs`. The `overflow` shorthand subtests are deferred. `overflow-block` / `overflow-inline` subtests are skipped (logical-axis properties not wired through `paws-style-ir`'s typed `CssPropertyName` yet — depend on writing-modes). |
| `parsing/overflow-computed.html` (cross-axis coercion subset: `'hidden visible'` → `'hidden auto'`, etc.) | `paws-wpt :: css_overflow :: overflow_visible_coerces_to_auto_when_other_axis_is_scrollable` | translated | Verifies the spec rule that a visible axis is computed as `auto` when the other axis is `hidden | scroll | auto` (i.e. introduces a scroll container), and stays `visible` when the other axis is `clip` or `visible`. Stylo applies this coercion automatically when consuming computed values, so the longhand IR path picks it up for free. |
| `parsing/overflow-valid.html` (longhand subset) | covered by `overflow_longhand_computed_values_match_spec` | translated | `test_valid_value("overflow-{x,y}", v)` reduces to "specified parses + computes to the same keyword", which the computed-value translation covers for every spec keyword. The shorthand subtests (`'hidden visible'`, etc.) are deferred. |
| `parsing/overflow-invalid.html` (longhand subset: `test_invalid_value("overflow-x", 'visible clip')` + `test_invalid_value("overflow-y", 'clip hidden')`) | `paws-wpt :: css_overflow :: overflow_longhand_two_value_form_is_invalid_and_drops_declaration` | translated | Two-token values on a longhand are invalid; the bespoke `ir_to_overflow` pattern-matches `[Ident(_)]` only, returns `None` for any other shape, and the declaration is dropped at `engine/src/style/ir_convert/mod.rs :: convert_raw_declaration`. The fixture verifies both axes stay at their initial `visible` value. The shorthand subtests (`test_invalid_value("overflow", 'none')`, etc.) are deferred. |
| `clip-001.html` through `clip-008.html`, `dynamic-visible-to-clip-001.html`, `overflow-clip-content-visual-overflow.html`, `overflow-clipped-transparent-border-clip.html` (reftests) | `paws-wpt :: css_overflow :: overflow_hidden_and_clip_emit_layer_mask_ops` | translated (engine-contract slice) | Upstream verifies `overflow: hidden | clip` visual clipping via reftest image comparison. Paws has no reftest framework, so the translation checks the engine-side contract those reftests depend on: the iOS renderer emits `SetClipsToBounds` for clipped `ViewKind::Layer` nodes, which Swift then maps onto `CALayer.masksToBounds`. The fixture mounts three classed children (`.hidden`, `.clip`, `.visible`) under a flex parent; the runner inspects the emitted op stream. The `.visible` child must not emit a clip op (matches CALayer's default). |
| `parsing/overflow-computed.html` — `overflow` shorthand subtests (~25 cases like `'hidden visible'`, `'clip scroll'`, etc.) | _none_ | skipped | The `overflow` shorthand IR path was rolled back; a future PR will introduce a generic mechanism. Currently a `css!()`-compiled `overflow: <values>;` declaration is silently dropped at `engine/src/style/ir_convert/mod.rs :: 469`. |
| `parsing/overflow-computed.html` — `overflow-block` / `overflow-inline` subtests | _none_ | skipped | Logical-axis longhands not yet wired into `paws-style-ir`'s typed `CssPropertyName` enum. Depend on the writing-modes feature, which is also unimplemented. |
| `parsing/overflow-valid.html` — `overflow` shorthand subtests | _none_ | skipped | Shorthand deferred (see above). |
| `parsing/overflow-invalid.html` — shorthand subtests | _none_ | skipped | Shorthand deferred. |
| `clip-001.html` through `clip-008.html`, `dynamic-visible-to-clip-001.html` (per-axis single-axis cases like `overflow-x: clip; overflow-y: visible`) | _none_ | skipped | Single-axis clipping is a real divergence from this PR's implementation: `CALayer.masksToBounds` clips both axes uniformly, so when only one axis is `clip | hidden` and the other is `visible` we over-clip. Spec-correct single-axis clipping needs either a `CAShapeLayer` mask or a child wrapper view; tracked as a follow-up. |
| `clip-005.html` — `outline` interaction | _none_ | skipped | `outline` property not implemented. |
| `clip-008.html` — `border-radius` corner clipping | _none_ | skipped | `border-radius` property not implemented (`CssPropertyName::BorderTop{Left,Right}Radius` & friends fall through to `None` at `engine/src/style/ir_convert/mod.rs :: 419-422`). |
| `overflow-clip-hit-testing.html`, `overflow-clip-margin-hit-testing.html` | _none_ | skipped | Hit-test clipping for `overflow: hidden | clip` is the PR3 follow-up tracked at `engine/src/hit_test/mod.rs :: 14-15`. Today's hit-test ignores ancestor overflow. |
| `overflow-clip-cant-scroll.html` | _none_ | skipped | Asserts that `overflow: clip` refuses programmatic `scrollTo` / `scrollBy`. Programmatic scroll API + the `NotScrollable` error code are the PR5 follow-up; there is no DOM scroll API today. |
| `overflow-clip-no-off-axis-scrollbar.html`, `overflow-clip-scroll-size.html`, `overflow-clip-clamps-and-ignores-scroll-offsets-vertical-rl.html` | _none_ | skipped | Scrollbar / `offsetHeight` / `clientHeight` shape and scroll-offset clamping are PR4–PR6 follow-ups. The iOS renderer uses `UIScrollView`'s native scrollbar; the asserted JS layout-metric surface (`offsetHeight - clientHeight = 0`) is not yet exposed through any host API. |
| `overflow-clip-margin-001.html` through `overflow-clip-margin-022.html`, `overflow-clip-margin-border-radius.html`, `overflow-clip-margin-border-radius-002.html`, `overflow-clip-margin-computed.html`, `overflow-clip-margin-hit-testing.html`, `overflow-clip-margin-intersection-observer.html`, `overflow-clip-margin-invalidation.html`, `overflow-clip-margin-mul-column-{border,content,padding}-box.html`, `overflow-clip-margin-svg.html`, `overflow-clip-margin-visual-box{,-and-value{,-with-border-radius}}.html`, `overflow-clip-margin.html` | _none_ | skipped | `overflow-clip-margin` property (`https://drafts.csswg.org/css-overflow-3/#overflow-clip-margin`) is not implemented — not in `paws-style-ir`'s `CssPropertyName` enum. Tracked as a future spec slice. |
| `overflow-clip-transform-001.html` | _none_ | skipped | `transform` property not implemented. |
| `overflow-clip-x-visible-y-svg.html`, `overflow-clip-y-visible-x-svg.html`, `overflow-clip-margin-svg.html` | _none_ | skipped | SVG out of scope. |
| `single-axis-overflow-clip-rtl.html`, `overflow-clip-clamps-and-ignores-scroll-offsets-vertical-rl.html` | _none_ | skipped | RTL / writing modes not implemented. |
| `overflow-clip-rounded-table.html` | _none_ | skipped | Table layout + border-radius interaction; table layout (CSS Tables 3) only partially supported. |
| `rounded-overflow-clip-visible.html`, `overflow-clip-margin-border-radius{,-002}.html` | _none_ | skipped | `border-radius` not implemented. |
| `overflow-hidden-resize-with-stacking-context-child.html` | _none_ | skipped | `resize` property + stacking-context interaction; `resize` not implemented. |
| `document-element-overflow-hidden-scroll.html` | _none_ | skipped | Document-element overflow propagation to viewport; engine has no concept of viewport-vs-`<html>` overflow distinction yet. |

### shadow-dom/

| WPT path | Paws Rust test | Status | Reason / notes |
| --- | --- | --- | --- |
| _none yet_ | | | |

_Add new sections above this line as additional WPT slices come into scope._

## Workflow when translating a test

1. Read the upstream test at `wpt-reference/<spec-path>/<test>.html`.
2. Author a Rust test under `paws-wpt/tests/` (crate to be created in the WPT runner work). The test should:
   - Mount the equivalent fixture as a Yew component using `html! { ... }`.
   - Execute through the Paws engine via `paws-runner` (or whatever harness `paws-wpt` exposes).
   - Assert on engine state with `paws_wpt::testharness::*` — the Rust analog of `assert_equals`, `assert_throws_dom`, etc.
3. Add or update the row in the relevant per-spec table above. Adjust the status summary counts.
4. Include the updated **status summary** table and the **changed rows** in your PR description (see `agents.md`).
5. If you're skipping rather than translating, fill in the `Reason / notes` column with the missing engine capability — that line becomes a TODO for whoever picks up that capability later.
