//! Direct conversion from Stylo [`ComputedValues`] to [`taffy::Style`].
//!
//! Vendored and adapted from [stylo_taffy](https://github.com/nicell/nicell/blitz/packages/stylo_taffy)
//! (`convert.rs`), targeting **stylo 0.11 + taffy 0.4** (the original targets stylo ^0.8 + taffy ^0.9).
//!
//! Key adaptations:
//! - Taffy 0.4 uses simple enum constructors (`Dimension::Auto`, `LengthPercentage::Length(f32)`)
//!   instead of compact representations and `style_helpers`.
//! - `calc()` values are resolved to their length component (best-effort) since taffy 0.4 has no
//!   `CompactLength`.
//! - Features absent from taffy 0.4 are omitted: `BoxSizing`, `TextAlign`, `Float`, `Clear`,
//!   grid named lines / template areas.

/// Private module of type aliases for cleaner references to Stylo types.
pub(crate) mod stylo_types {
    pub(crate) use style::properties::longhands::position::computed_value::T as Position;
    pub(crate) use style::properties::ComputedValues;
    pub(crate) use style::values::computed::length_percentage::Unpacked as UnpackedLP;
    pub(crate) use style::values::computed::{BorderSideWidth, LengthPercentage};
    pub(crate) use style::values::generics::length::{GenericMargin, GenericMaxSize, GenericSize};
    pub(crate) use style::values::generics::position::{GenericAspectRatio, Inset as GenericInset};
    pub(crate) use style::values::generics::NonNegative;
    pub(crate) use style::values::specified::align::{AlignFlags, ContentDistribution};
    pub(crate) use style::values::specified::border::BorderStyle;
    pub(crate) use style::values::specified::box_::{
        Display, DisplayInside, DisplayOutside, Overflow,
    };

    pub(crate) type MarginVal = GenericMargin<LengthPercentage>;
    pub(crate) type InsetVal = GenericInset<style::values::computed::Percentage, LengthPercentage>;
    pub(crate) type Size = GenericSize<NonNegative<LengthPercentage>>;
    pub(crate) type MaxSize = GenericMaxSize<NonNegative<LengthPercentage>>;

    pub(crate) type Gap = style::values::generics::length::GenericLengthPercentageOrNormal<
        NonNegative<LengthPercentage>,
    >;

    pub(crate) use style::computed_values::{
        flex_direction::T as FlexDirection, flex_wrap::T as FlexWrap,
    };
    pub(crate) use style::values::generics::flex::GenericFlexBasis;
    pub(crate) type FlexBasis = GenericFlexBasis<Size>;

    pub(crate) use style::computed_values::grid_auto_flow::T as GridAutoFlow;
    pub(crate) use style::values::computed::{GridLine, GridTemplateComponent, ImplicitGridTracks};
    pub(crate) use style::values::generics::grid::{
        RepeatCount, TrackBreadth, TrackListValue, TrackSize,
    };
    pub(crate) use style::values::specified::GenericGridTemplateComponent;
}

use stylo_types as st;

// ─── Primitive converters ────────────────────────────────────────────

/// Converts a Stylo `LengthPercentage` to a Taffy `LengthPercentage`.
#[inline]
pub fn length_percentage(val: &st::LengthPercentage) -> taffy::LengthPercentage {
    match val.unpack() {
        st::UnpackedLP::Length(len) => taffy::LengthPercentage::Length(len.px()),
        st::UnpackedLP::Percentage(pct) => taffy::LengthPercentage::Percent(pct.0),
        st::UnpackedLP::Calc(calc) => {
            // Best-effort: resolve calc() against a zero basis (drops percentage terms).
            // Taffy 0.4 has no calc representation; full support would need layout-time resolution.
            let resolved = calc.resolve(style::values::computed::Length::new(0.0));
            taffy::LengthPercentage::Length(resolved.px())
        }
    }
}

