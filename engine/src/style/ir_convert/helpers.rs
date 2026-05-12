//! IR → Stylo value conversion helpers.
//!
//! This module provides two categories of helpers:
//!
//! 1. **Typed IR converters** — infallible conversions from the validated IR
//!    types (`ArchivedSizeIR`, `ArchivedMarginIR`, etc.) to Stylo specified
//!    values.  These are the primary path.
//!
//! 2. **Raw token fallback helpers** — used only for `PropertyValueIR::Raw`
//!    tokens that haven't been typed yet.  These retain the original
//!    string-matching logic from before the typed IR refactor.

use ::style::values::computed::Percentage;
use ::style::values::generics::NonNegative;
use ::style::values::specified::length::LengthPercentage;
use core::fmt::Write;
use paws_style_ir::{
    ArchivedCssToken, ArchivedCssUnit, ArchivedGapIR, ArchivedInsetIR, ArchivedMarginIR,
    ArchivedMaxSizeIR, ArchivedSizeIR,
};

use super::length::{lp_ir_to_stylo, nn_lp_ir_to_stylo, no_calc_length};

// ═════════════════════════════════════════════════════════════════════
// Typed IR → Stylo converters (infallible)
// ═════════════════════════════════════════════════════════════════════

/// Converts an [`ArchivedSizeIR`] to a Stylo `Size`.
pub(crate) fn size_ir_to_stylo(ir: &ArchivedSizeIR) -> ::style::values::specified::Size {
    use ::style::values::specified::Size;
    match ir {
        ArchivedSizeIR::Auto => Size::Auto,
        ArchivedSizeIR::LengthPercentage(ref lp) => Size::LengthPercentage(nn_lp_ir_to_stylo(lp)),
    }
}

/// Converts an [`ArchivedMaxSizeIR`] to a Stylo `MaxSize`.
pub(crate) fn max_size_ir_to_stylo(ir: &ArchivedMaxSizeIR) -> ::style::values::specified::MaxSize {
    use ::style::values::specified::MaxSize;
    match ir {
        ArchivedMaxSizeIR::None => MaxSize::None,
        ArchivedMaxSizeIR::LengthPercentage(ref lp) => {
            MaxSize::LengthPercentage(nn_lp_ir_to_stylo(lp))
        }
    }
}

/// Converts an [`ArchivedMarginIR`] to a Stylo `Margin`.
pub(crate) fn margin_ir_to_stylo(
    ir: &ArchivedMarginIR,
) -> ::style::values::specified::length::Margin {
    use ::style::values::specified::length::Margin;
    match ir {
        ArchivedMarginIR::Auto => Margin::Auto,
        ArchivedMarginIR::LengthPercentage(ref lp) => Margin::LengthPercentage(lp_ir_to_stylo(lp)),
    }
}

/// Converts an [`ArchivedInsetIR`] to a Stylo `Inset`.
pub(crate) fn inset_ir_to_stylo(ir: &ArchivedInsetIR) -> ::style::values::specified::Inset {
    use ::style::values::specified::Inset;
    match ir {
        ArchivedInsetIR::Auto => Inset::Auto,
        ArchivedInsetIR::LengthPercentage(ref lp) => Inset::LengthPercentage(lp_ir_to_stylo(lp)),
    }
}

/// Converts an [`ArchivedGapIR`] to a Stylo `NonNegativeLengthPercentageOrNormal`.
pub(crate) fn gap_ir_to_stylo(
    ir: &ArchivedGapIR,
) -> ::style::values::specified::length::NonNegativeLengthPercentageOrNormal {
    use ::style::values::specified::length::NonNegativeLengthPercentageOrNormal;
    match ir {
        ArchivedGapIR::Normal => NonNegativeLengthPercentageOrNormal::Normal,
        ArchivedGapIR::LengthPercentage(ref lp) => {
            NonNegativeLengthPercentageOrNormal::LengthPercentage(nn_lp_ir_to_stylo(lp))
        }
    }
}

// ═════════════════════════════════════════════════════════════════════
// Raw token fallback helpers (for PropertyValueIR::Raw)
// ═════════════════════════════════════════════════════════════════════

/// Extracts a keyword string from a single-value token list.
pub(crate) fn ir_keyword(values: &[ArchivedCssToken]) -> Option<&str> {
    match values {
        [ArchivedCssToken::Ident(ref keyword)] => Some(keyword.as_str()),
        _ => None,
    }
}

