//! A sync keeps the rows folds hide up to date across edits: it gives the
//! map a fresh one of the same snapshot, width and folds would have.

use std::ops::Range;

use fe_buffer::{Buffer, Origin};
use fe_display::{DisplayMap, Monospace};
use rand::{Rng, SeedableRng, rngs::StdRng};

const WORDS: [&str; 4] = ["ab", "cdef", "g", "hijklmnop"];

fn words(rng: &mut StdRng, max: usize) -> String {
    let count = rng.random_range(0..=max);
    let words: Vec<&str> = (0..count)
        .map(|_| WORDS[rng.random_range(0..WORDS.len())])
        .collect();
    words.join(" ")
}

/// The byte range of `row`, without its line break.
fn row_range(buffer: &Buffer, row: u32) -> Range<usize> {
    let start = buffer.text().row_start(row);
    start..start + buffer.text().row_len(row) as usize
}

/// An offset somewhere in `row`, its ends included.
fn offset_in(rng: &mut StdRng, buffer: &Buffer, row: u32) -> usize {
    let range = row_range(buffer, row);
    let offset = rng.random_range(range.start..=range.end);
    buffer.text().clip_offset(offset, fe_text::Bias::Left)
}

fn random_row(rng: &mut StdRng, buffer: &Buffer) -> u32 {
    rng.random_range(0..buffer.text().row_count())
}

/// One or more edits, of a kind folds care about.
fn random_edits(rng: &mut StdRng, buffer: &mut Buffer, map: &DisplayMap) {
    let fold =
        (!map.folds().is_empty()).then(|| map.folds()[rng.random_range(0..map.folds().len())]);
    let edit: (Range<usize>, String) = match (rng.random_range(0..9), fold) {
        // Typing on a fold's header: same rows, perhaps more wrap rows.
        (0 | 1, Some(fold)) => {
            let at = offset_in(rng, buffer, fold.start);
            (at..at, words(rng, 3) + " ")
        }
        // A newline on a header.
        (2, Some(fold)) => {
            let at = offset_in(rng, buffer, fold.start);
            (at..at, "\n".into())
        }
        // Inside a fold.
        (3, Some(fold)) => {
            let row = rng.random_range(fold.start + 1..=fold.end);
            let at = offset_in(rng, buffer, row);
            let end = offset_in(rng, buffer, row);
            (at.min(end)..at.max(end), words(rng, 2))
        }
        // At the end of the file.
        (4, _) => {
            let len = buffer.len();
            let start = buffer.text().clip_offset(
                len.saturating_sub(rng.random_range(0..20)),
                fe_text::Bias::Left,
            );
            let insert = if rng.random_bool(0.5) {
                "\nab cdef"
            } else {
                ""
            };
            (start..len, insert.into())
        }
        // Whole rows inserted or deleted.
        (5, _) => {
            let row = random_row(rng, buffer);
            let at = buffer.text().row_start(row);
            if rng.random_bool(0.5) {
                let rows = rng.random_range(1..4);
                let insert: String = (0..rows).map(|_| words(rng, 4) + "\n").collect();
                (at..at, insert)
            } else {
                let end_row = (row + rng.random_range(1..4)).min(buffer.text().row_count() - 1);
                (at..buffer.text().row_start(end_row), String::new())
            }
        }
        // Several ranges in one edit.
        (6, _) => {
            let mut rows: Vec<u32> = (0..rng.random_range(2..6))
                .map(|_| random_row(rng, buffer))
                .collect();
            rows.sort_unstable();
            rows.dedup();
            let edits: Vec<(Range<usize>, String)> = rows
                .into_iter()
                .map(|row| {
                    let at = offset_in(rng, buffer, row);
                    let insert = match rng.random_range(0..3) {
                        0 => "\n".to_string(),
                        1 => words(rng, 3),
                        _ => format!("{}\n{}", words(rng, 2), words(rng, 2)),
                    };
                    (at..at, insert)
                })
                .collect();
            buffer.edit(edits, Origin::User);
            return;
        }
        // Two edits before the sync. Their rows merge into one edit
        // spanning both, which drops the folds between them, so they're
        // near each other.
        (7, _) => {
            let first = random_row(rng, buffer);
            for _ in 0..2 {
                let row = (first + rng.random_range(0..4)).min(buffer.text().row_count() - 1);
                let at = offset_in(rng, buffer, row);
                let insert = if rng.random_bool(0.5) { "\n" } else { "cdef " };
                buffer.edit([(at..at, insert)], Origin::User);
            }
            return;
        }
        // Anywhere, between folds or not.
        _ => {
            let row = random_row(rng, buffer);
            let at = offset_in(rng, buffer, row);
            let end = buffer.text().clip_offset(
                (at + rng.random_range(0..12)).min(buffer.len()),
                fe_text::Bias::Left,
            );
            let insert = ["", "x", "ab cdef ", "\n", "g\ng"][rng.random_range(0..5)];
            (at..end, insert.into())
        }
    };
    buffer.edit([edit], Origin::User);
}