/// Converts a Stylo `Size` (width/height) to a Taffy `Dimension`.
#[inline]
pub fn dimension(val: &st::Size) -> taffy::Dimension {
    match val {
        st::Size::LengthPercentage(val) => lp_to_dimension(&val.0),
        st::Size::Auto => taffy::Dimension::Auto,
        // Taffy 0.4 lacks intrinsic sizing keywords; fall back to Auto.
        st::Size::MaxContent
        | st::Size::MinContent
        | st::Size::FitContent
        | st::Size::FitContentFunction(_)
        | st::Size::Stretch
        | st::Size::WebkitFillAvailable => taffy::Dimension::Auto,
        // Anchor positioning is not supported.
        st::Size::AnchorSizeFunction(_) | st::Size::AnchorContainingCalcFunction(_) => {
            taffy::Dimension::Auto
        }
    }
}

/// Converts a Stylo `MaxSize` to a Taffy `Dimension`.
#[inline]
pub fn max_size_dimension(val: &st::MaxSize) -> taffy::Dimension {
    match val {
        st::MaxSize::LengthPercentage(val) => lp_to_dimension(&val.0),
        st::MaxSize::None => taffy::Dimension::Auto,
        st::MaxSize::MaxContent
        | st::MaxSize::MinContent
        | st::MaxSize::FitContent
        | st::MaxSize::FitContentFunction(_)
        | st::MaxSize::Stretch
        | st::MaxSize::WebkitFillAvailable => taffy::Dimension::Auto,
        st::MaxSize::AnchorSizeFunction(_) | st::MaxSize::AnchorContainingCalcFunction(_) => {
            taffy::Dimension::Auto
        }
    }
}

/// Converts a Stylo margin value to a Taffy `LengthPercentageAuto`.
#[inline]
pub fn margin(val: &st::MarginVal) -> taffy::prelude::LengthPercentageAuto {
    match val {
        st::MarginVal::Auto => taffy::prelude::LengthPercentageAuto::Auto,
        st::MarginVal::LengthPercentage(val) => lp_to_lpa(val),
        // Anchor positioning not supported.
        st::MarginVal::AnchorSizeFunction(_) | st::MarginVal::AnchorContainingCalcFunction(_) => {
            taffy::prelude::LengthPercentageAuto::Auto
        }
    }
}

/// Converts a Stylo border width + style to a Taffy `LengthPercentage`.
///
/// Hidden/none borders resolve to zero width.
#[inline]
pub fn border(width: &st::BorderSideWidth, style: st::BorderStyle) -> taffy::LengthPercentage {
    if style.none_or_hidden() {
        return taffy::LengthPercentage::Length(0.0);
    }
    taffy::LengthPercentage::Length(width.0.to_f32_px())
}

/// Converts a Stylo inset (top/right/bottom/left) to Taffy `LengthPercentageAuto`.
#[inline]
pub fn inset(val: &st::InsetVal) -> taffy::prelude::LengthPercentageAuto {
    match val {
        st::InsetVal::Auto => taffy::prelude::LengthPercentageAuto::Auto,
        st::InsetVal::LengthPercentage(val) => lp_to_lpa(val),
        // Anchor positioning not supported.
        st::InsetVal::AnchorSizeFunction(_)
        | st::InsetVal::AnchorFunction(_)
        | st::InsetVal::AnchorContainingCalcFunction(_) => {
            taffy::prelude::LengthPercentageAuto::Auto
        }
    }
}

// ─── Enum converters ─────────────────────────────────────────────────

/// Converts a Stylo `Display` to a Taffy `Display`.
#[inline]
pub fn display(input: st::Display) -> taffy::Display {
    let mut display = match input.inside() {
        st::DisplayInside::None => taffy::Display::None,
        st::DisplayInside::Flex => taffy::Display::Flex,
        st::DisplayInside::Grid => taffy::Display::Grid,
        st::DisplayInside::Flow | st::DisplayInside::FlowRoot => taffy::Display::Block,
        _ => taffy::Display::Block,
    };

    if matches!(input.outside(), st::DisplayOutside::None) {
        display = taffy::Display::None;
    }

    display
}

/// Converts a Stylo `Position` to a Taffy `Position`.
#[inline]
pub fn position(input: st::Position) -> taffy::Position {
    match input {
        st::Position::Relative | st::Position::Static | st::Position::Sticky => {
            taffy::Position::Relative
        }
        st::Position::Absolute | st::Position::Fixed => taffy::Position::Absolute,
    }
}