/// Extracts a bare numeric value from a single `<number-token>`.
pub(crate) fn ir_unitless(values: &[ArchivedCssToken]) -> Option<f32> {
    match values {
        [ArchivedCssToken::Number(val)] => Some((*val).into()),
        _ => None,
    }
}

/// Converts a single-value token list to a Stylo [`LengthPercentage`] (Raw fallback).
pub(crate) fn ir_to_lp(values: &[ArchivedCssToken]) -> Option<LengthPercentage> {
    match values {
        [ArchivedCssToken::Percentage(val)] => {
            let v: f32 = (*val).into();
            Some(LengthPercentage::Percentage(Percentage(v / 100.0)))
        }
        [ArchivedCssToken::Dimension(val, ref unit)] => {
            let v: f32 = (*val).into();
            no_calc_length(v, unit).map(LengthPercentage::Length)
        }
        // Bare zero is a valid zero-length.
        [ArchivedCssToken::Number(val)] if Into::<f32>::into(*val) == 0.0 => Some(
            LengthPercentage::Length(::style::values::specified::length::NoCalcLength::Absolute(
                ::style::values::specified::length::AbsoluteLength::Px(0.0),
            )),
        ),
        _ => None,
    }
}

/// Converts a single-value token list to a `NonNegative<LengthPercentage>` (Raw fallback).
pub(crate) fn ir_to_nn_lp(values: &[ArchivedCssToken]) -> Option<NonNegative<LengthPercentage>> {
    match values {
        [ArchivedCssToken::Percentage(val)] => {
            let v: f32 = (*val).into();
            if v < 0.0 {
                return None;
            }
            Some(NonNegative(LengthPercentage::Percentage(Percentage(
                v / 100.0,
            ))))
        }
        [ArchivedCssToken::Dimension(val, ref unit)] => {
            let v: f32 = (*val).into();
            if v < 0.0 {
                return None;
            }
            no_calc_length(v, unit).map(|l| NonNegative(LengthPercentage::Length(l)))
        }
        [ArchivedCssToken::Number(val)] if Into::<f32>::into(*val) == 0.0 => Some(NonNegative(
            LengthPercentage::Length(::style::values::specified::length::NoCalcLength::Absolute(
                ::style::values::specified::length::AbsoluteLength::Px(0.0),
            )),
        )),
        _ => None,
    }
}

/// Converts a token list to a Stylo `Size` (Raw fallback).
pub(crate) fn ir_to_size(values: &[ArchivedCssToken]) -> Option<::style::values::specified::Size> {
    use ::style::values::specified::Size;
    if ir_keyword(values) == Some("auto") {
        Some(Size::Auto)
    } else {
        ir_to_nn_lp(values).map(Size::LengthPercentage)
    }
}

/// Converts a token list to a Stylo `MaxSize` (Raw fallback).
pub(crate) fn ir_to_max_size(
    values: &[ArchivedCssToken],
) -> Option<::style::values::specified::MaxSize> {
    use ::style::values::specified::MaxSize;
    if ir_keyword(values) == Some("none") {
        Some(MaxSize::None)
    } else {
        ir_to_nn_lp(values).map(MaxSize::LengthPercentage)
    }
}

/// Converts a token list to a Stylo `Margin` (Raw fallback).
pub(crate) fn ir_to_margin(
    values: &[ArchivedCssToken],
) -> Option<::style::values::specified::length::Margin> {
    use ::style::values::specified::length::Margin;
    if ir_keyword(values) == Some("auto") {
        Some(Margin::Auto)
    } else {
        ir_to_lp(values).map(Margin::LengthPercentage)
    }
}

/// Converts a token list to a Stylo `Inset` (Raw fallback).
pub(crate) fn ir_to_inset(
    values: &[ArchivedCssToken],
) -> Option<::style::values::specified::Inset> {
    use ::style::values::specified::Inset;
    if ir_keyword(values) == Some("auto") {
        Some(Inset::Auto)
    } else {
        ir_to_lp(values).map(Inset::LengthPercentage)
    }
}

/// Converts a keyword token list to a Stylo `BorderStyle` (Raw fallback).
pub(crate) fn ir_to_border_style(
    values: &[ArchivedCssToken],
) -> Option<::style::values::specified::BorderStyle> {
    use ::style::values::specified::BorderStyle;
    match ir_keyword(values)? {
        "none" => Some(BorderStyle::None),
        "hidden" => Some(BorderStyle::Hidden),
        "solid" => Some(BorderStyle::Solid),
        "double" => Some(BorderStyle::Double),
        "dotted" => Some(BorderStyle::Dotted),
        "dashed" => Some(BorderStyle::Dashed),
        "groove" => Some(BorderStyle::Groove),
        "ridge" => Some(BorderStyle::Ridge),
        "inset" => Some(BorderStyle::Inset),
        "outset" => Some(BorderStyle::Outset),
        _ => None,
    }
}

