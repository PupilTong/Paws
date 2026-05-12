//! Paws WPT conformance suite — Yew-flavor translations of the W3C
//! [Web Platform Tests](https://github.com/web-platform-tests/wpt).
//!
//! The repo-root `wpt-alignment.md` is the tracker for which tests are
//! translated, in progress, or skipped.
//!
//! ## Why translation, not execution
//!
//! Paws does not embed a JavaScript engine, so we cannot run upstream
//! WPT test files (which are JS-driven via `testharness.js`) verbatim.
//! Instead each in-scope test is hand-translated into:
//!
//! 1. A **Yew fixture** under `paws-wpt/fixtures/<spec-area>-<test-name>/`
//!    that mounts the equivalent setup as a Yew component — i.e. the
//!    same API surface a Paws developer writes against.
//! 2. A **host-side runner test** under `paws-wpt/tests/<spec-area>.rs`
//!    that loads the fixture's compiled `.wasm`, executes it through
//!    [`paws_runner`](../paws_runner/index.html), and asserts on the
//!    resulting engine state via the [`testharness`] vocabulary.
//!
//! This delivers the developer-experience promise: when you write Yew
//! on Paws, the engine behaves the way real-browser Yew would, as
//! defined by WPT.
//!
//! ## Adding a translated test
//!
//! See [`wpt-alignment.md`](https://github.com/anthropics/paws/blob/main/wpt-alignment.md)
//! for the workflow. Briefly:
//!
//! 1. Read the upstream test under `wpt-reference/<spec-path>/<test>.html`
//!    (the read-only WPT clone — gitignored).
//! 2. Add a `paws-wpt/fixtures/<spec-area>-<test-name>/` crate that mounts
//!    the equivalent fixture as a Yew component, and add its directory
//!    name to `FIXTURES` in `paws-wpt/build.rs`.
//! 3. Add a `#[test]` to the relevant `paws-wpt/tests/<spec-area>.rs` file
//!    that loads the fixture, runs it, and asserts via
//!    [`testharness`] helpers.
//! 4. Update `wpt-alignment.md` with the row + summary counts, and
//!    quote the summary table in the PR description (required by
//!    `agents.md`).

pub mod testharness;

include!(concat!(env!("OUT_DIR"), "/wpt_fixtures.rs"));