fn assert_matches_a_fresh_map(map: &DisplayMap, buffer: &Buffer, width: Option<f32>, round: usize) {
    let mut fresh = DisplayMap::new(buffer.current(), 4);
    fresh.set_wrap_width(width, &mut Monospace);
    fresh.wrap_pending(0, usize::MAX, &mut Monospace);
    fresh.fold_many(map.folds().iter().map(|fold| fold.start..=fold.end));

    assert_eq!(map.folds(), fresh.folds(), "round {round}");
    assert_eq!(map.row_count(), fresh.row_count(), "round {round}");
    for row in 0..map.row_count() {
        assert_eq!(
            map.row(row),
            fresh.row(row),
            "round {round}, display row {row}"
        );
    }
    for row in 0..buffer.text().row_count() {
        assert_eq!(
            map.hidden_rows(row),
            fresh.hidden_rows(row),
            "round {round}, buffer row {row}"
        );
        assert_eq!(
            map.row_for_buffer_row(row),
            fresh.row_for_buffer_row(row),
            "round {round}, buffer row {row}"
        );
    }
}

fn random_edits_match_a_fresh_map(width: Option<f32>) {
    let mut rng = StdRng::seed_from_u64(181);
    let text: String = (0..4_000).map(|_| words(&mut rng, 6) + "\n").collect();
    let mut buffer = Buffer::new(&text);
    let mut map = DisplayMap::new(buffer.current(), 4);
    map.set_wrap_width(width, &mut Monospace);
    map.wrap_pending(0, usize::MAX, &mut Monospace);

    // Short and long folds, nesting in and overlapping each other.
    map.fold_many((0..300).map(|_| {
        let start = rng.random_range(0..3_990);
        let len = if rng.random_bool(0.8) {
            rng.random_range(1..8)
        } else {
            rng.random_range(8..200)
        };
        start..=start + len
    }));
    assert!(map.folds().len() > 250);
    assert_matches_a_fresh_map(&map, &buffer, width, 0);

    for round in 1..=200 {
        random_edits(&mut rng, &mut buffer, &map);
        map.sync(buffer.current(), &mut Monospace);
        if round % 5 == 0 || round == 200 {
            assert_matches_a_fresh_map(&map, &buffer, width, round);
        }
    }
    assert!(map.folds().len() > 100, "{} folds left", map.folds().len());
}

#[test]
fn folds_hidden_rows_after_random_edits_match_a_fresh_map() {
    // The same rounds wrapped and unwrapped, side by side.
    std::thread::scope(|scope| {
        let wrapped = scope.spawn(|| random_edits_match_a_fresh_map(Some(8.)));
        random_edits_match_a_fresh_map(None);
        wrapped.join().unwrap();
    });
}
