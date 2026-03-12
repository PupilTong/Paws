/*!
Layout Module Architecture & Socratic Reasoning

1. 评估这样实现的后果 (Consequences):
   - **Performance**: We delegate block and flex/grid layout to Taffy, which is highly optimized. We maintain our own shadow tree mapping DOM nodes to Taffy `NodeId`s.
   - **Memory/Leaks**: The Taffy tree mimics the DOM tree. Managing lifecycle synchronization (insert/remove) requires care to avoid orphaned Taffy nodes.
   - **Concurrency**: Layout computations are currently single-threaded in Taffy. We must ensure style resolution (which is multi-threaded) completes before layout begins.

2. 总结和解释当前的思路 (Summary & Rationality):
   - Layout is strictly decoupled from the DOM. `Taffy` computes box layouts.
   - Text layout is a placeholder that will call out to OS capabilities (via foreign function interface or traits). We avoid `parley` to save binary size and use native text rendering.
   - The separation allows us to selectively re-layout dirty nodes rather than the whole tree, though initial implementation will be an eager top-down build.

3. 列出假设 (Assumptions):
   - **First Render**: Building the Taffy tree from the DOM tree maps 1:1 for elements. Text nodes return leaf Taffy nodes with specific measurements.
   - **Updates**: Changing a DOM node's style dirties its Taffy node. We assume Taffy's internal cache handles incremental re-layout efficiently.
   - **OS Text interface assumption**: OS can measure text synchronously or we pre-measure during the rendering pipeline.

4. 对blitz的审视 (Blitz Review):
   - **Simplification**: Blitz integrates `parley` intimately with layout. By removing it, we simplify text measuring to a trait/interface (`TextMeasurer`), mocking it for now.
   - **Improvement**: Clean split between `layout::block` (Taffy) and `layout::text` (OS integration).
*/

pub(crate) mod block;
pub(crate) mod text;

pub use block::{LayoutBox, LayoutState};
pub use text::{MockTextMeasurer, TextMeasurer};
