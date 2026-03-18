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

mod enums;
mod flex;
mod grid;
pub(crate) mod length;

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
        display: enums::display(style.clone_display()),
        position: enums::position(style.clone_position()),
        overflow: taffy::Point {
            x: enums::overflow(style.clone_overflow_x()),
            y: enums::overflow(style.clone_overflow_y()),
        },
        scrollbar_width: 0.0,

        // Sizing
        size: sizing(&pos.width, &pos.height),
        min_size: sizing(&pos.min_width, &pos.min_height),
        max_size: max_sizing(&pos.max_width, &pos.max_height),
        aspect_ratio: enums::aspect_ratio(pos.aspect_ratio),

        // Inset
        inset: taffy::Rect {
            left: length::inset(&pos.left),
            right: length::inset(&pos.right),
            top: length::inset(&pos.top),
            bottom: length::inset(&pos.bottom),
        },

        // Spacing
        margin: taffy::Rect {
            left: length::margin(&margin_s.margin_left),
            right: length::margin(&margin_s.margin_right),
            top: length::margin(&margin_s.margin_top),
            bottom: length::margin(&margin_s.margin_bottom),
        },
        padding: taffy::Rect {
            left: length::length_percentage(&padding_s.padding_left.0),
            right: length::length_percentage(&padding_s.padding_right.0),
            top: length::length_percentage(&padding_s.padding_top.0),
            bottom: length::length_percentage(&padding_s.padding_bottom.0),
        },
        border: taffy::Rect {
            left: length::border(&border_s.border_left_width, border_s.border_left_style),
            right: length::border(&border_s.border_right_width, border_s.border_right_style),
            top: length::border(&border_s.border_top_width, border_s.border_top_style),
            bottom: length::border(&border_s.border_bottom_width, border_s.border_bottom_style),
        },

        // Gap
        gap: taffy::Size {
            width: length::gap(&pos.column_gap),
            height: length::gap(&pos.row_gap),
        },

        // Alignment
        align_content: enums::content_alignment(pos.align_content),
        justify_content: enums::content_alignment(pos.justify_content),
        align_items: enums::item_alignment(pos.align_items.0),
        align_self: enums::item_alignment(pos.align_self.0),
        justify_items: enums::item_alignment((pos.justify_items.computed.0).0),
        justify_self: enums::item_alignment(pos.justify_self.0),

        // Flexbox
        flex_direction: flex::flex_direction(pos.flex_direction),
        flex_wrap: flex::flex_wrap(pos.flex_wrap),
        flex_grow: pos.flex_grow.0,
        flex_shrink: pos.flex_shrink.0,
        flex_basis: flex::flex_basis(&pos.flex_basis),

        // Grid container
        grid_auto_flow: grid::grid_auto_flow(pos.grid_auto_flow),
        grid_template_rows: grid::grid_template_tracks(&pos.grid_template_rows),
        grid_template_columns: grid::grid_template_tracks(&pos.grid_template_columns),
        grid_auto_rows: grid::grid_auto_tracks(&pos.grid_auto_rows),
        grid_auto_columns: grid::grid_auto_tracks(&pos.grid_auto_columns),

        // Grid item
        grid_row: taffy::Line {
            start: grid::grid_line(&pos.grid_row_start),
            end: grid::grid_line(&pos.grid_row_end),
        },
        grid_column: taffy::Line {
            start: grid::grid_line(&pos.grid_column_start),
            end: grid::grid_line(&pos.grid_column_end),
        },
    }
}

// ─── Sizing helpers ──────────────────────────────────────────────────

#[inline]
fn sizing(width: &st::Size, height: &st::Size) -> taffy::Size<taffy::Dimension> {
    taffy::Size {
        width: length::dimension(width),
        height: length::dimension(height),
    }
}

#[inline]
fn max_sizing(width: &st::MaxSize, height: &st::MaxSize) -> taffy::Size<taffy::Dimension> {
    taffy::Size {
        width: length::max_size_dimension(width),
        height: length::max_size_dimension(height),
    }
}
