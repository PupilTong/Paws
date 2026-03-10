use selectors::matching::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
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
use url::Url;

use crate::dom::PawsElement;

pub(crate) mod convert;
pub(crate) mod css_style_sheet;
pub(crate) mod dom;
pub(crate) mod sheet_cache;
pub mod typed_om;

pub(crate) use convert::to_taffy_style;
pub(crate) use css_style_sheet::CSSStyleSheet;
pub(crate) use sheet_cache::StylesheetCache;

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

fn build_device() -> Device {
    let default_values = ComputedValues::initial_values_with_font_override(Font::initial_values());
    let viewport = euclid::Size2D::<f32, CSSPixel>::new(800.0, 600.0);
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

/// Applies a single inline style declaration to an element.
///
/// Parses only the new property value (not the entire block) and merges it
/// into the existing `PropertyDeclarationBlock`, avoiding the overhead of
/// serializing and re-parsing the full style attribute.
pub(crate) fn update_inline_style(
    context: &StyleContext,
    element: &mut PawsElement,
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

pub(crate) fn compute_style_for_node(
    _doc: &crate::dom::Document,
    style_context: &StyleContext,
    node: &PawsElement,
    parent_style: Option<&ComputedValues>,
) -> Arc<ComputedValues> {
    let lock = &style_context.lock;
    let guard = lock.read();
    let guards = StylesheetGuards::same(&guard);
    let default_parent = ComputedValues::initial_values_with_font_override(Font::initial_values());
    let effective_parent = parent_style.unwrap_or(&default_parent);

    // Cache conditions need to be tracked
    let bloom_filter = selectors::bloom::BloomFilter::new();
    let mut selector_caches = selectors::matching::SelectorCaches::default();

    let mut matching_context = MatchingContext::new(
        MatchingMode::Normal,
        Some(&bloom_filter),
        &mut selector_caches,
        QuirksMode::NoQuirks,
        NeedsSelectorFlags::No,
        MatchingForInvalidation::No,
    );

    let animations = Default::default();
    let mut match_results = smallvec::SmallVec::new();

    // push_applicable_declarations args using &PawsElement
    style_context
        .stylist
        .push_applicable_declarations::<&PawsElement>(
            node,
            None,
            <&PawsElement as TElement>::style_attribute(&node),
            None,
            animations,
            RuleInclusion::All,
            &mut match_results,
            &mut matching_context,
        );

    let rule_node = style_context.rule_tree.insert_ordered_rules_with_important(
        match_results
            .into_iter()
            .map(|block| (block.source.clone(), block.cascade_priority)),
        &guards,
    );

    let mut conditions = RuleCacheConditions::default();

    stylo::properties::cascade::cascade::<&PawsElement>(
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
    )
}

/// Holds the Stylo styling engine state: the `Stylist`, rule tree, and shared lock.
pub struct StyleContext {
    pub(crate) stylist: Stylist,
    pub(crate) rule_tree: RuleTree,
    pub(crate) lock: SharedRwLock,
    #[allow(dead_code)]
    pub(crate) url: Url,
    pub(crate) url_data: UrlExtraData,
}

impl StyleContext {
    /// Creates a new style context with default device settings (800x600 viewport).
    pub(crate) fn new(url: url::Url) -> Self {
        let lock = SharedRwLock::new();
        let device = build_device();
        let stylist = Stylist::new(device, QuirksMode::NoQuirks);
        let rule_tree = RuleTree::new();

        // Initialize URL singletons
        let url_data = UrlExtraData::from(url.clone());

        Self {
            stylist,
            rule_tree,
            lock,
            url,
            url_data,
        }
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
        self.stylist
            .flush(&guards, None::<&crate::dom::PawsElement>, None);
    }
}
