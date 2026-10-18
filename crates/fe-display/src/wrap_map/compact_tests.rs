//! The wrap tree stays compact under edits, and keeping it so changes no
//! wrap row.

use std::{fmt, time::Instant};

use fe_text::Text;
use rand::{Rng, SeedableRng, rngs::StdRng};

use super::*;
use crate::Monospace;

fn wrapped_map(text: &Text, width: f32) -> WrapMap {
    let mut map = WrapMap::new(text, 4);
    map.rebuild(text, Some(width), 4, &mut Monospace);
    assert!(!map.wrap_pending(text, 0, usize::MAX, &mut Monospace));
    map
}

/// Replaces `range` of `text` with `insert` and splices `map` to follow.
fn edit(map: &mut WrapMap, text: &mut Text, range: Range<usize>, insert: &str) {
    let first_row = text.offset_to_point(range.start).row as usize;
    let old_rows = text.offset_to_point(range.end).row as usize - first_row + 1;
    text.replace(range.clone(), insert);
    let new_rows = text.offset_to_point(range.start + insert.len()).row as usize - first_row + 1;
    map.splice(text, first_row, old_rows, new_rows, &mut Monospace);
}

fn segments(map: &WrapMap, text: &Text) -> Vec<WrapSegment> {
    (0..map.row_count())
        .map(|row| map.segment(text, row))
        .collect()
}

#[test]
fn wrap_tree_matches_a_fresh_map_after_random_splices() {
    let mut rng = StdRng::seed_from_u64(7);
    let alphabet = ['a', 'b', ' ', ' ', '\n', 'é'];
    let random_text = |rng: &mut StdRng, len: usize| -> String {
        (0..len)
            .map(|_| alphabet[rng.random_range(0..alphabet.len())])
            .collect()
    };
    let mut text = Text::new(&random_text(&mut rng, 3_000));
    let mut map = wrapped_map(&text, 8.);
    map.compactor.every = 1;
    map.compactor.window = 5;
    let mut rewritten = 0;
    for round in 0..4_000 {
        let start = text.clip_offset(rng.random_range(0..=text.len()), fe_text::Bias::Left);
        let end = text.clip_offset(
            (start + rng.random_range(0..6)).min(text.len()),
            fe_text::Bias::Left,
        );
        // Mostly typing a char, as keystrokes do; sometimes more.
        let insert = if rng.random_bool(0.7) {
            random_text(&mut rng, 1)
        } else {
            let len = rng.random_range(0..12);
            random_text(&mut rng, len)
        };
        // A window runs after each splice that isn't rewritten in place.
        let runs = map.compactor.runs;
        edit(&mut map, &mut text, start..end, &insert);
        rewritten += usize::from(map.compactor.runs == runs);
        if round % 100 == 99 {
            let fresh = wrapped_map(&text, 8.);
            assert_eq!(map.row_count(), fresh.row_count(), "round {round}");
            assert_eq!(
                segments(&map, &text),
                segments(&fresh, &text),
                "round {round}"
            );
        }
    }
    // Both paths ran often: edits rewritten in place, and windows, which
    // went round the whole tree again and again.
    assert!(rewritten > 500, "{rewritten} rewritten in place");
    assert!(map.compactor.runs > 500, "{} windows", map.compactor.runs);
    assert!(map.compactor.sweeps > 5, "{} sweeps", map.compactor.sweeps);
}

/// Windows keep going round the tree while edits add rows behind them.
#[test]
fn windows_sweep_a_growing_tree() {
    let mut text = Text::new(&"one two three four\n".repeat(200));
    let mut map = wrapped_map(&text, 10.);
    map.compactor.every = 1;
    map.compactor.window = 10;
    let mut rng = StdRng::seed_from_u64(3);
    for _ in 0..1_000 {
        let offset = text.clip_offset(rng.random_range(0..text.len()), fe_text::Bias::Left);
        edit(&mut map, &mut text, offset..offset, "\n");
    }
    // 1,000 windows of 10 items over at most 1,201 items.
    assert_eq!(map.compactor.runs, 1_000);
    assert!(map.compactor.sweeps >= 8, "{} sweeps", map.compactor.sweeps);
    let fresh = wrapped_map(&text, 10.);
    assert_eq!(segments(&map, &text), segments(&fresh, &text));
}

/// Typing that keeps a row's wrap rows rewrites its item where it is: no
/// item moves, and the row reads its new wrap points.
#[test]
fn an_edit_keeping_wrap_rows_leaves_the_tree_in_place() {
    let mut text = Text::new(&"one two three four five six seven\n".repeat(200));
    let mut map = wrapped_map(&text, 10.);
    let items = |map: &WrapMap| -> Vec<usize> {
        map.lines
            .iter()
            .map(|item| item as *const LineItem as usize)
            .collect()
    };
    let before = items(&map);
    let row_start = text.row_start(100);
    let first = map.first_row(100);
    assert_eq!(map.segment(&text, first).range, 0..8);
    edit(&mut map, &mut text, row_start..row_start, "x");
    assert_eq!(items(&map), before);
    assert_eq!(map.segment(&text, first).range, 0..9);
    assert_eq!(map.segment(&text, first + 1).range, 9..20);
    let fresh = wrapped_map(&text, 10.);
    assert_eq!(segments(&map, &text), segments(&fresh, &text));

    // A new row moves items.
    edit(&mut map, &mut text, row_start..row_start, "\n");
    assert_ne!(items(&map), before);
    let fresh = wrapped_map(&text, 10.);
    assert_eq!(segments(&map, &text), segments(&fresh, &text));
}

