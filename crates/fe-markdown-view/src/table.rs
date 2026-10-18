// Adapted from longbridge/gpui-kit@2154ba8, crates/base/src/text/node.rs; Apache-2.0.
// Commit 2154ba84afd2e771f117fd17e55c060dd5e03fd4.
// `render_table`, `render_table_rows` and `TableColumn`. Changed: cells are
// fe-markdown's inlines, measured by shaping their text and sizing their
// images each frame rather than through a cache; the table is as wide as its content up to the
// column and never scrolls, so its columns stop shrinking at their wrap
// floors and clip; there's no table style refinement, corner rounding,
// action row or streaming fade; cell padding is VS Code's preview's; the
// header row is bold, and every other body row is tinted.

//! Tables: a bold header row, cells aligned per column, 1px borders and
//! every other body row tinted.

use fe_markdown::{Align, Cell, Row};
use gpui::{
    AnyElement, Div, FontWeight, IntoElement, ParentElement as _, Styled as _, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    inlines::{self, Base, Inlines},
    view::RenderCx,
};

/// A cell's horizontal padding, both sides, and its vertical padding, as
/// VS Code's preview's.
const CELL_PAD_PX: f32 = 20.0;
const CELL_PAD_Y_PX: f32 = 5.0;
/// The divider every column but the last draws on its right.
const CELL_BORDER_PX: f32 = 1.0;
/// The frame's border, left and right.
const TABLE_BORDER_PX: f32 = 2.0;
/// The narrowest a column wraps down to, unless its content is narrower.
const WRAP_MIN_PX: f32 = 64.0;

/// A table column's cell widths for layout, excluding the divider after it.
struct TableColumn {
    /// The width that fits the content on its widest line.
    max: f32,
    /// The width below which the column stops shrinking.
    min: f32,
    divider: f32,
}

/// A table with `head` as its header row and `rows` under it, each column
/// aligned as `align` says.
///
/// - As wide as its content, if the column has room: each column fits its
///   widest cell on one line.
/// - Narrower: the narrow columns keep their full content width and only
///   the widest ones shrink and wrap, all down to one common width (see
///   [`render_rows`]). Shrinking every column by the same ratio would wrap
///   the last word of a column that nearly fits onto a line of its own.
/// - Narrower than the columns' floors: the table clips.
pub(crate) fn render(
    align: &[Align],
    head: &[Cell],
    rows: &[Row],
    cx: &mut RenderCx,
) -> AnyElement {
    let style = cx.style;
    let header = Base {
        weight: FontWeight::BOLD,
        color: style.foreground,
    };
    let body = Base {
        weight: FontWeight::NORMAL,
        color: style.foreground,
    };
    let mut grid: Vec<Vec<Inlines>> = Vec::with_capacity(rows.len() + 1);
    grid.push(
        head.iter()
            .map(|cell| inlines::layout(cell, header, cx))
            .collect(),
    );
    for row in rows {
        grid.push(
            row.cells
                .iter()
                .map(|cell| inlines::layout(cell, body, cx))
                .collect(),
        );
    }
    let col_count = grid
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
        .max(align.len());
    let col_w = measure_columns(&grid, col_count, cx);

    // Lengths are rounded to device pixels for layout. Round the content
    // widths up, or a column can lose the fraction of a pixel that keeps
    // its text on one line.
    let scale_factor = cx.window.scale_factor();
    let device_ceil = |width: f32| (width * scale_factor).ceil() / scale_factor;
    let columns: Vec<TableColumn> = (col_w.iter().enumerate())
        .map(|(ix, &w)| {
            let floor = w.min(WRAP_MIN_PX);
            let divider = if ix + 1 < col_count {
                CELL_BORDER_PX
            } else {
                0.
            };
            let max = device_ceil(w - divider);
            TableColumn {
                min: device_ceil(floor - divider).min(max),
                max,
                divider,
            }
        })
        .collect();
    let total_w = columns.iter().map(|c| c.max + c.divider).sum::<f32>() + TABLE_BORDER_PX;
    let rows = render_rows(grid, align, &columns, cx);
    div()
        .w_full()
        .max_w(px(total_w))
        .overflow_hidden()
        .border_1()
        .border_color(style.border)
        .children(rows)
        .into_any_element()
}

