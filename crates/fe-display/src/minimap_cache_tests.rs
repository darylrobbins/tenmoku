//! [`RowRunsCache`] following a display map through edits, folds and soft
//! wrap. Each row's runs are tagged with the row's text, so a cached row is
//! right when its tag is the text that row shows now.

use fe_buffer::{Buffer, Origin};

use super::*;
use crate::Monospace;

/// One run per row, coloured by the row's text.
fn runs_of(display: &DisplayMap, row: u32) -> Vec<MinimapRun<String>> {
    let range = display.row(row).range;
    let text = display.text().slice_to_string(range);
    vec![MinimapRun {
        columns: 0..1,
        color: text,
    }]
}

/// Caches every row of `display`.
fn fill(cache: &mut RowRunsCache<String>, display: &DisplayMap) {
    assert_eq!(cache.sync(display), Synced::Cleared, "the first sync");
    for row in 0..display.row_count() {
        cache.insert(display, row, runs_of(display, row), 0);
    }
}

/// The rows cached, after checking each still holds its row's runs.
#[track_caller]
fn cached(cache: &RowRunsCache<String>, display: &DisplayMap) -> Vec<u32> {
    let rows: Vec<u32> = (0..display.row_count())
        .filter(|&row| cache.row(row).is_some())
        .collect();
    for &row in &rows {
        let cached = cache.row(row).unwrap();
        if !cached.stale {
            assert_eq!(cached.runs, runs_of(display, row), "row {row}");
        }
    }
    rows
}

fn numbered(rows: usize) -> String {
    (0..rows).map(|n| format!("row {n}\n")).collect()
}

fn edit(buffer: &mut Buffer, display: &mut DisplayMap, edits: Vec<(Range<usize>, &str)>) {
    buffer.edit(edits, Origin::User);
    display.sync(&buffer.snapshot(), &mut Monospace);
}

fn offset(buffer: &Buffer, needle: &str) -> usize {
    buffer.snapshot().text().to_string().find(needle).unwrap()
}

#[test]
fn inserting_and_deleting_rows_moves_the_other_rows_runs() {
    let mut buffer = Buffer::new(&numbered(10));
    let mut display = DisplayMap::new(&buffer.snapshot(), 4);
    let mut cache = RowRunsCache::new(100);
    fill(&mut cache, &display);
    let ids: Vec<u64> = (0..10).map(|row| cache.row(row).unwrap().id).collect();

    // Two rows typed at the start of row 4, before its text.
    let at = offset(&buffer, "row 4");
    edit(&mut buffer, &mut display, vec![(at..at, "new\nnew\n")]);
    assert_eq!(cache.sync(&display), Synced::Spliced);
    // Rows 4 to 6 are the edit's; old rows 5 on are rows 7 on.
    let expected: Vec<u32> = (0..display.row_count())
        .filter(|row| !(4..7).contains(row))
        .collect();
    assert_eq!(cached(&cache, &display), expected);
    assert_eq!(cache.row(7).unwrap().id, ids[5], "moved, not rebuilt");
    assert_eq!(cache.row(3).unwrap().id, ids[3]);

    // Deleting them (and row 4's text) moves the rows after back up.
    let (start, end) = (offset(&buffer, "new"), offset(&buffer, "row 5"));
    edit(&mut buffer, &mut display, vec![(start..end, "")]);
    assert_eq!(cache.sync(&display), Synced::Spliced);
    assert_eq!(display.row_count(), 10);
    assert_eq!(cached(&cache, &display), [0, 1, 2, 3, 5, 6, 7, 8, 9]);
    assert_eq!(cache.row(5).unwrap().id, ids[6]);
    assert_eq!(cache.sync(&display), Synced::Unchanged);
}