/// Converts a Stylo `Overflow` to a Taffy `Overflow`.
#[inline]
pub fn overflow(input: st::Overflow) -> taffy::Overflow {
    match input {
        st::Overflow::Visible => taffy::Overflow::Visible,
        st::Overflow::Clip => taffy::Overflow::Clip,
        st::Overflow::Hidden => taffy::Overflow::Hidden,
        st::Overflow::Scroll => taffy::Overflow::Scroll,
        st::Overflow::Auto => taffy::Overflow::Scroll,
    }
}

/// Converts a Stylo `AspectRatio` to Taffy's `Option<f32>`.
#[inline]
pub fn aspect_ratio(input: st::GenericAspectRatio<st::NonNegative<f32>>) -> Option<f32> {
    use style::values::generics::position::PreferredRatio;
    match input.ratio {
        PreferredRatio::None => None,
        // Ratio<NonNegative<f32>>(width, height) → width.0 / height.0
        PreferredRatio::Ratio(val) => {
            let w = (val.0).0;
            let h = (val.1).0;
            if h != 0.0 {
                Some(w / h)
            } else {
                None
            }
        }
    }
}

/// Converts a Stylo `ContentDistribution` (align-content / justify-content) to Taffy.
#[inline]
pub fn content_alignment(input: st::ContentDistribution) -> Option<taffy::AlignContent> {
    match input.primary().value() {
        st::AlignFlags::NORMAL | st::AlignFlags::AUTO => None,
        st::AlignFlags::START | st::AlignFlags::LEFT => Some(taffy::AlignContent::Start),
        st::AlignFlags::END | st::AlignFlags::RIGHT => Some(taffy::AlignContent::End),
        st::AlignFlags::FLEX_START => Some(taffy::AlignContent::FlexStart),
        st::AlignFlags::FLEX_END => Some(taffy::AlignContent::FlexEnd),
        st::AlignFlags::CENTER => Some(taffy::AlignContent::Center),
        st::AlignFlags::STRETCH => Some(taffy::AlignContent::Stretch),
        st::AlignFlags::SPACE_BETWEEN => Some(taffy::AlignContent::SpaceBetween),
        st::AlignFlags::SPACE_AROUND => Some(taffy::AlignContent::SpaceAround),
        st::AlignFlags::SPACE_EVENLY => Some(taffy::AlignContent::SpaceEvenly),
        _ => None,
    }
}

/// Converts Stylo `AlignFlags` (align-items / align-self) to Taffy.
#[inline]
pub fn item_alignment(input: st::AlignFlags) -> Option<taffy::AlignItems> {
    match input.value() {
        st::AlignFlags::AUTO => None,
        st::AlignFlags::NORMAL | st::AlignFlags::STRETCH => Some(taffy::AlignItems::Stretch),
        st::AlignFlags::FLEX_START => Some(taffy::AlignItems::FlexStart),
        st::AlignFlags::FLEX_END => Some(taffy::AlignItems::FlexEnd),
        st::AlignFlags::SELF_START | st::AlignFlags::START | st::AlignFlags::LEFT => {
            Some(taffy::AlignItems::Start)
        }
        st::AlignFlags::SELF_END | st::AlignFlags::END | st::AlignFlags::RIGHT => {
            Some(taffy::AlignItems::End)
        }
        st::AlignFlags::CENTER => Some(taffy::AlignItems::Center),
        st::AlignFlags::BASELINE => Some(taffy::AlignItems::Baseline),
        _ => None,
    }
}

/// Converts a Stylo gap value to Taffy `LengthPercentage`.
#[inline]
pub fn gap(input: &st::Gap) -> taffy::LengthPercentage {
    match input {
        st::Gap::Normal => taffy::LengthPercentage::Length(0.0),
        st::Gap::LengthPercentage(val) => length_percentage(&val.0),
    }
}

// ─── Flexbox converters ──────────────────────────────────────────────

