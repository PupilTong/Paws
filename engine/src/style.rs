use selectors::matching::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
};
use std::borrow::Cow;
use style as stylo;
use stylo::applicable_declarations::ApplicableDeclarationBlock;
use stylo::computed_value_flags::ComputedValueFlags;
use stylo::dom::TElement;
use stylo::font_metrics::FontMetrics;
use stylo::media_queries::{Device, MediaType};
use stylo::parser::ParserContext;
use stylo::properties::cascade::{apply_declarations, CascadeMode, FirstLineReparenting};
use stylo::properties::style_structs::Font;
use stylo::properties::{ComputedValues, LonghandId, PropertyId};
use stylo::queries::values::PrefersColorScheme;
use stylo::rule_cache::RuleCacheConditions;
use stylo::rule_tree::{CascadeLevel, RuleTree};
use stylo::servo_arc::Arc;
use stylo::shared_lock::{SharedRwLock, StylesheetGuards};
use stylo::stylesheets::layer_rule::LayerOrder;
use stylo::stylesheets::{CssRuleType, Namespaces, Origin, UrlExtraData};
use stylo::stylist::{RuleInclusion, Stylist};
use stylo::values::computed::font::GenericFontFamily;
use stylo::values::computed::CSSPixelLength;
use stylo::values::specified::font::{QueryFontMetricsFlags, FONT_MEDIUM_PX};
use stylo::values::specified::position::PositionTryFallbacksTryTactic;
use stylo_traits::{CSSPixel, CssStringWriter, CssWriter, DevicePixel, ParsingMode, ToCss};
use url::Url;

use crate::dom::{Element, ElementHandle};

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

pub fn update_inline_style(lock: &SharedRwLock, element: &mut Element, name: &str, value: &str) {
    let url = Url::parse("about:blank").expect("valid url");
    let url_data = UrlExtraData::from(url);

    // Serialize existing styles to preserve them (append behavior)
    // Read pass
    let existing_css = if let Some(ref block) = element.style_attribute {
        let guard = lock.read();
        let borrowed_block = block.read_with(&guard); // Try without explicit deref first? No, error said method not found.
                                                      // Maybe block is &Arc. Arc derefs to T?
                                                      // servo_arc::Arc derefs to T.
                                                      // Check if I need to import trait?
                                                      // use stylo::shared_lock::Locked;
                                                      // read_with is on Locked.
                                                      // I'll try explicit deref.
        let mut s = String::new();
        // PropertyDeclarationBlock::to_css takes &mut String (CssStringWriter)
        let _ = borrowed_block.to_css(&mut s);
        s
    } else {
        String::new()
    };

    // Write pass
    let _guard = lock.write();
    // Ensure separator to handle cases where to_css omits trailing semicolon or for safety
    let css = if existing_css.is_empty() {
        format!("{}: {}", name, value)
    } else {
        format!("{}; {}: {}", existing_css, name, value)
    };

    // Use write_with to access mutable reference (requires write guard)
    let new_block = {
        // Parse property into the new block
        // Using parse_style_attribute which takes context, declarations, and input string.
        stylo::properties::parse_style_attribute(
            &css,
            &url_data,
            None,
            QuirksMode::NoQuirks,
            stylo::stylesheets::CssRuleType::Style,
        )
    };
    // Update the Arc
    element.style_attribute = Some(Arc::new(lock.wrap(new_block)));
}

