use fe_buffer::{Buffer, Origin};
use proptest::prelude::*;

use super::*;
use crate::Monospace;

fn rows(map: &DisplayMap) -> Vec<DisplayRow> {
    (0..map.row_count()).map(|row| map.row(row)).collect()
}

/// Rows tile the visible text in order, and every buffer offset maps to the
/// row showing it, or if folded away to its fold's header.
fn check_round_trips(map: &DisplayMap) -> Result<(), TestCaseError> {
    let text = map.text();
    let rows = rows(map);
    prop_assert_eq!(rows[0].range.start, 0);
    let last = rows.last().unwrap();
    if last.folded {
        let hidden = map.hidden_rows(last.buffer_row + 1).unwrap();
        prop_assert_eq!(hidden.end, text.row_count());
    } else {
        prop_assert_eq!(last.range.end, text.len());
    }
    for pair in rows.windows(2) {
        let (row, next) = (&pair[0], &pair[1]);
        if !row.last {
            prop_assert_eq!(next.buffer_row, row.buffer_row);
            prop_assert_eq!(next.range.start, row.range.end);
        } else {
            prop_assert!(next.first);
            prop_assert_eq!(row.folded, next.buffer_row > row.buffer_row + 1);
            if !row.folded {
                prop_assert_eq!(next.range.start, row.range.end + 1);
            }
        }
    }
    for offset in (0..=text.len()).filter(|&offset| text.is_char_boundary(offset)) {
        let display_row = map.row_for_offset(offset);
        let row = &rows[display_row as usize];
        let buffer_row = text.offset_to_point(offset).row;
        if let Some(hidden) = map.hidden_rows(buffer_row) {
            prop_assert!(hidden.contains(&buffer_row));
            prop_assert!(row.folded && row.buffer_row == hidden.start - 1);
        } else {
            let at_wrap = offset == row.range.end && !row.last;
            prop_assert!(row.range.contains(&offset) || offset == row.range.end && !at_wrap);
            prop_assert_eq!(row.buffer_row, buffer_row);
        }
    }
    Ok(())
}

/// An edit at offset `at` (clamped) replacing `len` bytes with `insert`.
type EditSpec = (usize, usize, String);

fn apply(buffer: &mut Buffer, batch: &[EditSpec]) {
    let text = buffer.text();
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut end = 0;
    let mut specs = batch.to_vec();
    specs.sort_by_key(|(at, _, _)| *at);
    for (at, len, insert) in specs {
        let start = text.clip_offset(at.min(text.len()), fe_text::Bias::Left);
        if !edits.is_empty() && start < end {
            continue;
        }
        let stop = text.clip_offset((start + len).min(text.len()), fe_text::Bias::Left);
        end = stop + 1;
        edits.push((start..stop, insert));
    }
    buffer.edit(edits, Origin::User);
}

fn edit_strategy() -> impl Strategy<Value = EditSpec> {
    (0usize..80, 0usize..12, "[ab \n\t]{0,10}")
}

proptest! {
    /// Wrapping, folding and following edits (synced per batch or after
    /// several) agree with a map built from scratch, and round-trip.
    #[test]
    fn synced_maps_match_fresh_ones(
        text in "[ab \n\té]{0,80}",
        width in prop::option::of(1usize..10),
        folds in prop::collection::vec((0u32..12, 1u32..5), 0..3),
        batches in prop::collection::vec(prop::collection::vec(edit_strategy(), 1..3), 0..4),
        sync_each in any::<bool>(),
    ) {
        let width = width.map(|width| width as f32);
        let mut buffer = Buffer::new(&text);
        let mut map = DisplayMap::new(buffer.current(), 4);
        map.set_wrap_width(width, &mut Monospace);
        for (start, len) in folds {
            map.fold(start..=start + len);
        }
        check_round_trips(&map)?;
        for batch in &batches {
            apply(&mut buffer, batch);
            if sync_each {
                map.sync(buffer.current(), &mut Monospace);
            }
        }
        map.sync(buffer.current(), &mut Monospace);
        check_round_trips(&map)?;

        let mut fresh = DisplayMap::new(buffer.current(), 4);
        fresh.set_wrap_width(width, &mut Monospace);
        for fold in map.folds() {
            fresh.fold(fold.start..=fold.end);
        }
        prop_assert_eq!(rows(&map), rows(&fresh));
    }
}

#[test]
fn folds_hide_rows_and_follow_edits() {
    let mut buffer = Buffer::new("fn a() {\n    one\n    two\n}\nfn b() {}");
    let mut map = DisplayMap::new(buffer.current(), 4);
    assert!(map.fold(0..=2));
    let shown: Vec<_> = rows(&map)
        .iter()
        .map(|row| (row.buffer_row, row.folded))
        .collect();
    assert_eq!(shown, [(0, true), (3, false), (4, false)]);
    assert_eq!(map.hidden_rows(1), Some(1..3));
    assert_eq!(map.row_for_buffer_row(2), 0);
    assert_eq!(map.row_for_buffer_row(3), 1);

    // An edit above shifts the fold down.
    buffer.edit([(0..0, "// x\n")], Origin::User);
    map.sync(buffer.current(), &mut Monospace);
    assert_eq!(map.folds(), [Fold { start: 1, end: 3 }]);

    // An edit inside it unfolds it.
    let inside = buffer.text().row_start(2);
    buffer.edit([(inside..inside, "z")], Origin::User);
    map.sync(buffer.current(), &mut Monospace);
    assert_eq!(map.folds(), []);
    assert_eq!(map.row_count(), 6);
}

#[test]
fn wrapped_rows_continue_their_buffer_row() {
    let buffer = Buffer::new("aaaa bbbb cc\nd");
    let mut map = DisplayMap::new(buffer.current(), 4);
    map.set_wrap_width(Some(6.), &mut Monospace);
    let shown: Vec<_> = rows(&map)
        .iter()
        .map(|row| (row.buffer_row, row.range.clone(), row.first, row.last))
        .collect();
    assert_eq!(
        shown,
        [
            (0, 0..5, true, false),
            (0, 5..10, false, false),
            (0, 10..12, false, true),
            (1, 13..14, true, true),
        ]
    );
    // The offset at a wrap shows at the start of the next row.
    assert_eq!(map.row_for_offset(5), 1);
    assert_eq!(map.row_for_offset(12), 2);
    map.set_wrap_width(None, &mut Monospace);
    assert_eq!(map.row_count(), 2);
}
