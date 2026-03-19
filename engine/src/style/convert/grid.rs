//! Grid property converters from Stylo to Taffy.
//!
//! Targets taffy 0.9.2 renamed grid types:
//! - `TrackSizingFunction` (was `NonRepeatedTrackSizingFunction`)
//! - `GridTemplateComponent<S>` (was `TrackSizingFunction`)
//! - `RepetitionCount` (was `GridTrackRepetition`)

use super::length::length_percentage;
use super::stylo_types as st;
use taffy::prelude::*;

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
/// Supports named grid lines via `GridPlacement::NamedLine` and `GridPlacement::NamedSpan`.
#[inline]
pub fn grid_line(input: &st::GridLine) -> taffy::GridPlacement<String> {
    use stylo_atoms::atom;

    if input.is_auto() {
        taffy::GridPlacement::Auto
    } else if input.is_span {
        if input.ident.0 != atom!("") {
            taffy::GridPlacement::NamedSpan(
                input.ident.0.to_string(),
                input.line_num.try_into().unwrap(),
            )
        } else {
            taffy::GridPlacement::Span(input.line_num as u16)
        }
    } else if input.ident.0 != atom!("") {
        taffy::GridPlacement::NamedLine(input.ident.0.to_string(), input.line_num as i16)
    } else if input.line_num != 0 {
        taffy::style_helpers::line(input.line_num as i16)
    } else {
        taffy::GridPlacement::Auto
    }
}

/// Converts a Stylo `GridTemplateComponent` to a Vec of Taffy `GridTemplateComponent`.
#[inline]
pub fn grid_template_tracks(
    input: &st::GridTemplateComponent,
) -> Vec<taffy::GridTemplateComponent<String>> {
    match input {
        st::GenericGridTemplateComponent::None => Vec::new(),
        st::GenericGridTemplateComponent::TrackList(list) => list
            .values
            .iter()
            .map(|track| match track {
                st::TrackListValue::TrackSize(size) => {
                    taffy::GridTemplateComponent::Single(track_size(size))
                }
                st::TrackListValue::TrackRepeat(repeat) => {
                    taffy::GridTemplateComponent::Repeat(taffy::GridTemplateRepetition {
                        count: track_repeat(repeat.count),
                        tracks: repeat.track_sizes.iter().map(track_size).collect(),
                        line_names: repeat
                            .line_names
                            .iter()
                            .map(|line_name_set| {
                                line_name_set
                                    .iter()
                                    .map(|ident| ident.0.to_string())
                                    .collect::<Vec<_>>()
                            })
                            .collect::<Vec<_>>(),
                    })
                }
            })
            .collect(),
        st::GenericGridTemplateComponent::Subgrid(_)
        | st::GenericGridTemplateComponent::Masonry => Vec::new(),
    }
}

/// Extracts grid template line names from a Stylo `GridTemplateComponent`.
#[inline]
pub fn grid_template_line_names(input: &st::GridTemplateComponent) -> Vec<Vec<String>> {
    match input {
        st::GenericGridTemplateComponent::TrackList(list) => list
            .line_names
            .iter()
            .map(|line_name_set| {
                line_name_set
                    .iter()
                    .map(|ident| ident.0.to_string())
                    .collect::<Vec<_>>()
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Converts Stylo grid template areas to Taffy `GridTemplateArea` list.
#[inline]
pub fn grid_template_areas(input: &st::GridTemplateAreas) -> Vec<taffy::GridTemplateArea<String>> {
    match input {
        st::GridTemplateAreas::None => Vec::new(),
        st::GridTemplateAreas::Areas(template_areas_arc) => template_areas_arc
            .0
            .areas
            .iter()
            .map(|area| taffy::GridTemplateArea {
                name: area.name.to_string(),
                row_start: area.rows.start as u16,
                row_end: area.rows.end as u16,
                column_start: area.columns.start as u16,
                column_end: area.columns.end as u16,
            })
            .collect(),
    }
}

/// Converts Stylo implicit grid tracks to a Vec of Taffy `TrackSizingFunction`.
#[inline]
pub fn grid_auto_tracks(input: &st::ImplicitGridTracks) -> Vec<taffy::TrackSizingFunction> {
    input.0.iter().map(track_size).collect()
}

/// Converts a Stylo `RepeatCount` to a Taffy `RepetitionCount`.
#[inline]
fn track_repeat(input: st::RepeatCount<i32>) -> taffy::RepetitionCount {
    match input {
        st::RepeatCount::Number(val) => taffy::RepetitionCount::Count(val.try_into().unwrap_or(1)),
        st::RepeatCount::AutoFill => taffy::RepetitionCount::AutoFill,
        st::RepeatCount::AutoFit => taffy::RepetitionCount::AutoFit,
    }
}

/// Converts a Stylo `TrackSize` to a Taffy `TrackSizingFunction` (= `MinMax<Min, Max>`).
#[inline]
fn track_size(input: &st::TrackSize<st::LengthPercentage>) -> taffy::TrackSizingFunction {
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
            min: taffy::MinTrackSizingFunction::AUTO,
            max: match limit {
                st::TrackBreadth::Breadth(lp) => {
                    taffy::MaxTrackSizingFunction::fit_content(length_percentage(lp))
                }
                _ => taffy::MaxTrackSizingFunction::AUTO,
            },
        },
    }
}

/// Converts a Stylo `TrackBreadth` to a Taffy `MinTrackSizingFunction`.
#[inline]
fn min_track(input: &st::TrackBreadth<st::LengthPercentage>) -> taffy::MinTrackSizingFunction {
    match input {
        st::TrackBreadth::Breadth(lp) => taffy::MinTrackSizingFunction::from(length_percentage(lp)),
        st::TrackBreadth::Fr(_) | st::TrackBreadth::Auto => taffy::MinTrackSizingFunction::AUTO,
        st::TrackBreadth::MinContent => taffy::MinTrackSizingFunction::MIN_CONTENT,
        st::TrackBreadth::MaxContent => taffy::MinTrackSizingFunction::MAX_CONTENT,
    }
}

/// Converts a Stylo `TrackBreadth` to a Taffy `MaxTrackSizingFunction`.
#[inline]
fn max_track(input: &st::TrackBreadth<st::LengthPercentage>) -> taffy::MaxTrackSizingFunction {
    match input {
        st::TrackBreadth::Breadth(lp) => taffy::MaxTrackSizingFunction::from(length_percentage(lp)),
        st::TrackBreadth::Fr(val) => taffy::MaxTrackSizingFunction::from_fr(*val),
        st::TrackBreadth::Auto => taffy::MaxTrackSizingFunction::AUTO,
        st::TrackBreadth::MinContent => taffy::MaxTrackSizingFunction::MIN_CONTENT,
        st::TrackBreadth::MaxContent => taffy::MaxTrackSizingFunction::MAX_CONTENT,
    }
}
