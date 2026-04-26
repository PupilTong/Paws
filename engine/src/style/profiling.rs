use std::time::Duration;

/// Snapshot of aggregated style-resolution timings.
///
/// Populate this via [`crate::RuntimeState::style_profiling_snapshot`]. The
/// counters stay zero unless the crate is built with the `style-profiling`
/// feature enabled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StyleProfilingSnapshot {
    /// `true` when timing counters are compiled in.
    pub enabled: bool,
    /// Number of full `Document::resolve_style` passes observed.
    pub resolve_passes: u64,
    /// Number of element nodes that went through selector matching.
    pub element_nodes_styled: u64,
    /// Time spent in Stylo selector matching.
    pub selector_matching_ns: u64,
    /// Time spent building / inserting rule-tree nodes.
    pub rule_tree_insertion_ns: u64,
    /// Time spent in Stylo cascade.
    pub cascade_ns: u64,
    /// Wall-clock time spent inside `Document::resolve_style`.
    pub total_resolve_ns: u64,
}

#[cfg(feature = "style-profiling")]
#[derive(Default)]
pub(crate) struct StyleProfiler {
    totals: std::sync::Mutex<StyleProfilingTotals>,
}

#[cfg(feature = "style-profiling")]
#[derive(Debug, Clone, Copy, Default)]
struct StyleProfilingTotals {
    resolve_passes: u64,
    element_nodes_styled: u64,
    selector_matching_ns: u64,
    rule_tree_insertion_ns: u64,
    cascade_ns: u64,
    total_resolve_ns: u64,
}

#[cfg(not(feature = "style-profiling"))]
#[derive(Default)]
pub(crate) struct StyleProfiler {
    _private: (),
}

#[cfg(not(feature = "style-profiling"))]
#[derive(Clone, Copy, Default)]
pub(crate) struct DisabledTimer {
    _private: (),
}

#[cfg(feature = "style-profiling")]
impl StyleProfiler {
    pub(crate) fn reset(&self) {
        *self.totals.lock().expect("style profiling mutex poisoned") =
            StyleProfilingTotals::default();
    }

    pub(crate) fn snapshot(&self) -> StyleProfilingSnapshot {
        let totals = *self.totals.lock().expect("style profiling mutex poisoned");
        StyleProfilingSnapshot {
            enabled: true,
            resolve_passes: totals.resolve_passes,
            element_nodes_styled: totals.element_nodes_styled,
            selector_matching_ns: totals.selector_matching_ns,
            rule_tree_insertion_ns: totals.rule_tree_insertion_ns,
            cascade_ns: totals.cascade_ns,
            total_resolve_ns: totals.total_resolve_ns,
        }
    }

    pub(crate) fn record_element_node(
        &self,
        selector_matching: Duration,
        rule_tree_insertion: Duration,
        cascade: Duration,
    ) {
        let mut totals = self.totals.lock().expect("style profiling mutex poisoned");
        totals.element_nodes_styled = totals.element_nodes_styled.saturating_add(1);
        totals.selector_matching_ns = totals
            .selector_matching_ns
            .saturating_add(duration_ns(selector_matching));
        totals.rule_tree_insertion_ns = totals
            .rule_tree_insertion_ns
            .saturating_add(duration_ns(rule_tree_insertion));
        totals.cascade_ns = totals.cascade_ns.saturating_add(duration_ns(cascade));
    }

    pub(crate) fn record_resolve_pass(&self, total_resolve: Duration) {
        let mut totals = self.totals.lock().expect("style profiling mutex poisoned");
        totals.resolve_passes = totals.resolve_passes.saturating_add(1);
        totals.total_resolve_ns = totals
            .total_resolve_ns
            .saturating_add(duration_ns(total_resolve));
    }
}

#[cfg(not(feature = "style-profiling"))]
impl StyleProfiler {
    pub(crate) fn reset(&self) {}

    pub(crate) fn snapshot(&self) -> StyleProfilingSnapshot {
        StyleProfilingSnapshot::default()
    }

    pub(crate) fn record_element_node(
        &self,
        _selector_matching: Duration,
        _rule_tree_insertion: Duration,
        _cascade: Duration,
    ) {
    }

    pub(crate) fn record_resolve_pass(&self, _total_resolve: Duration) {}
}

#[cfg(feature = "style-profiling")]
pub(crate) fn start_timer() -> std::time::Instant {
    std::time::Instant::now()
}

#[cfg(not(feature = "style-profiling"))]
pub(crate) fn start_timer() -> DisabledTimer {
    DisabledTimer::default()
}

#[cfg(feature = "style-profiling")]
pub(crate) fn elapsed(started_at: std::time::Instant) -> Duration {
    started_at.elapsed()
}

#[cfg(not(feature = "style-profiling"))]
pub(crate) fn elapsed(_started_at: DisabledTimer) -> Duration {
    Duration::ZERO
}

#[cfg(feature = "style-profiling")]
fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}
