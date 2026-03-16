//! Converts pre-parsed CSS IR (from `paws-style-ir`) into Stylo types.
//!
//! This module bridges the zero-copy [`ArchivedStyleSheetIR`] and Stylo's
//! [`CssRule`] tree.
//!
//! # Dispatch strategy
//!
//! For **typed** IR values (`PropertyValueIR::Size`, `Display`, etc.) the
//! conversion is an infallible enum-to-enum map — no string matching,
//! no runtime validation.
//!
//! For the **Raw** fallback (untyped / forward-compat tokens) the legacy
//! string-matching converters in [`helpers`], [`keyword`], and [`numeric`]
//! are used.
//!
//! # Sub-modules
//!
//! | Module         | Contents |
//! |----------------|----------|
//! | [`length`]     | IR → Stylo length/percentage primitives |
//! | [`helpers`]    | Typed IR converters + Raw fallback value helpers |
//! | [`keyword`]    | Typed keyword converters + Raw fallback |
//! | [`numeric`]    | Typed numeric converters + Raw fallback |

mod helpers;
mod keyword;
mod length;
mod numeric;

use ::style::properties::{Importance, PropertyDeclaration, PropertyDeclarationBlock};
use ::style::servo_arc::Arc;
use ::style::shared_lock::SharedRwLock;
use ::style::stylesheets::{CssRule, CssRules, StyleRule, UrlExtraData};
use paws_style_ir::{ArchivedCssPropertyName, ArchivedCssToken, ArchivedPropertyValueIR};

use helpers::{
    gap_ir_to_stylo, inset_ir_to_stylo, ir_to_border_style, ir_to_border_width, ir_to_gap,
    ir_to_inset, ir_to_margin, ir_to_max_size, ir_to_nn_lp, ir_to_size, margin_ir_to_stylo,
    max_size_ir_to_stylo, size_ir_to_stylo,
};
use keyword::{
    border_style_ir_to_stylo, box_sizing_ir_to_stylo, clear_ir_to_stylo, convert_box_sizing,
    convert_clear, convert_display, convert_flex_direction, convert_flex_wrap, convert_float,
    convert_object_fit, convert_overflow_x, convert_overflow_y, convert_position,
    convert_visibility, display_ir_to_stylo, flex_direction_ir_to_stylo, flex_wrap_ir_to_stylo,
    float_ir_to_stylo, object_fit_ir_to_stylo, overflow_ir_to_stylo, position_ir_to_stylo,
    visibility_ir_to_stylo,
};
use length::nn_lp_ir_to_stylo;
use numeric::{
    convert_flex_basis, convert_flex_grow, convert_flex_shrink, convert_order, convert_z_index,
    flex_basis_ir_to_stylo, nn_number_ir_to_stylo, z_index_ir_to_stylo,
};

// ─── Public API ──────────────────────────────────────────────────────

/// Converts a slice of archived CSS rules into Stylo `CssRule` values.
pub(crate) fn construct_stylo_rules(
    rules_ir: &rkyv::vec::ArchivedVec<paws_style_ir::ArchivedCssRuleIR>,
    lock: &SharedRwLock,
    url_data: &UrlExtraData,
    _context: &::style::parser::ParserContext,
) -> Vec<CssRule> {
    let mut stylo_rules = Vec::new();
    for rule_ir in rules_ir.iter() {
        match rule_ir {
            paws_style_ir::ArchivedCssRuleIR::Style(s) => {
                if let Some(rule) = convert_style_rule(s, lock, url_data) {
                    stylo_rules.push(rule);
                }
            }
            paws_style_ir::ArchivedCssRuleIR::AtRule(_) => {
                // At-rules not yet supported in the typed path
            }
        }
    }
    stylo_rules
}

// ─── Rule conversion ─────────────────────────────────────────────────