/// Converts Stylo `FlexDirection` to Taffy.
#[inline]
pub fn flex_direction(input: st::FlexDirection) -> taffy::FlexDirection {
    match input {
        st::FlexDirection::Row => taffy::FlexDirection::Row,
        st::FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
        st::FlexDirection::Column => taffy::FlexDirection::Column,
        st::FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
    }
}

/// Converts Stylo `FlexWrap` to Taffy.
#[inline]
pub fn flex_wrap(input: st::FlexWrap) -> taffy::FlexWrap {
    match input {
        st::FlexWrap::Wrap => taffy::FlexWrap::Wrap,
        st::FlexWrap::WrapReverse => taffy::FlexWrap::WrapReverse,
        st::FlexWrap::Nowrap => taffy::FlexWrap::NoWrap,
    }
}

/// Converts Stylo `FlexBasis` to Taffy `Dimension`.
#[inline]
pub fn flex_basis(input: &st::FlexBasis) -> taffy::Dimension {
    match input {
        st::FlexBasis::Content => taffy::Dimension::Auto,
        st::FlexBasis::Size(size) => dimension(size),
    }
}

// ─── Grid converters ─────────────────────────────────────────────────

/// Converts Stylo `GridAutoFlow` to Taffy.
#[inline]
pub fn grid_auto_flow(input: st::GridAutoFlow) -> taffy::GridAutoFlow {
    let is_row = input.contains(st::GridAutoFlow::ROW);
    let is_dense = input.contains(st::GridAutoFlow::DENSE);

    match (is_row, is_dense) {
        (true, false) => taffy::GridAutoFlow::Row,
        (true, true) => taffy::GridAutoFlow::RowDense,
        (false, false) => taffy::GridAutoFlow::Column,
        (false, true) => taffy::GridAutoFlow::ColumnDense,
    }
}

/// Converts a Stylo `GridLine` to a Taffy `GridPlacement`.
///
/// Taffy 0.4 does not support named grid lines, so named values fall back to `Auto`.
#[inline]
pub fn grid_line(input: &st::GridLine) -> taffy::prelude::GridPlacement {
    use taffy::prelude::GridPlacement;
    if input.is_auto() {
        GridPlacement::Auto
    } else if input.is_span {
        GridPlacement::Span(input.line_num as u16)
    } else if input.line_num != 0 {
        GridPlacement::Line((input.line_num as i16).into())
    } else {
        GridPlacement::Auto
    }
}

/// Converts a Stylo `GridTemplateComponent` to a Vec of Taffy `TrackSizingFunction`.
#[inline]
pub fn grid_template_tracks(input: &st::GridTemplateComponent) -> Vec<taffy::TrackSizingFunction> {
    match input {
        st::GenericGridTemplateComponent::None => Vec::new(),
        st::GenericGridTemplateComponent::TrackList(list) => list
            .values
            .iter()
            .map(|track| match track {
                st::TrackListValue::TrackSize(size) => {
                    taffy::TrackSizingFunction::Single(track_size(size))
                }
                st::TrackListValue::TrackRepeat(repeat) => taffy::TrackSizingFunction::Repeat(
                    track_repeat(repeat.count),
                    repeat.track_sizes.iter().map(track_size).collect(),
                ),
            })
            .collect(),
        st::GenericGridTemplateComponent::Subgrid(_)
        | st::GenericGridTemplateComponent::Masonry => Vec::new(),
    }
}

/// Converts Stylo implicit grid tracks to a Vec of Taffy `NonRepeatedTrackSizingFunction`.
#[inline]
pub fn grid_auto_tracks(
    input: &st::ImplicitGridTracks,
) -> Vec<taffy::NonRepeatedTrackSizingFunction> {
    input.0.iter().map(track_size).collect()
}

/// Converts a Stylo `RepeatCount` to a Taffy `GridTrackRepetition`.
#[inline]
fn track_repeat(input: st::RepeatCount<i32>) -> taffy::GridTrackRepetition {
    match input {
        st::RepeatCount::Number(val) => {
            taffy::GridTrackRepetition::Count(val.try_into().unwrap_or(1))
        }
        st::RepeatCount::AutoFill => taffy::GridTrackRepetition::AutoFill,
        st::RepeatCount::AutoFit => taffy::GridTrackRepetition::AutoFit,
    }
}