#[test]
fn an_edit_drops_only_the_rows_it_touches() {
    let mut buffer = Buffer::new(&numbered(10));
    let mut display = DisplayMap::new(&buffer.snapshot(), 4);
    let mut cache = RowRunsCache::new(100);
    fill(&mut cache, &display);

    // A character typed on row 3, and carets on rows 6 and 8 in one batch.
    let at = offset(&buffer, "row 3") + 3;
    edit(&mut buffer, &mut display, vec![(at..at, "x")]);
    assert_eq!(cache.sync(&display), Synced::Spliced);
    assert_eq!(cached(&cache, &display), [0, 1, 2, 4, 5, 6, 7, 8, 9, 10]);
    let (six, eight) = (offset(&buffer, "row 6"), offset(&buffer, "row 8"));
    edit(
        &mut buffer,
        &mut display,
        vec![(six..six, "y\n"), (eight..eight + 1, "z")],
    );
    assert_eq!(cache.sync(&display), Synced::Spliced);
    // Row 6 split in two (6, 7); old row 8 is row 9.
    assert_eq!(cached(&cache, &display), [0, 1, 2, 4, 5, 8, 10, 11]);

    // Soft wrapped, an edit's rows are what its buffer row wraps to.
    display.set_wrap_width(Some(4.), &mut Monospace);
    while display.wrap_pending(0, usize::MAX, &mut Monospace) {}
    cache.clear();
    cache.sync(&display);
    for row in 0..display.row_count() {
        cache.insert(&display, row, runs_of(&display, row), 0);
    }
    let rows = display.row_count();
    let at = offset(&buffer, "row 2") + 3;
    edit(
        &mut buffer,
        &mut display,
        vec![(at..at, " grows to three rows")],
    );
    assert_eq!(cache.sync(&display), Synced::Spliced);
    let edited = display.row_for_buffer_row(2)..display.row_for_buffer_row(3);
    assert!(display.row_count() > rows);
    let expected: Vec<u32> = (0..display.row_count())
        .filter(|row| !edited.contains(row))
        .collect();
    assert_eq!(cached(&cache, &display), expected);
}

#[test]
fn invalidating_a_range_keeps_its_runs_stale() {
    let buffer = Buffer::new(&numbered(10));
    let mut display = DisplayMap::new(&buffer.snapshot(), 4);
    let mut cache = RowRunsCache::new(100);
    fill(&mut cache, &display);
    cache.invalidate(2..5);
    let stale: Vec<u32> = (0..10)
        .filter(|&row| cache.row(row).unwrap().stale)
        .collect();
    assert_eq!(stale, [2, 3, 4]);
    // Rebuilt the same, a row keeps its id; rebuilt otherwise, it gets a
    // new one.
    let (two, three) = (cache.row(2).unwrap().id, cache.row(3).unwrap().id);
    assert_eq!(cache.insert(&display, 2, runs_of(&display, 2), 0), two);
    assert!(!cache.row(2).unwrap().stale);
    assert_ne!(cache.insert(&display, 3, runs_of(&display, 4), 0), three);

    // Folds at once, other than one made or undone, move rows in ways an
    // edit log doesn't say: every row goes stale, and drawn rows are
    // rebuilt and compared.
    display.fold_many([1..=2, 5..=7]);
    assert_eq!(cache.sync(&display), Synced::Invalidated);
    assert!((0..7).all(|row| cache.row(row).unwrap().stale));
    // Another buffer has nothing in common.
    let other = Buffer::new(&numbered(10));
    let display = DisplayMap::new(&other.snapshot(), 4);
    assert_eq!(cache.sync(&display), Synced::Cleared);
    assert!(cache.is_empty());
}

#[test]
fn the_cache_keeps_at_most_its_capacity_around_the_rows_shown() {
    let buffer = Buffer::new(&numbered(10_000));
    let display = DisplayMap::new(&buffer.snapshot(), 4);
    let mut cache = RowRunsCache::new(1000);
    cache.sync(&display);
    for row in 0..3000 {
        cache.insert(&display, row, runs_of(&display, row), 0);
        assert!(cache.len() <= 1000);
    }
    // Inserting kept the rows nearest the last.
    assert!(cache.row(2999).is_some() && cache.row(1999).is_none());
    cache.retain_around(2900..3000);
    assert!(cache.len() <= 1000);
    // Shown far away, the rows cached go.
    cache.retain_around(9000..9100);
    assert!(cache.is_empty());
    for row in (9000..9100).chain(8500..8510) {
        cache.insert(&display, row, runs_of(&display, row), 0);
    }
    assert_eq!(cache.len(), 110);
    cache.retain_around(9000..9100);
    assert_eq!(cache.len(), 100, "8500 is beyond the capacity around 9000");
    assert_eq!(cached(&cache, &display).len(), 100);
}

