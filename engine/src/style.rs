use crate::runtime::RenderState;
use selectors::bloom::{BloomFilter, BLOOM_HASH_MASK};
use selectors::matching::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
    SelectorCaches,
};
use std::borrow::Cow;
// The `stylo` crate publishes as `stylo` on crates.io but exposes `style` as its crate name.
use style as stylo;
use stylo::computed_value_flags::ComputedValueFlags;
use stylo::dom::TElement;
use stylo::font_metrics::FontMetrics;
use stylo::media_queries::{Device, MediaType};
use stylo::parser::ParserContext;
use stylo::properties::cascade::FirstLineReparenting;
use stylo::properties::style_structs::Font;
pub use stylo::properties::{ComputedValues, PropertyId};
use stylo::queries::values::PrefersColorScheme;
use stylo::rule_cache::RuleCacheConditions;
use stylo::rule_tree::RuleTree;
use stylo::servo_arc::Arc;
use stylo::shared_lock::{SharedRwLock, StylesheetGuards};
use stylo::stylesheets::{CssRuleType, Namespaces, Origin, UrlExtraData};
use stylo::stylist::{RuleInclusion, Stylist};
use stylo::values::computed::font::GenericFontFamily;
use stylo::values::specified::font::FONT_MEDIUM_PX;
use stylo::values::specified::position::PositionTryFallbacksTryTactic;
use stylo_traits::{CSSPixel, DevicePixel, ParsingMode};
use taffy::{AvailableSpace, Size};
use url::Url;

use crate::dom::{Document, NodeType, PawsElement};

pub(crate) mod convert;
pub(crate) mod css_style_sheet;
pub(crate) mod dom;
pub(crate) mod ir_convert;
pub(crate) mod profiling;
pub(crate) mod sheet_cache;
pub mod typed_om;

pub(crate) use convert::to_taffy_style;
pub(crate) use css_style_sheet::CSSStyleSheet;
pub use profiling::StyleProfilingSnapshot;
pub(crate) use sheet_cache::StylesheetCache;

const DEFAULT_VIEWPORT_WIDTH: f32 = 800.0;
const DEFAULT_VIEWPORT_HEIGHT: f32 = 600.0;

#[derive(Default)]
pub(crate) struct StyleMatchingState {
    selector_caches: SelectorCaches,
    ancestor_filter: AncestorBloomFilter,
}

impl StyleMatchingState {
    pub(crate) fn prepare_for_node<S: RenderState>(
        &mut self,
        doc: &Document<S>,
        node_id: taffy::NodeId,
    ) {
        self.ancestor_filter.rebuild_for_node(doc, node_id);
    }
}

#[derive(Default)]
struct AncestorBloomFilter {
    filter: BloomFilter,
    elements: Vec<PushedAncestor>,
    pushed_hashes: Vec<u32>,
    ancestor_path: Vec<taffy::NodeId>,
}

struct PushedAncestor {
    id: taffy::NodeId,
    num_hashes: usize,
}

impl AncestorBloomFilter {
    fn filter(&self) -> &BloomFilter {
        &self.filter
    }

    fn rebuild_for_node<S: RenderState>(&mut self, doc: &Document<S>, node_id: taffy::NodeId) {
        self.ancestor_path.clear();
        let mut current = traversal_parent_id(doc, node_id);
        while let Some(parent_id) = current {
            self.ancestor_path.push(parent_id);
            current = traversal_parent_id(doc, parent_id);
        }
        self.ancestor_path.reverse();

        let common_len = self
            .elements
            .iter()
            .zip(self.ancestor_path.iter())
            .take_while(|(pushed, path_id)| pushed.id == **path_id)
            .count();

        while self.elements.len() > common_len {
            self.pop();
        }

        for index in common_len..self.ancestor_path.len() {
            let ancestor_id = self.ancestor_path[index];
            self.push(doc, ancestor_id);
        }
    }

    fn push<S: RenderState>(&mut self, doc: &Document<S>, node_id: taffy::NodeId) {
        let Some(node) = doc.get_node(node_id) else {
            return;
        };
        debug_assert!(node.is_element());

        let mut count = 0;
        stylo::bloom::each_relevant_element_hash(node, |hash| {
            count += 1;
            let hash = hash & BLOOM_HASH_MASK;
            self.filter.insert_hash(hash);
            self.pushed_hashes.push(hash);
        });
        self.elements.push(PushedAncestor {
            id: node_id,
            num_hashes: count,
        });
    }

    fn pop(&mut self) {
        let Some(pushed) = self.elements.pop() else {
            return;
        };

        for _ in 0..pushed.num_hashes {
            let hash = self
                .pushed_hashes
                .pop()
                .expect("ancestor bloom hash stack should match pushed elements");
            self.filter.remove_hash(hash);
        }
    }
}