/// The max-content width of every table column: the widest cell's
/// widest line, its text shaped with the runs it's drawn with and its
/// images at the size they're drawn at, plus the cell's padding and
/// divider.
fn measure_columns(grid: &[Vec<Inlines>], col_count: usize, cx: &mut RenderCx) -> Vec<f32> {
    let mut col_w = vec![CELL_PAD_PX; col_count];
    for row in grid {
        for (ix, cell) in row.iter().enumerate() {
            let width = inlines::max_width(cell, cx);
            let border = if ix + 1 < col_count {
                CELL_BORDER_PX
            } else {
                0.
            };
            col_w[ix] = col_w[ix].max(width + CELL_PAD_PX + border);
        }
    }
    col_w
}

/// The table rows, each a flex row laid out on the same column grid.
///
/// Every column is a cell plus slack beside it. A cell grows from zero up
/// to its content width `max` and never below its floor `min`: flex layout
/// grows all cells equally and freezes each one that reaches its content
/// width, so the narrow columns fit their content and the widest ones share
/// what is left. The slack grows only after every cell is frozen, so a
/// frame wider than the content spreads the extra width in proportion to
/// the columns, placed after, around or before the cell to follow the
/// column alignment.
///
/// A row with fewer cells than the table keeps empty cells in their place,
/// so its columns line up with the other rows.
fn render_rows(
    grid: Vec<Vec<Inlines>>,
    align: &[Align],
    columns: &[TableColumn],
    cx: &mut RenderCx,
) -> Vec<AnyElement> {
    // The slack's share of the frame while any cell is still growing is
    // `SLACK_GROW / (cells * CELL_GROW)`, a negligible fraction of a
    // pixel. The slack factors still add up to `SLACK_GROW`: flex layout
    // hands out only that fraction of the free space when the factors sum
    // to less than one, and the slack must take all of it.
    const CELL_GROW: f32 = 1e6;
    const SLACK_GROW: f32 = 1.0;

    let style = cx.style;
    let col_count = columns.len();
    let total_w = columns.iter().map(|column| column.max).sum::<f32>().max(1.);
    let slack = |grow: f32| -> Div {
        div()
            .flex_basis(px(0.))
            .flex_grow(grow)
            .flex_shrink(0.)
            .border_color(style.border)
    };

    let row_count = grid.len();
    let mut rows = Vec::with_capacity(row_count);
    for (row_ix, row) in grid.into_iter().enumerate() {
        let mut row = row.into_iter();
        let mut cells = Vec::with_capacity(col_count * 2);
        for (ix, column) in columns.iter().enumerate() {
            let is_last_col = ix + 1 == col_count;
            let align = align.get(ix).copied().unwrap_or(Align::None);
            let content = row.next().map(|cell| inlines::render(cell, cx));

            let grow = SLACK_GROW * column.max / total_w;
            let (lead, trail) = match align {
                Align::None | Align::Left => (0., grow),
                Align::Center => (grow / 2., grow / 2.),
                Align::Right => (grow, 0.),
            };
            if lead > 0. {
                cells.push(slack(lead).into_any_element());
            }
            cells.push(
                div()
                    .flex()
                    .flex_col()
                    .flex_basis(px(0.))
                    .flex_grow(CELL_GROW)
                    .min_w(px(column.min))
                    .max_w(px(column.max))
                    .overflow_hidden()
                    // A text aligns its lines, and a flow's lines align as
                    // boxes.
                    .when(align == Align::Center, |this| {
                        this.text_center().items_center()
                    })
                    .when(align == Align::Right, |this| this.text_right().items_end())
                    .px(px(CELL_PAD_PX / 2.))
                    .py(px(CELL_PAD_Y_PX))
                    .children(content.into_iter().flatten())
                    .into_any_element(),
            );
            if trail > 0. || !is_last_col {
                cells.push(
                    slack(trail)
                        .when(!is_last_col, |this| this.border_r_1())
                        .into_any_element(),
                );
            }
        }
        rows.push(
            div()
                .w_full()
                .flex()
                .flex_row()
                .when(row_ix < row_count - 1, |this| this.border_b_1())
                .border_color(style.border)
                // The header is row 0, so the even rows are every other
                // body row.
                .when(row_ix > 0 && row_ix % 2 == 0, |this| {
                    this.bg(style.row_tint)
                })
                .children(cells)
                .into_any_element(),
        );
    }
    rows
}
