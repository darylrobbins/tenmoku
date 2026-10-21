//! Rows are 20 px. The text nests `fn f` (rows 2-7) in `impl S` (1-8) in
//! `mod m` (0-9), then has `fn g` (10-20) and filler to row 29.

use std::ops::RangeInclusive;

use fe_buffer::Buffer;

use super::*;
use crate::{Block, Monospace};

const LH: f32 = 20.;

const SCOPES: [RangeInclusive<u32>; 4] = [0..=9, 1..=8, 2..=7, 10..=20];

fn text() -> String {
    let mut text = String::from(
        "mod m {\n    impl S {\n        fn f() {\n            a\n            b\n            c\n            \
         d\n        }\n    }\n}\nfn g() {\n",
    );
    text.push_str(&"    x\n".repeat(9));
    text.push_str("}\n");
    text.push_str(&"z\n".repeat(9));
    text
}

fn map(text: &str) -> DisplayMap {
    let buffer = Buffer::new(text);
    DisplayMap::new(buffer.current(), 4)
}

/// The pinned rows as `(buffer_row, y)`.
fn pinned(
    display: &DisplayMap,
    blocks: &BlockMap,
    scroll_y: f32,
    max_rows: usize,
    keep_above: Option<f32>,
) -> Vec<(u32, f32)> {
    let rows = sticky_rows(&SCOPES, display, blocks, scroll_y, LH, max_rows, keep_above);
    rows.iter().map(|row| (row.buffer_row, row.y)).collect()
}

fn plain(display: &DisplayMap, scroll_y: f32) -> Vec<(u32, f32)> {
    pinned(display, &BlockMap::default(), scroll_y, 5, None)
}

#[test]
fn no_band_at_the_top_or_with_a_header_exactly_at_the_top() {
    let display = map(&text());
    assert_eq!(plain(&display, 0.), []);
    // fn g's header is the top row.
    assert_eq!(plain(&display, 10. * LH), []);
    assert_eq!(plain(&display, 10. * LH + 1.), [(10, 0.)]);
    // Past g's last row.
    assert_eq!(plain(&display, 21. * LH), []);
}

#[test]
fn a_three_deep_chain_is_pinned() {
    let display = map(&text());
    let chain = [(0, 0.), (1, LH), (2, 2. * LH)];
    assert_eq!(plain(&display, 3. * LH), chain);
    assert_eq!(plain(&display, 3. * LH + 5.), chain);
    // f's header at the top is under the band's first two rows.
    assert_eq!(plain(&display, 2. * LH), chain);
    // Scrolled a pixel, each header is above its slot: m covers impl's,
    // impl f's.
    assert_eq!(plain(&display, 1.), chain);
}

#[test]
fn the_innermost_row_is_pushed_up_by_the_overlap_as_its_last_row_scrolls_under() {
    let display = map(&text());
    // f's last row, 7, ends at 160 px: 55 px into the view at 105, 5 px
    // above the third slot's bottom.
    assert_eq!(plain(&display, 105.), [(0, 0.), (1, LH), (2, 35.)]);
    assert_eq!(plain(&display, 100.), [(0, 0.), (1, LH), (2, 2. * LH)]);
    assert_eq!(plain(&display, 119.), [(0, 0.), (1, LH), (2, 21.)]);
    // Gone once its bottom reaches the slot's top.
    assert_eq!(plain(&display, 120.), [(0, 0.), (1, LH)]);
    // Then impl, whose last row ends at 180 px, and m, at 200.
    assert_eq!(plain(&display, 150.), [(0, 0.), (1, 10.)]);
    assert_eq!(plain(&display, 190.), [(0, -10.)]);
    assert_eq!(plain(&display, 200.), []);
}