fn compute_style_for_handle(handle: ElementHandle) -> Arc<ComputedValues> {
    // Access RuntimeState via TLS context (set by caller) to get the lock.
    crate::dom::CONTEXT.with(|c| {
        let state_opt = c.borrow();
        let state = state_opt
            .as_ref()
            .expect("TLS not set for style computation");
        let lock = &state.style_context.lock;
        let style_context = &state.style_context;
        let guard = lock.read();
        let guards = StylesheetGuards::same(&guard);

        // Cache conditions need to be tracked
        // We use a temporary bloom filter and cache for this element (not efficient but simple)
        let mut bloom_filter = selectors::bloom::BloomFilter::new();
        let mut selector_caches = selectors::matching::SelectorCaches::default();

        let mut matching_context = MatchingContext::new(
            MatchingMode::Normal,
            Some(&mut bloom_filter),
            &mut selector_caches,
            QuirksMode::NoQuirks,
            NeedsSelectorFlags::No,
            MatchingForInvalidation::No,
        );

        let animations = Default::default();
        let mut match_results = smallvec::SmallVec::new();

        // push_applicable_declarations args
        style_context
            .stylist
            .push_applicable_declarations::<ElementHandle>(
                handle,
                None,
                <ElementHandle as TElement>::style_attribute(&handle),
                None,
                animations,
                RuleInclusion::All,
                &mut match_results,
                &mut matching_context,
            );

        println!("Match results before manual push: {}", match_results.len());
        // Manual push of inline style (redundancy check for now)
        let attribute = handle.style_attribute();
        if let Some(borrow) = attribute {
            let block = borrow.clone_arc();
            match_results.push(ApplicableDeclarationBlock::from_declarations(
                block,
                CascadeLevel::same_tree_author_normal(),
                LayerOrder::style_attribute(),
            ));
        }

        let rule_node = style_context.rule_tree.insert_ordered_rules_with_important(
            match_results
                .into_iter()
                .map(|block| (block.source.clone(), block.cascade_priority)),
            &guards,
        );

        let mut conditions = RuleCacheConditions::default();

        let computed_values = apply_declarations::<ElementHandle, _>(
            &style_context.stylist,
            None, // Pseudo
            &rule_node,
            &guards,
            std::iter::empty(), // iter_declarations (using rule_node)
            None,               // presentational_hints
            None,               // parent_computed_values
            FirstLineReparenting::No,
            &PositionTryFallbacksTryTactic::default(),
            CascadeMode::Unvisited {
                visited_rules: None,
            },
            ComputedValueFlags::empty(),
            None, // initial_computed_values
            &mut conditions,
            None, // important_declarations
        );
        computed_values
    })
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

pub fn computed_style(element: ElementHandle, property: &str) -> Option<String> {
    let url = Url::parse("about:blank").ok()?;
    let url_data = UrlExtraData::from(url);
    let parser_context = build_parser_context(&url_data);
    let property_id = PropertyId::parse(property, &parser_context).ok()?;
    let longhand = property_id.longhand_id()?;

    let computed = compute_style_for_handle(element);
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

    pub fn add_stylesheet(&mut self, css: &str) {
        let url = Url::parse("about:blank").expect("valid url");
        let url_data = UrlExtraData::from(url);
        let _context = build_parser_context(&url_data);
        // Stylesheet::from_str args:
        // css, url_data, origin, media, shared_lock, stylesheet_loader, error_reporter, quirks_mode, allow_import_rules
        // 9 arguments expected.

        let stylesheet = stylo::stylesheets::Stylesheet::from_str(
            css,
            url_data,
            stylo::stylesheets::Origin::Author,
            Arc::new(self.lock.wrap(stylo::media_queries::MediaList::empty())),
            self.lock.clone(),
            None, // stylesheet_loader
            None, // error_reporter
            QuirksMode::NoQuirks,
            stylo::stylesheets::AllowImportRules::Yes,
        );

        // DocumentStyleSheet might be a tuple struct or simple constructor
        // If DocumentStyleSheet::new didn't work, try struct literal if public?
        // Or From implementation.
        // Usually: stylo::stylesheets::DocumentStyleSheet(Arc::new(stylesheet))
        let document_stylesheet = stylo::stylesheets::DocumentStyleSheet(Arc::new(stylesheet));

        let guard = self.lock.read();
        self.stylist.append_stylesheet(document_stylesheet, &guard);
    }
}

impl Default for StyleContext {
    fn default() -> Self {
        Self::new()
    }
}
