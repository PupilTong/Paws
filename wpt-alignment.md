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
| Translated (Yew flavor passing) | 1 |
| In progress | 0 |
| Skipped (with reason) | 0 |
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
