use std::ops::Range;

use fe_buffer::{Buffer, Origin};
use rand::{Rng, SeedableRng, rngs::StdRng};

use super::super::tests::{rows, wrap_at};
use super::super::*;
use crate::Monospace;

/// The display rows of `map` as a list of wrap rows, from its folds' hidden
/// rows found with one [`WrapMap::first_row`] each.
fn per_row_projection(map: &DisplayMap) -> (Vec<Range<usize>>, Vec<usize>) {
    let hidden: Vec<Range<usize>> = map
        .folds()
        .iter()
        .map(|fold| {
            let start = map.wrap.first_row(fold.start as usize + 1);
            start..map.wrap.first_row(fold.end as usize + 1)
        })
        .collect();
    let mut hiding = vec![false; map.wrap.row_count()];
    for range in &hidden {
        hiding[range.clone()].fill(true);
    }
    let visible = (0..hiding.len()).filter(|&row| !hiding[row]).collect();
    (hidden, visible)
}

#[test]
fn batched_fold_rows_match_per_row_lookups() {
    let mut rng = StdRng::seed_from_u64(7);
    let text: String = (0..5_000)
        .map(|_| {
            let words = rng.random_range(0..12);
            let row: Vec<&str> = (0..words)
                .map(|_| ["ab", "cdef", "g"][rng.random_range(0..3)])
                .collect();
            row.join(" ") + "\n"
        })
        .collect();
    let mut buffer = Buffer::new(&text);
    let mut map = DisplayMap::new(buffer.current(), 4);
    wrap_at(&mut map, Some(8.));
    assert!(map.wrap.row_count() > 7_500);

    // Short and long folds, nesting in and overlapping each other.
    map.fold_many((0..500).map(|_| {
        let start = rng.random_range(0..4_990);
        let len = if rng.random_bool(0.8) {
            rng.random_range(1..8)
        } else {
            rng.random_range(8..400)
        };
        start..=start + len
    }));
    assert!(map.folds().len() > 475);
    assert!(!map.folds().is_sorted_by_key(|fold| fold.end));

    for _ in 0..40 {
        let len = buffer.len();
        let start = rng.random_range(0..=len);
        let start = buffer.text().clip_offset(start, fe_text::Bias::Left);
        let end = buffer.text().clip_offset(
            (start + rng.random_range(0..30)).min(len),
            fe_text::Bias::Left,
        );
        let insert =
            ["", "x", "ab cdef ", "\n", "g\ng", "abcdefgh abcdefgh\n"][rng.random_range(0..6)];
        buffer.edit([(start..end, insert)], Origin::User);
        map.sync(buffer.current(), &mut Monospace);

        let (hidden, visible) = per_row_projection(&map);
        for (fold, range) in map.folds().iter().zip(&hidden) {
            if !range.is_empty() {
                let run = map.folds.hidden_run(range.start);
                assert!(
                    run.is_some_and(|run| run.start <= range.start && range.end <= run.end),
                    "fold {fold:?} hides wrap rows {range:?}",
                );
            }
        }
        assert_eq!(map.folds.display_row_count(), visible.len());
        for (display_row, &wrap_row) in visible.iter().enumerate() {
            assert_eq!(map.folds.to_wrap(display_row), Some(wrap_row));
            assert_eq!(map.folds.to_display(wrap_row), Some(display_row));
        }
        let hidden_rows = map.wrap.row_count() - visible.len();
        assert_eq!(
            (0..map.wrap.row_count())
                .filter(|&row| map.folds.hidden_run(row).is_some())
                .count(),
            hidden_rows
        );
    }
    assert!(map.folds().len() > 375);

    // And the rows shown are those of a map folded afresh.
    let mut fresh = DisplayMap::new(buffer.current(), 4);
    wrap_at(&mut fresh, Some(8.));
    fresh.fold_many(map.folds().iter().map(|fold| fold.start..=fold.end));
    assert_eq!(rows(&map), rows(&fresh));
}
