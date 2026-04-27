//! Feature-gated style-resolution profiling counters.
//!
//! The `profiling` feature intentionally favors observability over
//! perfectly unperturbed timings. In particular, `Instant::now()` calls still
//! add measurable overhead, so profile-on numbers should be compared against
//! other profile-on runs, not directly against the default build.

#[cfg(feature = "profiling")]
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Snapshot of aggregated style-resolution timings.
///
/// Populate this via [`crate::RuntimeState::style_profiling_snapshot`]. The
/// counters stay zero unless the crate is built with the `profiling`
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

#[cfg(feature = "profiling")]
#[derive(Default)]
pub(crate) struct StyleProfiler {
    resolve_passes: AtomicU64,
    element_nodes_styled: AtomicU64,
    selector_matching_ns: AtomicU64,
    rule_tree_insertion_ns: AtomicU64,
    cascade_ns: AtomicU64,
    total_resolve_ns: AtomicU64,
}

#[cfg(not(feature = "profiling"))]
#[derive(Default)]
pub(crate) struct StyleProfiler {
    _private: (),
}

#[cfg(not(feature = "profiling"))]
#[derive(Clone, Copy, Default)]
pub(crate) struct DisabledTimer {
    _private: (),
}

#[cfg(feature = "profiling")]
impl StyleProfiler {
    pub(crate) fn reset(&self) {
        self.resolve_passes.store(0, Ordering::Relaxed);
        self.element_nodes_styled.store(0, Ordering::Relaxed);
        self.selector_matching_ns.store(0, Ordering::Relaxed);
        self.rule_tree_insertion_ns.store(0, Ordering::Relaxed);
        self.cascade_ns.store(0, Ordering::Relaxed);
        self.total_resolve_ns.store(0, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> StyleProfilingSnapshot {
        StyleProfilingSnapshot {
            enabled: true,
            resolve_passes: self.resolve_passes.load(Ordering::Relaxed),
            element_nodes_styled: self.element_nodes_styled.load(Ordering::Relaxed),
            selector_matching_ns: self.selector_matching_ns.load(Ordering::Relaxed),
            rule_tree_insertion_ns: self.rule_tree_insertion_ns.load(Ordering::Relaxed),
            cascade_ns: self.cascade_ns.load(Ordering::Relaxed),
            total_resolve_ns: self.total_resolve_ns.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn record_element_node(
        &self,
        selector_matching: Duration,
        rule_tree_insertion: Duration,
        cascade: Duration,
    ) {
        self.element_nodes_styled.fetch_add(1, Ordering::Relaxed);
        self.selector_matching_ns
            .fetch_add(duration_ns(selector_matching), Ordering::Relaxed);
        self.rule_tree_insertion_ns
            .fetch_add(duration_ns(rule_tree_insertion), Ordering::Relaxed);
        self.cascade_ns
            .fetch_add(duration_ns(cascade), Ordering::Relaxed);
    }

    pub(crate) fn record_resolve_pass(&self, total_resolve: Duration) {
        self.resolve_passes.fetch_add(1, Ordering::Relaxed);
        self.total_resolve_ns
            .fetch_add(duration_ns(total_resolve), Ordering::Relaxed);
    }
}

#[cfg(not(feature = "profiling"))]
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

#[cfg(feature = "profiling")]
pub(crate) fn start_timer() -> std::time::Instant {
    std::time::Instant::now()
}

#[cfg(not(feature = "profiling"))]
pub(crate) fn start_timer() -> DisabledTimer {
    DisabledTimer::default()
}

#[cfg(feature = "profiling")]
pub(crate) fn elapsed(started_at: std::time::Instant) -> Duration {
    started_at.elapsed()
}

#[cfg(not(feature = "profiling"))]
pub(crate) fn elapsed(_started_at: DisabledTimer) -> Duration {
    Duration::ZERO
}

#[cfg(feature = "profiling")]
fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}