/// Converts a Stylo `TrackSize` to a Taffy `NonRepeatedTrackSizingFunction` (= `MinMax<Min, Max>`).
#[inline]
fn track_size(
    input: &st::TrackSize<st::LengthPercentage>,
) -> taffy::NonRepeatedTrackSizingFunction {
    match input {
        st::TrackSize::Breadth(breadth) => taffy::MinMax {
            min: min_track(breadth),
            max: max_track(breadth),
        },
        st::TrackSize::Minmax(min, max) => taffy::MinMax {
            min: min_track(min),
            max: max_track(max),
        },
        st::TrackSize::FitContent(limit) => taffy::MinMax {
            min: taffy::MinTrackSizingFunction::Auto,
            max: match limit {
                st::TrackBreadth::Breadth(lp) => {
                    taffy::MaxTrackSizingFunction::FitContent(length_percentage(lp))
                }
                _ => taffy::MaxTrackSizingFunction::Auto,
            },
        },
    }
}

/// Converts a Stylo `TrackBreadth` to a Taffy `MinTrackSizingFunction`.
#[inline]
fn min_track(input: &st::TrackBreadth<st::LengthPercentage>) -> taffy::MinTrackSizingFunction {
    match input {
        st::TrackBreadth::Breadth(lp) => {
            taffy::MinTrackSizingFunction::Fixed(length_percentage(lp))
        }
        st::TrackBreadth::Fr(_) | st::TrackBreadth::Auto => taffy::MinTrackSizingFunction::Auto,
        st::TrackBreadth::MinContent => taffy::MinTrackSizingFunction::MinContent,
        st::TrackBreadth::MaxContent => taffy::MinTrackSizingFunction::MaxContent,
    }
}

/// Converts a Stylo `TrackBreadth` to a Taffy `MaxTrackSizingFunction`.
#[inline]
fn max_track(input: &st::TrackBreadth<st::LengthPercentage>) -> taffy::MaxTrackSizingFunction {
    match input {
        st::TrackBreadth::Breadth(lp) => {
            taffy::MaxTrackSizingFunction::Fixed(length_percentage(lp))
        }
        st::TrackBreadth::Fr(val) => taffy::MaxTrackSizingFunction::Fraction(*val),
        st::TrackBreadth::Auto => taffy::MaxTrackSizingFunction::Auto,
        st::TrackBreadth::MinContent => taffy::MaxTrackSizingFunction::MinContent,
        st::TrackBreadth::MaxContent => taffy::MaxTrackSizingFunction::MaxContent,
    }
}

// ─── Main entry point ────────────────────────────────────────────────