/// Converts a keyword token list to a Stylo `BorderSideWidth` (Raw fallback).
pub(crate) fn ir_to_border_width(
    values: &[ArchivedCssToken],
) -> Option<::style::values::specified::BorderSideWidth> {
    use ::style::values::specified::BorderSideWidth;
    if ir_keyword(values)? == "medium" {
        Some(BorderSideWidth::medium())
    } else {
        None
    }
}

/// Converts a token list to a `NonNegativeLengthPercentageOrNormal` (Raw fallback).
pub(crate) fn ir_to_gap(
    values: &[ArchivedCssToken],
) -> Option<::style::values::specified::length::NonNegativeLengthPercentageOrNormal> {
    use ::style::values::specified::length::NonNegativeLengthPercentageOrNormal;
    if ir_keyword(values) == Some("normal") {
        Some(NonNegativeLengthPercentageOrNormal::Normal)
    } else {
        ir_to_nn_lp(values).map(NonNegativeLengthPercentageOrNormal::LengthPercentage)
    }
}

// ═════════════════════════════════════════════════════════════════════
// Token → CSS-source stringifier (for the generic Stylo Raw fallback)
// ═════════════════════════════════════════════════════════════════════

/// Returns the CSS-source string for an archived [`CssUnit`].
///
/// Mirrors `CssUnit::as_str` but operates on the archived enum, which has
/// no `as_str` method of its own.
fn archived_unit_as_str(unit: &ArchivedCssUnit) -> &'static str {
    match unit {
        ArchivedCssUnit::Px => "px",
        ArchivedCssUnit::Cm => "cm",
        ArchivedCssUnit::Mm => "mm",
        ArchivedCssUnit::In => "in",
        ArchivedCssUnit::Pt => "pt",
        ArchivedCssUnit::Pc => "pc",
        ArchivedCssUnit::Q => "q",
        ArchivedCssUnit::Em => "em",
        ArchivedCssUnit::Rem => "rem",
        ArchivedCssUnit::Ex => "ex",
        ArchivedCssUnit::Ch => "ch",
        ArchivedCssUnit::Vh => "vh",
        ArchivedCssUnit::Vw => "vw",
        ArchivedCssUnit::Vmin => "vmin",
        ArchivedCssUnit::Vmax => "vmax",
        ArchivedCssUnit::Svh => "svh",
        ArchivedCssUnit::Svw => "svw",
        ArchivedCssUnit::Lvh => "lvh",
        ArchivedCssUnit::Lvw => "lvw",
        ArchivedCssUnit::Dvh => "dvh",
        ArchivedCssUnit::Dvw => "dvw",
        ArchivedCssUnit::Cqw => "cqw",
        ArchivedCssUnit::Cqh => "cqh",
        ArchivedCssUnit::Cqi => "cqi",
        ArchivedCssUnit::Cqb => "cqb",
        ArchivedCssUnit::Cqmin => "cqmin",
        ArchivedCssUnit::Cqmax => "cqmax",
        ArchivedCssUnit::Fr => "fr",
        ArchivedCssUnit::Deg => "deg",
        ArchivedCssUnit::Rad => "rad",
        ArchivedCssUnit::Grad => "grad",
        ArchivedCssUnit::Turn => "turn",
        ArchivedCssUnit::S => "s",
        ArchivedCssUnit::Ms => "ms",
        ArchivedCssUnit::Dpi => "dpi",
        ArchivedCssUnit::Dpcm => "dpcm",
        ArchivedCssUnit::Dppx => "dppx",
    }
}