/// How many leaves and internal nodes a tree has, read from its `Debug`
/// output, which names each node, and its height.
#[derive(Debug, Default)]
struct Shape {
    leaves: usize,
    internal: usize,
    height: u8,
    after_height: bool,
}

impl fmt::Write for Shape {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.after_height
            && let Ok(height) = s.parse()
        {
            self.height = self.height.max(height);
            self.after_height = false;
        }
        match s {
            "height" => self.after_height = true,
            "Leaf" => self.leaves += 1,
            "Internal" => self.internal += 1,
            _ => {}
        }
        Ok(())
    }
}

fn shape(map: &WrapMap) -> Shape {
    let mut shape = Shape::default();
    fmt::write(&mut shape, format_args!("{:?}", map.lines)).unwrap();
    shape
}

/// How many times walking the items in order jumps backward, or more than
/// 1 KiB forward, in memory: a leaf that isn't just after the one before.
fn jumps(map: &WrapMap) -> usize {
    let mut last = 0;
    let mut jumps = 0;
    for item in map.lines.iter() {
        let at = item as *const LineItem as usize;
        if at <= last || at - last > 1024 {
            jumps += 1;
        }
        last = at;
    }
    jumps
}

/// The display bench's corpus: 1M code-like lines, most wider than 80
/// columns.
fn corpus() -> String {
    let lines = [
        "    let value = compute_something(argument_one, argument_two, &mut state)?; // update the state",
        "    if value.is_empty() { return Err(Error::new(\"émpty value 😀 encountered while parsing\")); }",
        "fn handle_request(request: Request, context: &Context) -> Result<Response, HandlerError> {",
        "        .map(|item| item.name.to_uppercase()).filter(|name| !name.starts_with('_')).collect()",
        "}",
    ];
    let mut text = String::new();
    for ix in 0..1_000_000 {
        text.push_str(lines[ix % lines.len()]);
        text.push('\n');
    }
    text
}

/// Types `key` at `count` random offsets, as the display bench does.
/// Returns the mean and slowest splice, in µs.
fn type_keys(
    map: &mut WrapMap,
    text: &mut Text,
    rng: &mut StdRng,
    key: &str,
    count: usize,
) -> (f64, f64) {
    let (mut total, mut slowest) = (0f64, 0f64);
    for _ in 0..count {
        let offset = text.clip_offset(rng.random_range(0..text.len()), fe_text::Bias::Left);
        let start = Instant::now();
        edit(map, text, offset..offset, key);
        let took = start.elapsed().as_secs_f64() * 1e6;
        total += took;
        slowest = slowest.max(took);
    }
    (total / count as f64, slowest)
}

/// The fold bench's rebuild: the wrap rows of 200k runs, one every 5 rows,
/// in one cursor pass. The fastest of 30, in ms.
fn walk(map: &WrapMap) -> f64 {
    (0..30)
        .map(|_| {
            let start = Instant::now();
            let bounds = (0..1_000_000).step_by(5).flat_map(|row| [row + 1, row + 4]);
            std::hint::black_box(map.first_rows(bounds).sum::<usize>());
            start.elapsed().as_secs_f64() * 1e3
        })
        .fold(f64::INFINITY, f64::min)
}

fn report(label: &str, map: &WrapMap) {
    let shape = shape(map);
    let items = map.item_count();
    println!(
        "{label}: {items} items, {} leaves ({:.2} items each), {} internal, height {}, \
         {} jumps, walk {:.2} ms",
        shape.leaves,
        items as f64 / shape.leaves as f64,
        shape.internal,
        shape.height,
        jumps(map),
        walk(map),
    );
}

/// Prints the wrapped 1M-line tree's shape and a fold-style walk's time,
/// fresh and after 5k and 100k keystrokes, typing a char (which keeps the
/// row's wrap rows) and typing Enter (which doesn't), with the default
/// compaction and with none. Run in release:
/// `cargo test --release -p fe-display --lib wrap_map::compact_tests::measure -- --ignored --nocapture`
#[test]
#[ignore]
fn measure() {
    let corpus = corpus();
    for (key, window) in [("x", 384), ("\n", 384), ("\n", 0)] {
        println!("typing {key:?}, window {window}");
        let mut text = Text::new(&corpus);
        let mut map = wrapped_map(&text, 80.);
        map.compactor.window = window;
        let mut rng = StdRng::seed_from_u64(1);
        report("  fresh", &map);
        for (label, count) in [("after 5k", 5_000), ("after 100k", 95_000)] {
            let (mean, slowest) = type_keys(&mut map, &mut text, &mut rng, key, count);
            println!(
                "  splices: mean {mean:.2} µs, slowest {slowest:.0} µs, {} windows so far",
                map.compactor.runs
            );
            report(&format!("  {label}"), &map);
        }
        let start = Instant::now();
        map.lines = SumTree::from_iter(map.lines.iter().cloned(), ());
        println!("  repack: {:.0} ms", start.elapsed().as_secs_f64() * 1e3);
        report("  after 100k, repacked", &map);
    }
}
