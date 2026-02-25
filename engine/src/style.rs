use selectors::matching::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
};
use std::borrow::Cow;
use style as stylo;
use stylo::computed_value_flags::ComputedValueFlags;
use stylo::dom::TElement;
use stylo::font_metrics::FontMetrics;
use stylo::media_queries::{Device, MediaType};
use stylo::parser::ParserContext;
use stylo::properties::cascade::FirstLineReparenting;
use stylo::properties::style_structs::Font;
use stylo::properties::{ComputedValues, LonghandId, PropertyId};
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
use stylo_traits::{CSSPixel, CssStringWriter, CssWriter, DevicePixel, ParsingMode, ToCss};
use url::Url;

use crate::dom::PawsElement;

pub mod css_style_sheet;
pub mod dom;
pub mod sheet_cache;

pub use css_style_sheet::CSSStyleSheet;
pub use sheet_cache::StylesheetCache;

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

fn build_parser_context<'a>(url_data: &'a UrlExtraData) -> ParserContext<'a> {
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

pub fn update_inline_style(
    lock: &SharedRwLock,
    element: &mut PawsElement,
    name: &str,
    value: &str,
) {
    let url = Url::parse("about:blank").expect("valid url");
    let url_data = UrlExtraData::from(url);

    // Serialize existing styles to preserve them (append behavior)
    // Read pass
    let existing_css = if let Some(ref block) = element.style_attribute {
        let guard = lock.read();
        let borrowed_block = block.read_with(&guard);
        let mut s = String::new();
        let _ = borrowed_block.to_css(&mut s);
        s
    } else {
        String::new()
    };

    // Write pass
    let _guard = lock.write();
    let css = if existing_css.is_empty() {
        format!("{}: {}", name, value)
    } else {
        let trimmed = existing_css.trim_end_matches(';');
        format!("{}; {}: {}", trimmed, name, value)
    };

    let new_block = {
        println!("Parsing style attribute CSS: {}", css);
        stylo::properties::parse_style_attribute(
            &css,
            &url_data,
            None,
            QuirksMode::NoQuirks,
            stylo::stylesheets::CssRuleType::Style,
        )
    };
    element.style_attribute = Some(Arc::new(lock.wrap(new_block)));
}

fn compute_style_for_node(
    state: &crate::runtime::RuntimeState,
    node: &PawsElement,
) -> Arc<ComputedValues> {
    let lock = &state.style_context.lock;
    let style_context = &state.style_context;
    let guard = lock.read();
    let guards = StylesheetGuards::same(&guard);
    let default_parent = ComputedValues::initial_values_with_font_override(Font::initial_values());

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

    println!("Match results before manual push: {}", match_results.len());

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
        Some(&default_parent), // parent_style
        None,                  // layout_parent_style
        FirstLineReparenting::No,
        &PositionTryFallbacksTryTactic::default(),
        None, // visited_rules
        ComputedValueFlags::empty(),
        None, // rule_cache
        &mut conditions,
        None, // element
    )
}

fn serialize_computed_value(style: &ComputedValues, longhand: LonghandId) -> Option<String> {
    let mut output = CssStringWriter::new();
    {
        let mut writer = CssWriter::new(&mut output);
        match longhand {
            LonghandId::Height => style.clone_height().to_css(&mut writer).ok()?,
            LonghandId::Width => style.clone_width().to_css(&mut writer).ok()?,
            LonghandId::Display => style.clone_display().to_css(&mut writer).ok()?,
            LonghandId::Color => style.clone_color().to_css(&mut writer).ok()?,
            LonghandId::BackgroundColor => {
                style.clone_background_color().to_css(&mut writer).ok()?
            }
            _ => return None,
        }
    }
    Some(output)
}

pub fn computed_style(
    state: &crate::runtime::RuntimeState,
    node_id: usize,
    property: &str,
) -> Option<String> {
    let url = Url::parse("about:blank").ok()?;
    let url_data = UrlExtraData::from(url);
    let parser_context = build_parser_context(&url_data);
    let property_id = PropertyId::parse(property, &parser_context).ok()?;
    let longhand = property_id.longhand_id()?;

    let node = state.doc.get_node(node_id)?;
    let computed = compute_style_for_node(state, node);
    serialize_computed_value(&computed, longhand)
}

// Public struct to hold persistent Stylo context
pub struct StyleContext {
    pub stylist: Stylist,
    pub rule_tree: RuleTree,
    pub lock: SharedRwLock,
}

impl StyleContext {
    pub fn new() -> Self {
        let lock = SharedRwLock::new();
        // Stylist needs the lock?
        // Stylist::new doesn't take lock, it takes device.
        let device = build_device();
        let stylist = Stylist::new(device, QuirksMode::NoQuirks);
        let rule_tree = RuleTree::new();
        Self {
            stylist,
            rule_tree,
            lock,
        }
    }

    pub fn add_stylesheet(&mut self, sheet: &CSSStyleSheet) {
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

impl Default for StyleContext {
    fn default() -> Self {
        Self::new()
    }
}
