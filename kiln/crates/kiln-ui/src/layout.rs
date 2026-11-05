// SPDX-License-Identifier: Apache-2.0

//! Shorthands for the two flex layouts most views are built from. Each is
//! a plain `div`, so everything GPUI's `div` takes still applies.

use gpui::{Div, Styled as _, div};

/// A `div` laid out as a column: children top to bottom.
pub fn v_flex() -> Div {
    div().flex().flex_col()
}

/// A `div` laid out as a row: children left to right.
pub fn h_flex() -> Div {
    div().flex().flex_row()
}