/// Eagerly converts an entire Stylo [`ComputedValues`] into a [`taffy::Style`].
///
/// This replaces the string round-trip (`ComputedValues → CSS string → parse → taffy`)
/// with direct type-level conversion, handling percentages, calc(), and all layout properties.
pub fn to_taffy_style(style: &st::ComputedValues) -> taffy::Style {
    let pos = style.get_position();
    let margin_s = style.get_margin();
    let padding_s = style.get_padding();
    let border_s = style.get_border();

    taffy::Style {
        display: self::display(style.clone_display()),
        position: self::position(style.clone_position()),
        overflow: taffy::Point {
            x: self::overflow(style.clone_overflow_x()),
            y: self::overflow(style.clone_overflow_y()),
        },
        scrollbar_width: 0.0,

        // Sizing
        size: taffy::Size {
            width: self::dimension(&pos.width),
            height: self::dimension(&pos.height),
        },
        min_size: taffy::Size {
            width: self::dimension(&pos.min_width),
            height: self::dimension(&pos.min_height),
        },
        max_size: taffy::Size {
            width: self::max_size_dimension(&pos.max_width),
            height: self::max_size_dimension(&pos.max_height),
        },
        aspect_ratio: self::aspect_ratio(pos.aspect_ratio),

        // Inset (top/right/bottom/left)
        inset: taffy::Rect {
            left: self::inset(&pos.left),
            right: self::inset(&pos.right),
            top: self::inset(&pos.top),
            bottom: self::inset(&pos.bottom),
        },

        // Spacing
        margin: taffy::Rect {
            left: self::margin(&margin_s.margin_left),
            right: self::margin(&margin_s.margin_right),
            top: self::margin(&margin_s.margin_top),
            bottom: self::margin(&margin_s.margin_bottom),
        },
        padding: taffy::Rect {
            left: self::length_percentage(&padding_s.padding_left.0),
            right: self::length_percentage(&padding_s.padding_right.0),
            top: self::length_percentage(&padding_s.padding_top.0),
            bottom: self::length_percentage(&padding_s.padding_bottom.0),
        },
        border: taffy::Rect {
            left: self::border(&border_s.border_left_width, border_s.border_left_style),
            right: self::border(&border_s.border_right_width, border_s.border_right_style),
            top: self::border(&border_s.border_top_width, border_s.border_top_style),
            bottom: self::border(&border_s.border_bottom_width, border_s.border_bottom_style),
        },

        // Gap
        gap: taffy::Size {
            width: self::gap(&pos.column_gap),
            height: self::gap(&pos.row_gap),
        },

        // Alignment
        align_content: self::content_alignment(pos.align_content),
        justify_content: self::content_alignment(pos.justify_content),
        align_items: self::item_alignment(pos.align_items.0),
        align_self: self::item_alignment(pos.align_self.0),
        justify_items: self::item_alignment((pos.justify_items.computed.0).0),
        justify_self: self::item_alignment(pos.justify_self.0),

        // Flexbox
        flex_direction: self::flex_direction(pos.flex_direction),
        flex_wrap: self::flex_wrap(pos.flex_wrap),
        flex_grow: pos.flex_grow.0,
        flex_shrink: pos.flex_shrink.0,
        flex_basis: self::flex_basis(&pos.flex_basis),

        // Grid container
        grid_auto_flow: self::grid_auto_flow(pos.grid_auto_flow),
        grid_template_rows: self::grid_template_tracks(&pos.grid_template_rows),
        grid_template_columns: self::grid_template_tracks(&pos.grid_template_columns),
        grid_auto_rows: self::grid_auto_tracks(&pos.grid_auto_rows),
        grid_auto_columns: self::grid_auto_tracks(&pos.grid_auto_columns),

        // Grid item
        grid_row: taffy::Line {
            start: self::grid_line(&pos.grid_row_start),
            end: self::grid_line(&pos.grid_row_end),
        },
        grid_column: taffy::Line {
            start: self::grid_line(&pos.grid_column_start),
            end: self::grid_line(&pos.grid_column_end),
        },
    }
}

// ─── Internal helpers ────────────────────────────────────────────────

/// Converts a Stylo `LengthPercentage` to a Taffy `Dimension`.
#[inline]
fn lp_to_dimension(val: &st::LengthPercentage) -> taffy::Dimension {
    match val.unpack() {
        st::UnpackedLP::Length(len) => taffy::Dimension::Length(len.px()),
        st::UnpackedLP::Percentage(pct) => taffy::Dimension::Percent(pct.0),
        st::UnpackedLP::Calc(calc) => {
            let resolved = calc.resolve(style::values::computed::Length::new(0.0));
            taffy::Dimension::Length(resolved.px())
        }
    }
}

/// Converts a Stylo `LengthPercentage` to a Taffy `LengthPercentageAuto`.
#[inline]
fn lp_to_lpa(val: &st::LengthPercentage) -> taffy::prelude::LengthPercentageAuto {
    match val.unpack() {
        st::UnpackedLP::Length(len) => taffy::prelude::LengthPercentageAuto::Length(len.px()),
        st::UnpackedLP::Percentage(pct) => taffy::prelude::LengthPercentageAuto::Percent(pct.0),
        st::UnpackedLP::Calc(calc) => {
            let resolved = calc.resolve(style::values::computed::Length::new(0.0));
            taffy::prelude::LengthPercentageAuto::Length(resolved.px())
        }
    }
}
