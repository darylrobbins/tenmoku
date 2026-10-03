use fe_buffer::Buffer;
use proptest::prelude::*;

use super::tests::{EditSpec, apply, check_round_trips, edit_strategy, rows, wrap_at};
use super::*;
use crate::Monospace;

/// One unit per char, counting the chars measured.
#[derive(Default)]
struct Counting(usize);

impl CharWidth for Counting {
    fn char_width(&mut self, _: char) -> f32 {
        self.0 += 1;
        1.
    }
}

/// How many display rows each buffer row takes.
fn segments(map: &DisplayMap, buffer_row: u32) -> usize {
    let first = map.row_for_buffer_row(buffer_row);
    (first..map.row_count())
        .take_while(|&row| map.row(row).buffer_row == buffer_row)
        .count()
}

#[test]
fn set_wrap_width_defers_rewrap() {
    // `é` keeps every row off the ASCII fast path, so wrapping one measures.
    let buffer = Buffer::new(&"é aaaa bbbb cccc\n".repeat(1_000_000));
    let mut map = DisplayMap::new(buffer.current(), 4);
    let mut widths = Counting::default();

    // Turning wrap on measures the ASCII table and nothing else.
    map.set_wrap_width(Some(8.), &mut widths);
    assert_eq!(widths.0, 128);
    assert!(map.wrap_is_pending());
    assert_eq!(map.wrap.item_count(), 1);
    assert_eq!(map.row_count(), 1_000_001);
    assert_eq!(segments(&map, 500_000), 1);

    // `é aaaa `, `bbbb `, `cccc`.
    map.wrap_pending(500_000, 0, &mut widths);
    assert_eq!(segments(&map, 500_000), 3);
    assert_eq!(map.row_count(), 1_000_003);

    // A new width keeps showing the old wrap.
    widths.0 = 0;
    map.set_wrap_width(Some(12.), &mut widths);
    assert_eq!(widths.0, 128);
    assert!(map.wrap_is_pending());
    assert!(map.wrap.item_count() <= 3);
    assert_eq!(segments(&map, 500_000), 3);
    assert_eq!(map.row_count(), 1_000_003);
    let row = map.row(map.row_for_buffer_row(500_000) + 1);
    let start = map.text().row_start(500_000);
    assert_eq!(row.range, start + 8..start + 13);
}

#[test]
fn slices_wrap_the_viewport_first() {
    // 20 rows of 10 bytes with their newlines, each wrapping in two at 5.
    let buffer = Buffer::new(&vec!["aaaa bbbb"; 20].join("\n"));
    let mut map = DisplayMap::new(buffer.current(), 4);
    map.set_wrap_width(Some(5.), &mut Monospace);
    let wrapped =
        |map: &DisplayMap| -> Vec<u32> { (0..20).filter(|&row| segments(map, row) == 2).collect() };

    // From `around` onward, until the budget is spent.
    assert!(map.wrap_pending(10, 25, &mut Monospace));
    assert_eq!(wrapped(&map), [10, 11, 12]);
    // At least one row.
    assert!(map.wrap_pending(10, 0, &mut Monospace));
    assert_eq!(wrapped(&map), [10, 11, 12, 13]);
    // Then the rows before `around`, nearest first.
    assert!(map.wrap_pending(10, 70, &mut Monospace));
    assert_eq!(wrapped(&map), (9..20).collect::<Vec<_>>());
    assert!(map.wrap_pending(10, 1, &mut Monospace));
    assert_eq!(wrapped(&map), (8..20).collect::<Vec<_>>());
    assert!(!map.wrap_pending(10, usize::MAX, &mut Monospace));
    assert_eq!(wrapped(&map), (0..20).collect::<Vec<_>>());
    assert!(!map.wrap_is_pending());
}

#[derive(Clone, Debug)]
enum Op {
    /// Batches of edits, then a sync.
    Edit(Vec<Vec<EditSpec>>),
    Width(Option<usize>),
    TabSize(usize),
    Slice {
        around: u32,
        budget: usize,
    },
    /// The same text in a new buffer, which the map can't follow by edits.
    Reopen,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        prop::collection::vec(prop::collection::vec(edit_strategy(), 1..3), 1..3)
            .prop_map(Op::Edit),
        prop::option::of(1usize..10).prop_map(Op::Width),
        prop::sample::select(vec![2, 4]).prop_map(Op::TabSize),
        (0u32..12, 0usize..30).prop_map(|(around, budget)| Op::Slice { around, budget }),
        Just(Op::Reopen),
    ]
}

proptest! {
    /// Edits, width and tab size changes and slices in any order round-trip
    /// at every step, and once every row is wrapped agree with a map built
    /// from scratch.
    #[test]
    fn pending_wrap_with_edits_matches_fresh(
        text in "[ab \n\té]{0,80}",
        folds in prop::collection::vec((0u32..12, 1u32..5), 0..3),
        ops in prop::collection::vec(op_strategy(), 0..10),
    ) {
        let mut buffer = Buffer::new(&text);
        let mut map = DisplayMap::new(buffer.current(), 4);
        let mut tab_size = 4;
        for (start, len) in folds {
            map.fold(start..=start + len);
        }
        for op in ops {
            match op {
                Op::Edit(batches) => {
                    for batch in &batches {
                        apply(&mut buffer, batch);
                    }
                    map.sync(buffer.current(), &mut Monospace);
                }
                Op::Width(width) => {
                    map.set_wrap_width(width.map(|width| width as f32), &mut Monospace);
                }
                Op::TabSize(size) => {
                    tab_size = size;
                    map.set_tab_size(size, &mut Monospace);
                }
                Op::Slice { around, budget } => {
                    let pending = map.wrap_pending(around, budget, &mut Monospace);
                    prop_assert_eq!(pending, map.wrap_is_pending());
                }
                Op::Reopen => {
                    buffer = Buffer::new(&buffer.text().slice_to_string(0..buffer.len()));
                    map.sync(buffer.current(), &mut Monospace);
                }
            }
            check_round_trips(&map)?;
        }
        while map.wrap_pending(3, 7, &mut Monospace) {
            check_round_trips(&map)?;
        }
        check_round_trips(&map)?;

        let mut fresh = DisplayMap::new(buffer.current(), tab_size);
        wrap_at(&mut fresh, map.wrap_width());
        for fold in map.folds() {
            fresh.fold(fold.start..=fold.end);
        }
        prop_assert_eq!(rows(&map), rows(&fresh));
    }
}
