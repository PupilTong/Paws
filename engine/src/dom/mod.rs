/*!
DOM Module Architecture & Socratic Reasoning

1. 评估这样实现的后果 (Consequences):
   - **Performance**: Using a `Slab` arena for the DOM tree guarantees memory contiguity and cache-friendly access, avoiding `Rc`/`RefCell` overhead and memory fragmentation.
   - **Memory Leaks**: Arena allocation prevents circular reference leaks common in DOM implementations.
   - **Race Conditions & Multi-threading**: The DOM tree itself is heavily mutated by the main (UI) thread. Stylo's style resolution runs in parallel. Thus, `PawsElement` internal Stylo data uses `AtomicRefCell` and shared locks. The structure safely supports parallel reads via `SharedRwLock`.
   - **Redundancy**: No HTML string parsing redundancy like Blitz, saving overhead. We only build the tree programmatically.

2. 总结和解释当前的思路 (Summary & Rationality):
   - We maintain a flattened DOM tree using indices (`usize`) instead of pointers.
   - A modern API wrapper exposes only the subset of DOM APIs needed by our constraints (e.g., `getComputedStyleMap`, node appending/removal).
   - The tree acts as the single source of truth for both JS/WASM bindings and the Layout engine.

3. 列出假设 (Assumptions):
   - **First Render**: Tree is built rapidly in memory; Stylo does a clean pass. `Slab` allocation is fast.
   - **Updates**: Incremental updates rely on dirty flags. Re-using `Slab` slots keeps memory stable.
   - We assume all text layout is deferred to the OS, so the DOM text nodes only hold strings, not font metrics.

4. 对blitz的审视 (Blitz Review):
   - **Simplification**: Blitz includes full HTML5 parsing and legacy CSS handling. We strip this out, focusing strictly on programmatically generated nodes, cutting down on parser complexity and memory footprint.
   - **Improvement**: We enforce stricter bounds on DOM APIs, rejecting deprecated methods early, keeping the API surface minimal and robust.
*/

pub mod api;
pub mod document;
pub mod element;

pub use document::{Document, DomError};
pub use element::{NodeFlags, NodeType, PawsElement};