#[test]
fn max_rows_caps_the_band() {
    let display = map(&text());
    let blocks = BlockMap::default();
    assert_eq!(
        pinned(&display, &blocks, 3. * LH, 2, None),
        [(0, 0.), (1, LH)]
    );
    assert_eq!(pinned(&display, &blocks, 3. * LH, 1, None), [(0, 0.)]);
    assert_eq!(pinned(&display, &blocks, 3. * LH, 0, None), []);
    // The capped innermost row is pushed by its own last row.
    assert_eq!(
        pinned(&display, &blocks, 150., 2, None),
        [(0, 0.), (1, 10.)]
    );
}

#[test]
fn a_header_inside_a_closed_fold_is_skipped() {
    let mut display = map(&text());
    // Hides f's header, row 2, behind impl's: buffer rows from 3 show a
    // display row higher.
    assert!(display.fold(1..=2));
    assert_eq!(plain(&display, 2. * LH), [(0, 0.), (1, LH)]);
    // f's last row, 7, now ends at 140 px; impl's, 8, at 160.
    assert_eq!(plain(&display, 130.), [(0, 0.), (1, 10.)]);
    // A folded scope's last row is its header: pinned only while that's
    // under its slot, where it already is.
    display.unfold_all();
    assert!(display.fold(2..=7));
    assert_eq!(plain(&display, 10.), [(0, 0.), (1, LH), (2, 30.)]);
    assert_eq!(plain(&display, 20.), [(0, 0.), (1, LH)]);
}

#[test]
fn a_wrapped_header_takes_one_slot() {
    let mut display = map(&text().replace("impl S {", "impl SSSSSSSSSSSS {"));
    display.set_wrap_width(Some(20.), &mut Monospace);
    assert!(!display.wrap_pending(0, usize::MAX, &mut Monospace));
    // impl's header shows on display rows 1 and 2, and f's on 3.
    assert_eq!(display.row_for_buffer_row(2), 3);
    let chain = [(0, 0.), (1, LH), (2, 2. * LH)];
    assert_eq!(plain(&display, 4. * LH), chain);
    assert_eq!(plain(&display, 2. * LH), chain);
    // impl's header starts on its first row, under the first slot, and f's
    // is at the third slot's top.
    assert_eq!(plain(&display, LH), [(0, 0.), (1, LH)]);
}

#[test]
fn block_rows_count() {
    let display = map(&text());
    // A code lens above m's header: m's header is at the top at 20 px.
    let lens = BlockMap::new([Block {
        above: 0,
        rows: 1,
        id: 1,
    }]);
    assert_eq!(pinned(&display, &lens, LH, 5, None), []);
    assert_eq!(
        pinned(&display, &lens, LH + 1., 5, None),
        [(0, 0.), (1, LH), (2, 2. * LH)]
    );
    // A code lens above f's header: f's last row ends at 180 px, not 160,
    // so it stays pinned at 125 px, pushed up by 5.
    let lens = BlockMap::new([Block {
        above: 2,
        rows: 1,
        id: 1,
    }]);
    assert_eq!(
        pinned(&display, &lens, 125., 5, None),
        [(0, 0.), (1, LH), (2, 35.)]
    );
    assert_eq!(plain(&display, 125.), [(0, 0.), (1, LH)]);
}

#[test]
fn keep_above_drops_rows_at_or_below_it() {
    let display = map(&text());
    let blocks = BlockMap::default();
    let chain = [(0, 0.), (1, LH), (2, 2. * LH)];
    assert_eq!(pinned(&display, &blocks, 3. * LH, 5, Some(3. * LH)), chain);
    assert_eq!(
        pinned(&display, &blocks, 3. * LH, 5, Some(2. * LH)),
        chain[..2]
    );
    assert_eq!(
        pinned(&display, &blocks, 3. * LH, 5, Some(2. * LH - 1.)),
        chain[..1]
    );
    assert_eq!(pinned(&display, &blocks, 3. * LH, 5, Some(0.)), []);
    // A pushed row's bottom is higher.
    assert_eq!(
        pinned(&display, &blocks, 105., 5, Some(55.)),
        [(0, 0.), (1, LH), (2, 35.)]
    );
}
