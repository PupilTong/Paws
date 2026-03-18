//! Grid property converters from Stylo to Taffy.

use super::length::length_percentage;
use super::stylo_types as st;

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