fn traversal_parent_id<S: RenderState>(
    doc: &Document<S>,
    node_id: taffy::NodeId,
) -> Option<taffy::NodeId> {
    let node = doc.get_node(node_id)?;
    let parent_id = node.parent?;
    let parent = doc.get_node(parent_id)?;
    if parent.node_type == NodeType::ShadowRoot {
        let host_id = parent.parent?;
        doc.get_node(host_id)?.is_element().then_some(host_id)
    } else {
        parent.is_element().then_some(parent_id)
    }
}

#[derive(Debug, Default)]
struct SimpleFontMetricsProvider;

impl stylo::servo::media_queries::FontMetricsProvider for SimpleFontMetricsProvider {
    fn query_font_metrics(
        &self,
        _vertical: bool,
        _font: &Font,
        base_size: stylo::values::computed::CSSPixelLength,
        _flags: stylo::values::specified::font::QueryFontMetricsFlags,
    ) -> FontMetrics {
        FontMetrics {
            ascent: stylo::values::computed::Length::new(base_size.px()),
            ..FontMetrics::default()
        }
    }

    fn base_size_for_generic(
        &self,
        _generic: GenericFontFamily,
    ) -> stylo::values::computed::Length {
        stylo::values::computed::Length::new(FONT_MEDIUM_PX)
    }
}

pub(crate) fn build_parser_context<'a>(url_data: &'a UrlExtraData) -> ParserContext<'a> {
    ParserContext::new(
        Origin::Author,
        url_data,
        Some(CssRuleType::Style),
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        Cow::Owned(Namespaces::default()),
        None,
        None,
    )
}

fn build_device(viewport: euclid::Size2D<f32, CSSPixel>) -> Device {
    let default_values = ComputedValues::initial_values_with_font_override(Font::initial_values());
    let device_pixel_ratio = euclid::Scale::<f32, CSSPixel, DevicePixel>::new(1.0);
    Device::new(
        MediaType::screen(),
        QuirksMode::NoQuirks,
        viewport,
        device_pixel_ratio,
        Box::new(SimpleFontMetricsProvider),
        default_values,
        PrefersColorScheme::Light,
    )
}

/// Converts Taffy's available-space constraint to Stylo's concrete viewport,
/// falling back per axis when the layout constraint is not a host dimension.
fn css_viewport_from_taffy(viewport: Size<AvailableSpace>) -> euclid::Size2D<f32, CSSPixel> {
    fn resolve_axis(axis: AvailableSpace, fallback: f32) -> f32 {
        match axis {
            AvailableSpace::Definite(value) if value.is_finite() && value >= 0.0 => value,
            _ => fallback,
        }
    }

    euclid::Size2D::<f32, CSSPixel>::new(
        resolve_axis(viewport.width, DEFAULT_VIEWPORT_WIDTH),
        resolve_axis(viewport.height, DEFAULT_VIEWPORT_HEIGHT),
    )
}

/// Applies a single inline style declaration to an element.
///
/// Parses only the new property value (not the entire block) and merges it
/// into the existing `PropertyDeclarationBlock`, avoiding the overhead of
/// serializing and re-parsing the full style attribute.
pub(crate) fn update_inline_style<S: RenderState>(
    context: &StyleContext,
    element: &mut PawsElement<S>,
    name: &str,
    value: &str,
) {
    let lock = &context.lock;
    let url_data = &context.url_data;

    // Parse the property name
    let property_id = match PropertyId::parse(name, &build_parser_context(url_data)) {
        Ok(id) => id,
        Err(_) => return,
    };

    // Parse only the new declaration
    let mut source_declarations = stylo::properties::SourcePropertyDeclaration::default();
    if stylo::properties::parse_one_declaration_into(
        &mut source_declarations,
        property_id,
        value,
        Origin::Author,
        url_data,
        None,
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    )
    .is_err()
    {
        return;
    }

    // Clone existing block under a read guard
    let mut block = if let Some(ref existing) = element.style_attribute {
        let guard = lock.read();
        existing.read_with(&guard).clone()
    } else {
        stylo::properties::PropertyDeclarationBlock::new()
    };

    // Merge the new declaration and wrap under a write guard
    block.extend(
        source_declarations.drain(),
        stylo::properties::Importance::Normal,
    );
    element.style_attribute = Some(Arc::new(lock.wrap(block)));
}