/// Reconstructs CSS-source text from a slice of archived tokens.
///
/// Used by the generic Stylo fallback to hand a `Raw` property value back to
/// Stylo's parser, which then handles shorthand expansion and value
/// validation for every property Stylo can compute.
///
/// The reconstruction is always syntactically valid CSS but is not
/// byte-identical to the source: a single space separator is inserted
/// between every pair of tokens that would otherwise re-tokenise as a
/// single token (e.g. between `Ident("hidden")` and `Ident("scroll")` so
/// the result is `"hidden scroll"`, not `"hiddenscroll"`). This is
/// necessary because the macro's `parse_tokens` uses `cssparser::next()`
/// which skips whitespace tokens; the original spacing is lost by the
/// time the IR reaches the engine. Stylo's parser re-tokenises the
/// output, so any added whitespace simply collapses.
///
/// Strings escape `\` and `"` per CSS Syntax Level 3 §4.3.5; bad-string /
/// bad-url tokens emit obviously-invalid output so Stylo rejects them
/// rather than silently accepting garbage.
pub(crate) fn tokens_to_css_string(values: &[ArchivedCssToken]) -> String {
    let mut out = String::with_capacity(values.len() * 4);
    let mut prev: Option<&ArchivedCssToken> = None;
    for tok in values {
        if let Some(p) = prev {
            if needs_separator_between(p, tok) {
                out.push(' ');
            }
        }
        emit_token(&mut out, tok);
        prev = Some(tok);
    }
    out
}

fn emit_token(out: &mut String, tok: &ArchivedCssToken) {
    match tok {
        ArchivedCssToken::Ident(s) => out.push_str(s.as_str()),
        ArchivedCssToken::Function(name) => {
            out.push_str(name.as_str());
            out.push('(');
        }
        ArchivedCssToken::AtKeyword(s) => {
            out.push('@');
            out.push_str(s.as_str());
        }
        ArchivedCssToken::Hash(s, _) => {
            out.push('#');
            out.push_str(s.as_str());
        }
        ArchivedCssToken::String(s) => {
            out.push('"');
            for ch in s.as_str().chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\A "),
                    _ => out.push(ch),
                }
            }
            out.push('"');
        }
        ArchivedCssToken::BadString => out.push('"'),
        ArchivedCssToken::Url(u) => {
            out.push_str("url(\"");
            for ch in u.as_str().chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    _ => out.push(ch),
                }
            }
            out.push_str("\")");
        }
        ArchivedCssToken::BadUrl => out.push_str("url("),
        ArchivedCssToken::Delim(c) => out.push(char::from(*c)),
        ArchivedCssToken::Number(val) => {
            let _ = write!(out, "{}", Into::<f32>::into(*val));
        }
        ArchivedCssToken::Percentage(val) => {
            let _ = write!(out, "{}%", Into::<f32>::into(*val));
        }
        ArchivedCssToken::Dimension(val, unit) => {
            let _ = write!(
                out,
                "{}{}",
                Into::<f32>::into(*val),
                archived_unit_as_str(unit)
            );
        }
        ArchivedCssToken::UnicodeRange(start, end) => {
            let _ = write!(out, "U+{:X}-{:X}", u32::from(*start), u32::from(*end));
        }
        ArchivedCssToken::Whitespace => out.push(' '),
        ArchivedCssToken::CDO => out.push_str("<!--"),
        ArchivedCssToken::CDC => out.push_str("-->"),
        ArchivedCssToken::Colon => out.push(':'),
        ArchivedCssToken::Semicolon => out.push(';'),
        ArchivedCssToken::Comma => out.push(','),
        ArchivedCssToken::OpenSquare => out.push('['),
        ArchivedCssToken::CloseSquare => out.push(']'),
        ArchivedCssToken::OpenParen => out.push('('),
        ArchivedCssToken::CloseParen => out.push(')'),
        ArchivedCssToken::OpenCurly => out.push('{'),
        ArchivedCssToken::CloseCurly => out.push('}'),
    }
}

/// Returns `true` if a whitespace separator must be inserted between
/// `prev` and `next` to keep the re-tokenisation lossless.
///
/// Conservative: when in doubt, insert a separator. Stylo's parser
/// collapses excess whitespace, so over-separation is harmless; the only
/// places we suppress it are where the prior token already ends with a
/// "natural separator" (open paren, comma, whitespace, etc.) or where
/// inserting one would change semantics (none in current property-value
/// usage).
fn needs_separator_between(prev: &ArchivedCssToken, next: &ArchivedCssToken) -> bool {
    use ArchivedCssToken::*;

    // The prior token already wrote a trailing separator-like char, or the
    // next token's leading char is itself a separator: no extra space.
    let prev_self_separates = matches!(
        prev,
        Whitespace
            | Comma
            | Colon
            | Semicolon
            | Function(_)            // emits `name(`
            | OpenParen
            | OpenSquare
            | OpenCurly
            | CDO
    );
    let next_self_separates = matches!(
        next,
        Whitespace | Comma | Colon | Semicolon | CloseParen | CloseSquare | CloseCurly | CDC
    );
    !(prev_self_separates || next_self_separates)
}