#[test]
fn a_fold_made_or_undone_moves_the_rows_after_it() {
    let buffer = Buffer::new(&numbered(10));
    let mut display = DisplayMap::new(&buffer.snapshot(), 4);
    let mut cache = RowRunsCache::new(100);
    fill(&mut cache, &display);
    let nine = cache.row(9).unwrap().id;
    display.fold(3..=6);
    assert_eq!(cache.sync(&display), Synced::Spliced);
    // Rows 4 to 6 are hidden; old row 9 is row 6.
    assert_eq!(cached(&cache, &display), [0, 1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(cache.row(6).unwrap().id, nine);
    display.unfold(3..=3);
    assert_eq!(cache.sync(&display), Synced::Spliced);
    assert_eq!(cached(&cache, &display), [0, 1, 2, 3, 7, 8, 9, 10]);
    assert_eq!(cache.row(9).unwrap().id, nine);
}

#[test]
fn an_edit_on_a_folds_header_drops_the_header_with_wrap_or_several_batches() {
    let mut buffer = Buffer::new(&numbered(10));
    let mut display = DisplayMap::new(&buffer.snapshot(), 4);
    display.fold(3..=6);
    let mut cache = RowRunsCache::new(100);
    fill(&mut cache, &display);
    // Two batches between syncs merge into one splice.
    let at = offset(&buffer, "row 3") + 5;
    edit(&mut buffer, &mut display, vec![(at..at, "x")]);
    edit(&mut buffer, &mut display, vec![(at + 1..at + 1, "y")]);
    assert_eq!(display.folds(), [Fold { start: 3, end: 6 }]);
    assert_eq!(cache.sync(&display), Synced::Spliced);
    assert_eq!(cached(&cache, &display), [0, 1, 2, 4, 5, 6, 7]);

    // Soft wrapped, the header's rows are its last display row's too.
    display.set_wrap_width(Some(4.), &mut Monospace);
    while display.wrap_pending(0, usize::MAX, &mut Monospace) {}
    cache.clear();
    cache.sync(&display);
    for row in 0..display.row_count() {
        cache.insert(&display, row, runs_of(&display, row), 0);
    }
    let header = display.row_for_buffer_row(3)..display.row_for_buffer_row(7);
    let at = offset(&buffer, "row 3xy") + 7;
    edit(&mut buffer, &mut display, vec![(at..at, " and more")]);
    assert_eq!(display.folds(), [Fold { start: 3, end: 6 }]);
    assert_eq!(cache.sync(&display), Synced::Spliced);
    let header = header.start..display.row_for_buffer_row(7);
    let expected: Vec<u32> = (0..display.row_count())
        .filter(|row| !header.contains(row))
        .collect();
    assert_eq!(cached(&cache, &display), expected);
}

/// Each cached row's id, by row.
fn ids(cache: &RowRunsCache<String>, display: &DisplayMap) -> Vec<Option<u64>> {
    (0..display.row_count())
        .map(|row| cache.row(row).map(|cached| cached.id))
        .collect()
}

#[test]
fn disjoint_edits_drop_only_their_rows_with_wrap_and_over_several_batches() {
    let mut buffer = Buffer::new(&numbered(300));
    let mut display = DisplayMap::new(&buffer.snapshot(), 4);
    // Wide enough that no row wraps but the edited ones.
    display.set_wrap_width(Some(12.), &mut Monospace);
    while display.wrap_pending(0, usize::MAX, &mut Monospace) {}
    let mut cache = RowRunsCache::new(1000);
    fill(&mut cache, &display);
    let before = ids(&cache, &display);

    // One batch at rows 66 and 260, which now wraps to two rows.
    let (a, b) = (
        offset(&buffer, "row 66\n") + 3,
        offset(&buffer, "row 260\n") + 3,
    );
    edit(
        &mut buffer,
        &mut display,
        vec![(a..a, "x"), (b..b, " longer still")],
    );
    assert_eq!(display.row_count(), 302);
    assert_eq!(cache.sync(&display), Synced::Spliced);
    let after = ids(&cache, &display);
    let dropped: Vec<usize> = (0..after.len())
        .filter(|&row| after[row].is_none())
        .collect();
    assert_eq!(dropped, [66, 260, 261]);
    assert_eq!(after[..66], before[..66]);
    assert_eq!(
        after[67..260],
        before[67..260],
        "untouched rows keep their ids"
    );
    assert_eq!(after[262..], before[261..]);
    cached(&cache, &display);

    // Several batches between syncs, each at its own row, one inserting a
    // row before the others.
    let before = after;
    let (c, d) = (
        offset(&buffer, "row 10\n"),
        offset(&buffer, "row 200\n") + 3,
    );
    edit(&mut buffer, &mut display, vec![(d..d, "y")]);
    edit(&mut buffer, &mut display, vec![(c..c, "new\n")]);
    assert_eq!(cache.sync(&display), Synced::Spliced);
    let after = ids(&cache, &display);
    let dropped: Vec<usize> = (0..after.len())
        .filter(|&row| after[row].is_none())
        .collect();
    assert_eq!(dropped, [10, 11, 67, 201, 261, 262]);
    assert_eq!(after[12..67], before[11..66]);
    assert_eq!(after[68..201], before[67..200]);
    assert_eq!(after[202..261], before[201..260]);
    cached(&cache, &display);
}