/// Converts a single archived style rule into a Stylo `CssRule::Style`.
fn convert_style_rule(
    s: &paws_style_ir::ArchivedStyleRuleIR,
    lock: &SharedRwLock,
    url_data: &UrlExtraData,
) -> Option<CssRule> {
    let sel_str = s.selectors.as_str();
    let selectors = ::style::selector_parser::SelectorParser::parse_author_origin_no_namespace(
        sel_str, url_data,
    )
    .ok()?;

    let mut block = PropertyDeclarationBlock::new();
    for decl in s.declarations.iter() {
        let importance = if decl.important {
            Importance::Important
        } else {
            Importance::Normal
        };
        if let Some(prop_decl) = convert_declaration(&decl.name, &decl.value) {
            block.push(prop_decl, importance);
        }
    }

    let nested_rules = if s.rules.is_empty() {
        None
    } else {
        let children = construct_stylo_rules(&s.rules, lock, url_data, &{
            ::style::parser::ParserContext::new(
                ::style::stylesheets::Origin::Author,
                url_data,
                Some(::style::stylesheets::CssRuleType::Style),
                ::stylo_traits::ParsingMode::DEFAULT,
                ::style::context::QuirksMode::NoQuirks,
                Default::default(),
                None,
                None,
            )
        });
        Some(Arc::new(lock.wrap(CssRules(children))))
    };

    let style_rule = StyleRule {
        selectors,
        block: Arc::new(lock.wrap(block)),
        rules: nested_rules,
        source_location: ::style::values::SourceLocation { line: 0, column: 0 },
    };
    Some(CssRule::Style(Arc::new(lock.wrap(style_rule))))
}

// ─── Property dispatch ───────────────────────────────────────────────

