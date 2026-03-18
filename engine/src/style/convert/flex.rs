//! Flexbox property converters from Stylo to Taffy.

use super::length::dimension;
use super::stylo_types as st;
use taffy::prelude::*;

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
        st::FlexBasis::Content => taffy::Dimension::AUTO,
        st::FlexBasis::Size(size) => dimension(size),
    }
}