pub(crate) fn compute_style_for_node<S: RenderState>(
    _doc: &crate::dom::Document<S>,
    style_context: &StyleContext,
    node: &PawsElement<S>,
    parent_style: Option<&ComputedValues>,
    matching_state: &mut StyleMatchingState,
) -> Arc<ComputedValues> {
    let lock = &style_context.lock;
    let guard = lock.read();
    let guards = StylesheetGuards::same(&guard);
    let default_parent = ComputedValues::initial_values_with_font_override(Font::initial_values());
    let effective_parent = parent_style.unwrap_or(&default_parent);

    let selector_matching_started = profiling::start_timer();

    let mut matching_context = MatchingContext::new(
        MatchingMode::Normal,
        Some(matching_state.ancestor_filter.filter()),
        &mut matching_state.selector_caches,
        QuirksMode::NoQuirks,
        NeedsSelectorFlags::No,
        MatchingForInvalidation::No,
    );

    let animations = Default::default();
    let mut match_results = smallvec::SmallVec::new();

    // push_applicable_declarations args using &PawsElement<S>
    style_context
        .stylist
        .push_applicable_declarations::<&PawsElement<S>>(
            node,
            None,
            <&PawsElement<S> as TElement>::style_attribute(&node),
            None,
            animations,
            RuleInclusion::All,
            &mut match_results,
            &mut matching_context,
        );
    let selector_matching = profiling::elapsed(selector_matching_started);

    let rule_tree_started = profiling::start_timer();
    let rule_node = style_context.rule_tree.insert_ordered_rules_with_important(
        match_results
            .into_iter()
            .map(|block| (block.source.clone(), block.cascade_priority)),
        &guards,
    );
    let rule_tree_insertion = profiling::elapsed(rule_tree_started);

    let mut conditions = RuleCacheConditions::default();
    let cascade_started = profiling::start_timer();

    let computed = stylo::properties::cascade::cascade::<&PawsElement>(
        &style_context.stylist,
        None, // Pseudo
        &rule_node,
        &guards,
        Some(effective_parent), // parent_style
        Some(effective_parent), // layout_parent_style
        FirstLineReparenting::No,
        &PositionTryFallbacksTryTactic::default(),
        None, // visited_rules
        ComputedValueFlags::empty(),
        None, // rule_cache
        &mut conditions,
        None, // element
    );
    let cascade = profiling::elapsed(cascade_started);
    style_context
        .profiler
        .record_element_node(selector_matching, rule_tree_insertion, cascade);
    computed
}

/// Holds the Stylo styling engine state: the `Stylist`, rule tree, and shared lock.
pub struct StyleContext {
    pub(crate) stylist: Stylist,
    pub(crate) rule_tree: RuleTree,
    pub(crate) lock: SharedRwLock,
    pub(crate) profiler: profiling::StyleProfiler,
    #[allow(dead_code)]
    pub(crate) url: Url,
    pub(crate) url_data: UrlExtraData,
}

impl StyleContext {
    /// Creates a new style context with default device settings (800x600 viewport).
    #[cfg(test)]
    pub(crate) fn new(url: url::Url) -> Self {
        Self::with_viewport(
            url,
            Size {
                width: AvailableSpace::MaxContent,
                height: AvailableSpace::MaxContent,
            },
        )
    }

    /// Creates a new style context with a Stylo device viewport matching the
    /// runtime layout viewport when that viewport is definite.
    pub(crate) fn with_viewport(url: url::Url, viewport: Size<AvailableSpace>) -> Self {
        let lock = SharedRwLock::new();
        let device = build_device(css_viewport_from_taffy(viewport));
        let stylist = Stylist::new(device, QuirksMode::NoQuirks);
        let rule_tree = RuleTree::new();

        // Initialize URL singletons
        let url_data = UrlExtraData::from(url.clone());

        Self {
            stylist,
            rule_tree,
            lock,
            profiler: profiling::StyleProfiler::default(),
            url,
            url_data,
        }
    }

    /// Updates Stylo's device viewport to match the runtime layout viewport.
    ///
    /// Returns `true` when the CSS viewport changed and computed styles must be
    /// refreshed. Non-definite Taffy axes keep the historical 800x600 Stylo
    /// fallback because there is no concrete host dimension to mirror.
    pub(crate) fn set_viewport(&mut self, viewport: Size<AvailableSpace>) -> bool {
        let css_viewport = css_viewport_from_taffy(viewport);
        if self.stylist.device().viewport_size() == css_viewport {
            return false;
        }

        let device = build_device(css_viewport);
        let guard = self.lock.read();
        let guards = StylesheetGuards::same(&guard);
        let changed_origins = self.stylist.set_device(device, &guards);
        if !changed_origins.is_empty() {
            self.stylist.force_stylesheet_origins_dirty(changed_origins);
            self.stylist.flush(&guards);
        }
        true
    }

    /// Appends a stylesheet to the stylist and flushes the cascade.
    pub(crate) fn add_stylesheet(&mut self, sheet: &CSSStyleSheet) {
        let document_stylesheet = stylo::stylesheets::DocumentStyleSheet(sheet.sheet.clone());
        let guard = self.lock.read();
        self.stylist.append_stylesheet(document_stylesheet, &guard);
        let guards = stylo::shared_lock::StylesheetGuards {
            author: &guard,
            ua_or_user: &guard,
        };
        self.stylist.flush(&guards);
    }

    pub(crate) fn reset_profiling(&self) {
        self.profiler.reset();
    }

    pub(crate) fn profiling_snapshot(&self) -> StyleProfilingSnapshot {
        self.profiler.snapshot()
    }
}