/// Converts a single IR property declaration to a Stylo `PropertyDeclaration`.
///
/// Typed IR values are converted via infallible enum maps.
/// Raw tokens fall through to the legacy string-matching path.
fn convert_declaration(
    name: &ArchivedCssPropertyName,
    value: &ArchivedPropertyValueIR,
) -> Option<PropertyDeclaration> {
    // ── Typed IR fast path ───────────────────────────────────────
    match value {
        ArchivedPropertyValueIR::Display(ref d) => {
            return Some(PropertyDeclaration::Display(display_ir_to_stylo(d)));
        }
        ArchivedPropertyValueIR::BoxSizing(ref bs) => {
            return Some(PropertyDeclaration::BoxSizing(box_sizing_ir_to_stylo(bs)));
        }
        ArchivedPropertyValueIR::Position(ref p) => {
            return Some(PropertyDeclaration::Position(position_ir_to_stylo(p)));
        }
        ArchivedPropertyValueIR::Float(ref f) => {
            return Some(PropertyDeclaration::Float(float_ir_to_stylo(f)));
        }
        ArchivedPropertyValueIR::Clear(ref c) => {
            return Some(PropertyDeclaration::Clear(clear_ir_to_stylo(c)));
        }
        ArchivedPropertyValueIR::Visibility(ref v) => {
            return Some(PropertyDeclaration::Visibility(visibility_ir_to_stylo(v)));
        }
        ArchivedPropertyValueIR::ObjectFit(ref of) => {
            return Some(PropertyDeclaration::ObjectFit(object_fit_ir_to_stylo(of)));
        }
        ArchivedPropertyValueIR::FlexDirection(ref fd) => {
            return Some(PropertyDeclaration::FlexDirection(
                flex_direction_ir_to_stylo(fd),
            ));
        }
        ArchivedPropertyValueIR::FlexWrap(ref fw) => {
            return Some(PropertyDeclaration::FlexWrap(flex_wrap_ir_to_stylo(fw)));
        }
        ArchivedPropertyValueIR::FlexGrow(ref n) => {
            return Some(PropertyDeclaration::FlexGrow(nn_number_ir_to_stylo(n)));
        }
        ArchivedPropertyValueIR::FlexShrink(ref n) => {
            return Some(PropertyDeclaration::FlexShrink(nn_number_ir_to_stylo(n)));
        }
        ArchivedPropertyValueIR::FlexBasis(ref fb) => {
            return Some(PropertyDeclaration::FlexBasis(Box::new(
                flex_basis_ir_to_stylo(fb),
            )));
        }
        ArchivedPropertyValueIR::Order(ref i) => {
            return Some(PropertyDeclaration::Order(numeric::integer_ir_to_stylo(i)));
        }
        ArchivedPropertyValueIR::ZIndex(ref z) => {
            return Some(PropertyDeclaration::ZIndex(z_index_ir_to_stylo(z)));
        }

        // Typed values that need property-name dispatch
        ArchivedPropertyValueIR::Size(ref s) => {
            let stylo_size = size_ir_to_stylo(s);
            return match name {
                ArchivedCssPropertyName::Width => Some(PropertyDeclaration::Width(stylo_size)),
                ArchivedCssPropertyName::Height => Some(PropertyDeclaration::Height(stylo_size)),
                ArchivedCssPropertyName::MinWidth => {
                    Some(PropertyDeclaration::MinWidth(stylo_size))
                }
                ArchivedCssPropertyName::MinHeight => {
                    Some(PropertyDeclaration::MinHeight(stylo_size))
                }
                _ => None,
            };
        }
        ArchivedPropertyValueIR::MaxSize(ref s) => {
            let stylo_max = max_size_ir_to_stylo(s);
            return match name {
                ArchivedCssPropertyName::MaxWidth => Some(PropertyDeclaration::MaxWidth(stylo_max)),
                ArchivedCssPropertyName::MaxHeight => {
                    Some(PropertyDeclaration::MaxHeight(stylo_max))
                }
                _ => None,
            };
        }
        ArchivedPropertyValueIR::Margin(ref m) => {
            let stylo_margin = margin_ir_to_stylo(m);
            return match name {
                ArchivedCssPropertyName::MarginTop => {
                    Some(PropertyDeclaration::MarginTop(stylo_margin))
                }
                ArchivedCssPropertyName::MarginRight => {
                    Some(PropertyDeclaration::MarginRight(stylo_margin))
                }
                ArchivedCssPropertyName::MarginBottom => {
                    Some(PropertyDeclaration::MarginBottom(stylo_margin))
                }
                ArchivedCssPropertyName::MarginLeft => {
                    Some(PropertyDeclaration::MarginLeft(stylo_margin))
                }
                _ => None,
            };
        }
        ArchivedPropertyValueIR::Padding(ref lp) => {
            let stylo_padding = nn_lp_ir_to_stylo(lp);
            return match name {
                ArchivedCssPropertyName::PaddingTop => {
                    Some(PropertyDeclaration::PaddingTop(stylo_padding))
                }
                ArchivedCssPropertyName::PaddingRight => {
                    Some(PropertyDeclaration::PaddingRight(stylo_padding))
                }
                ArchivedCssPropertyName::PaddingBottom => {
                    Some(PropertyDeclaration::PaddingBottom(stylo_padding))
                }
                ArchivedCssPropertyName::PaddingLeft => {
                    Some(PropertyDeclaration::PaddingLeft(stylo_padding))
                }
                _ => None,
            };
        }
        ArchivedPropertyValueIR::BorderStyle(ref bs) => {
            let stylo_bs = border_style_ir_to_stylo(bs);
            return match name {
                ArchivedCssPropertyName::BorderTopStyle => {
                    Some(PropertyDeclaration::BorderTopStyle(stylo_bs))
                }
                ArchivedCssPropertyName::BorderRightStyle => {
                    Some(PropertyDeclaration::BorderRightStyle(stylo_bs))
                }
                ArchivedCssPropertyName::BorderBottomStyle => {
                    Some(PropertyDeclaration::BorderBottomStyle(stylo_bs))
                }
                ArchivedCssPropertyName::BorderLeftStyle => {
                    Some(PropertyDeclaration::BorderLeftStyle(stylo_bs))
                }
                _ => None,
            };
        }
        ArchivedPropertyValueIR::Inset(ref i) => {
            let stylo_inset = inset_ir_to_stylo(i);
            return match name {
                ArchivedCssPropertyName::Top => Some(PropertyDeclaration::Top(stylo_inset)),
                ArchivedCssPropertyName::Right => Some(PropertyDeclaration::Right(stylo_inset)),
                ArchivedCssPropertyName::Bottom => Some(PropertyDeclaration::Bottom(stylo_inset)),
                ArchivedCssPropertyName::Left => Some(PropertyDeclaration::Left(stylo_inset)),
                _ => None,
            };
        }
        ArchivedPropertyValueIR::Overflow(ref o) => {
            let stylo_overflow = overflow_ir_to_stylo(o);
            return match name {
                ArchivedCssPropertyName::OverflowX => {
                    Some(PropertyDeclaration::OverflowX(stylo_overflow))
                }
                ArchivedCssPropertyName::OverflowY => {
                    Some(PropertyDeclaration::OverflowY(stylo_overflow))
                }
                _ => None,
            };
        }
        ArchivedPropertyValueIR::Gap(ref g) => {
            let stylo_gap = gap_ir_to_stylo(g);
            return match name {
                ArchivedCssPropertyName::ColumnGap => {
                    Some(PropertyDeclaration::ColumnGap(stylo_gap))
                }
                ArchivedCssPropertyName::RowGap => Some(PropertyDeclaration::RowGap(stylo_gap)),
                _ => None,
            };
        }

        // CSS-wide keywords are not yet handled in the typed path
        ArchivedPropertyValueIR::CssWide(_) => return None,

        // Raw fallback — dispatch below
        ArchivedPropertyValueIR::Raw(_) => {}
    }

    // ── Raw token fallback path ──────────────────────────────────
    let tokens = match value {
        ArchivedPropertyValueIR::Raw(ref t) => t.as_slice(),
        _ => return None,
    };

    convert_raw_declaration(name, tokens)
}

/// Legacy string-matching converter for Raw tokens.
fn convert_raw_declaration(
    name: &ArchivedCssPropertyName,
    value: &[ArchivedCssToken],
) -> Option<PropertyDeclaration> {
    match name {
        // ── Display & box model ──────────────────────────────────
        ArchivedCssPropertyName::Display => convert_display(value),
        ArchivedCssPropertyName::BoxSizing => convert_box_sizing(value),

        // ── Sizing ───────────────────────────────────────────────
        ArchivedCssPropertyName::Width => ir_to_size(value).map(PropertyDeclaration::Width),
        ArchivedCssPropertyName::Height => ir_to_size(value).map(PropertyDeclaration::Height),
        ArchivedCssPropertyName::MinWidth => ir_to_size(value).map(PropertyDeclaration::MinWidth),
        ArchivedCssPropertyName::MinHeight => ir_to_size(value).map(PropertyDeclaration::MinHeight),
        ArchivedCssPropertyName::MaxWidth => {
            ir_to_max_size(value).map(PropertyDeclaration::MaxWidth)
        }
        ArchivedCssPropertyName::MaxHeight => {
            ir_to_max_size(value).map(PropertyDeclaration::MaxHeight)
        }

        // ── Margin ───────────────────────────────────────────────
        ArchivedCssPropertyName::MarginTop => {
            ir_to_margin(value).map(PropertyDeclaration::MarginTop)
        }
        ArchivedCssPropertyName::MarginRight => {
            ir_to_margin(value).map(PropertyDeclaration::MarginRight)
        }
        ArchivedCssPropertyName::MarginBottom => {
            ir_to_margin(value).map(PropertyDeclaration::MarginBottom)
        }
        ArchivedCssPropertyName::MarginLeft => {
            ir_to_margin(value).map(PropertyDeclaration::MarginLeft)
        }

        // ── Padding ──────────────────────────────────────────────
        ArchivedCssPropertyName::PaddingTop => {
            ir_to_nn_lp(value).map(PropertyDeclaration::PaddingTop)
        }
        ArchivedCssPropertyName::PaddingRight => {
            ir_to_nn_lp(value).map(PropertyDeclaration::PaddingRight)
        }
        ArchivedCssPropertyName::PaddingBottom => {
            ir_to_nn_lp(value).map(PropertyDeclaration::PaddingBottom)
        }
        ArchivedCssPropertyName::PaddingLeft => {
            ir_to_nn_lp(value).map(PropertyDeclaration::PaddingLeft)
        }

        // ── Border width ─────────────────────────────────────────
        ArchivedCssPropertyName::BorderTopWidth => {
            ir_to_border_width(value).map(PropertyDeclaration::BorderTopWidth)
        }
        ArchivedCssPropertyName::BorderRightWidth => {
            ir_to_border_width(value).map(PropertyDeclaration::BorderRightWidth)
        }
        ArchivedCssPropertyName::BorderBottomWidth => {
            ir_to_border_width(value).map(PropertyDeclaration::BorderBottomWidth)
        }
        ArchivedCssPropertyName::BorderLeftWidth => {
            ir_to_border_width(value).map(PropertyDeclaration::BorderLeftWidth)
        }

        // ── Border style ─────────────────────────────────────────
        ArchivedCssPropertyName::BorderTopStyle => {
            ir_to_border_style(value).map(PropertyDeclaration::BorderTopStyle)
        }
        ArchivedCssPropertyName::BorderRightStyle => {
            ir_to_border_style(value).map(PropertyDeclaration::BorderRightStyle)
        }
        ArchivedCssPropertyName::BorderBottomStyle => {
            ir_to_border_style(value).map(PropertyDeclaration::BorderBottomStyle)
        }
        ArchivedCssPropertyName::BorderLeftStyle => {
            ir_to_border_style(value).map(PropertyDeclaration::BorderLeftStyle)
        }

        // ── Border color (not yet supported) ─────────────────────
        ArchivedCssPropertyName::BorderTopColor
        | ArchivedCssPropertyName::BorderRightColor
        | ArchivedCssPropertyName::BorderBottomColor
        | ArchivedCssPropertyName::BorderLeftColor => None,

        // ── Border radius (not yet supported) ────────────────────
        ArchivedCssPropertyName::BorderTopLeftRadius
        | ArchivedCssPropertyName::BorderTopRightRadius
        | ArchivedCssPropertyName::BorderBottomLeftRadius
        | ArchivedCssPropertyName::BorderBottomRightRadius => None,

        // ── Positioning ──────────────────────────────────────────
        ArchivedCssPropertyName::Position => convert_position(value),
        ArchivedCssPropertyName::Top => ir_to_inset(value).map(PropertyDeclaration::Top),
        ArchivedCssPropertyName::Right => ir_to_inset(value).map(PropertyDeclaration::Right),
        ArchivedCssPropertyName::Bottom => ir_to_inset(value).map(PropertyDeclaration::Bottom),
        ArchivedCssPropertyName::Left => ir_to_inset(value).map(PropertyDeclaration::Left),
        ArchivedCssPropertyName::ZIndex => convert_z_index(value),
        ArchivedCssPropertyName::Float => convert_float(value),
        ArchivedCssPropertyName::Clear => convert_clear(value),

        // ── Flexbox ──────────────────────────────────────────────
        ArchivedCssPropertyName::FlexDirection => convert_flex_direction(value),
        ArchivedCssPropertyName::FlexWrap => convert_flex_wrap(value),
        ArchivedCssPropertyName::FlexGrow => convert_flex_grow(value),
        ArchivedCssPropertyName::FlexShrink => convert_flex_shrink(value),
        ArchivedCssPropertyName::FlexBasis => convert_flex_basis(value),
        ArchivedCssPropertyName::Order => convert_order(value),

        // ── Alignment (not yet supported) ────────────────────────
        ArchivedCssPropertyName::AlignItems
        | ArchivedCssPropertyName::AlignSelf
        | ArchivedCssPropertyName::AlignContent
        | ArchivedCssPropertyName::JustifyContent
        | ArchivedCssPropertyName::JustifyItems
        | ArchivedCssPropertyName::JustifySelf => None,

        // ── Grid (not yet supported) ─────────────────────────────
        ArchivedCssPropertyName::GridTemplateColumns
        | ArchivedCssPropertyName::GridTemplateRows
        | ArchivedCssPropertyName::GridAutoFlow
        | ArchivedCssPropertyName::GridAutoColumns
        | ArchivedCssPropertyName::GridAutoRows
        | ArchivedCssPropertyName::GridColumnStart
        | ArchivedCssPropertyName::GridColumnEnd
        | ArchivedCssPropertyName::GridRowStart
        | ArchivedCssPropertyName::GridRowEnd => None,

        // ── Gap ──────────────────────────────────────────────────
        ArchivedCssPropertyName::ColumnGap => ir_to_gap(value).map(PropertyDeclaration::ColumnGap),
        ArchivedCssPropertyName::RowGap => ir_to_gap(value).map(PropertyDeclaration::RowGap),

        // ── Visual ───────────────────────────────────────────────
        ArchivedCssPropertyName::Opacity => None,
        ArchivedCssPropertyName::OverflowX => convert_overflow_x(value),
        ArchivedCssPropertyName::OverflowY => convert_overflow_y(value),
        ArchivedCssPropertyName::Overflow => None,
        ArchivedCssPropertyName::Visibility => convert_visibility(value),
        ArchivedCssPropertyName::ObjectFit => convert_object_fit(value),
        ArchivedCssPropertyName::ObjectPosition => None,

        // ── Color (not yet supported) ────────────────────────────
        ArchivedCssPropertyName::Color | ArchivedCssPropertyName::BackgroundColor => None,

        // ── Typography (not yet supported) ───────────────────────
        ArchivedCssPropertyName::FontSize
        | ArchivedCssPropertyName::FontWeight
        | ArchivedCssPropertyName::FontFamily
        | ArchivedCssPropertyName::FontStyle
        | ArchivedCssPropertyName::LineHeight
        | ArchivedCssPropertyName::TextAlign
        | ArchivedCssPropertyName::TextDecoration
        | ArchivedCssPropertyName::TextTransform
        | ArchivedCssPropertyName::LetterSpacing
        | ArchivedCssPropertyName::WordSpacing
        | ArchivedCssPropertyName::WhiteSpace
        | ArchivedCssPropertyName::VerticalAlign => None,

        // ── Misc ─────────────────────────────────────────────────
        ArchivedCssPropertyName::AspectRatio => None,

        // ── Catch-all ────────────────────────────────────────────
        ArchivedCssPropertyName::Other(_) | ArchivedCssPropertyName::Custom(_) => None,
    }
}
