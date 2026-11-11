//! The text area's accessible text, timed as a screen reader makes the
//! editor pay for it: per keystroke, the pure builder
//! ([`a11y_text::build`]) and an `accesskit_consumer::Tree` update with the
//! nodes it writes, which is the work each platform adapter does per frame
//! (design `docs/decisions/qenw9xxdzk-accessibility-design.md`, section 4,
//! "Performance"). The file is over 10,000 lines of this crate's Rust
//! source, and the view tall enough that the window reaches the 300-row cap
//! (a 100-row viewport), or a 60-row one under it. Two more cases rebuild
//! windows of rows the Rust file lacks: a 4 MiB line of non-ASCII text, and
//! rows of tabs.
//!
//! The budget is a p99 of at most 0.5 ms per keystroke for the two together
//! on the Mac. GPUI's own per-frame tree rebuild isn't counted, and frames
//! are laid out by a monospace measurer outside the timing, since the
//! headless platform can't turn accessibility on. Nothing asserts the
//! budget; the results go in the card's finish notes.
//!
//! Run with `cargo bench -p fe-gpui --bench a11y_text`.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use accesskit_consumer::{Node as ConsumerNode, Tree, TreeChangeHandler};
use fe_buffer::{Buffer, Origin, ResolvedSelection};
use fe_display::{
    CharWidth, DisplayMap, LongRows,
    layout::{
        Frame, FrameInput, FrameStyles, MeasuredLine, StyleRun, TextMeasure, Vec2, layout_frame,
    },
};
use gpui::accesskit::{self, Node, NodeId, Role, TreeId, TreeUpdate};

#[allow(dead_code)]
#[path = "../src/element/a11y_text.rs"]
mod a11y_text;

use a11y_text::{A11yText, Cap, Input, RunKey};

const CELL: f32 = 8.;
const LINE: f32 = 18.;
const LINES: usize = 10_000;
const KEYSTROKES: usize = 400;
const WARMUP: usize = 20;
const BUDGET: Duration = Duration::from_micros(500);

/// A shaped line whose x positions are known up front, as GPUI's glyphs
/// are: every char a cell, tabs to 4-column stops.
struct Line {
    /// The x of each byte index's boundary, and the line's width last.
    xs: Vec<f32>,
}

impl MeasuredLine for Line {
    fn width(&self) -> f32 {
        self.xs.last().copied().unwrap_or(0.)
    }

    fn x_for_index(&self, index: usize) -> f32 {
        self.xs[index.min(self.xs.len() - 1)]
    }

    fn index_for_x(&self, x: f32) -> usize {
        self.xs.partition_point(|at| *at < x)
    }
}

struct Mono;

impl CharWidth for Mono {
    fn char_width(&mut self, _: char) -> f32 {
        CELL
    }
}

impl TextMeasure for Mono {
    type Style = ();
    type Line = Line;

    fn shape(&mut self, text: &str, _: &[StyleRun<()>]) -> Line {
        let mut xs = Vec::with_capacity(text.len() + 1);
        let mut x = 0.;
        for ch in text.chars() {
            for _ in 0..ch.len_utf8() {
                xs.push(x);
            }
            x += CELL;
        }
        xs.push(x);
        Line { xs }
    }
}

/// Over [`LINES`] lines of this crate's source.
fn rust_file() -> String {
    let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    let mut paths: Vec<_> = std::fs::read_dir(src)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    paths.sort();
    let mut text = String::new();
    for path in paths {
        text.push_str(&std::fs::read_to_string(path).unwrap());
        if text.lines().count() >= LINES {
            break;
        }
    }
    text
}

fn frame(display: &DisplayMap, caret: usize, top: u32, rows: u32) -> (Frame<Line>, LongRows) {
    let selections = [ResolvedSelection {
        range: caret..caret,
        reversed: false,
    }];
    let input = FrameInput {
        display,
        tab_size: 4,
        selections: &selections,
        primary_head: caret,
        scroll: Vec2 {
            x: 0.,
            y: top as f32 * LINE,
        },
        autoscroll: false,
        width: 1600.,
        height: rows as f32 * LINE,
        line_height: LINE,
        em: CELL,
        styles: FrameStyles {
            text: (),
            line_number: (),
            active_line_number: (),
        },
    };
    let mut long_rows = LongRows::default();
    let frame = layout_frame(
        &input,
        &mut Mono,
        &mut long_rows,
        &mut Mono,
        |_| Vec::new(),
        |_| Vec::new(),
    );
    (frame, long_rows)
}

fn build(
    display: &DisplayMap,
    frame: &Frame<Line>,
    long_rows: &mut LongRows,
    caret: usize,
) -> A11yText {
    a11y_text::build(Input {
        display,
        frame,
        long_rows,
        widths: &mut Mono,
        tab_size: 4,
        anchor: caret,
        head: caret,
        origin: (0., 0.),
        scale: 2.,
        cap: Cap::EDITOR,
        read_only: false,
    })
}

fn run_id(key: RunKey) -> NodeId {
    NodeId(((u64::from(key.buffer_row) + 1) << 20) | u64::from(key.segment))
}

/// The tree a window with the text area focused sends.
fn update(text: &A11yText) -> TreeUpdate {
    let mut editor = Node::new(Role::MultilineTextInput);
    let mut nodes = text.write(&mut editor, run_id);
    editor.set_children(nodes.iter().map(|(id, _)| *id).collect::<Vec<_>>());
    let mut root = Node::new(Role::Window);
    root.set_children(vec![NodeId(1)]);
    nodes.push((NodeId(1), editor));
    nodes.push((NodeId(0), root));
    TreeUpdate {
        nodes,
        tree: Some(accesskit::Tree::new(NodeId(0))),
        tree_id: TreeId::ROOT,
        focus: NodeId(1),
    }
}

struct Ignore;

impl TreeChangeHandler for Ignore {
    fn node_added(&mut self, _: &ConsumerNode) {}
    fn node_updated(&mut self, _: &ConsumerNode, _: &ConsumerNode) {}
    fn focus_moved(&mut self, _: Option<&ConsumerNode>, _: Option<&ConsumerNode>) {}
    fn node_removed(&mut self, _: &ConsumerNode) {}
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let ix = ((sorted.len() as f64 - 1.) * p).round() as usize;
    sorted[ix]
}

fn report(name: &str, mut samples: Vec<Duration>) {
    samples.sort();
    let p99 = percentile(&samples, 0.99);
    println!(
        "{name:<34} min {:>9.3?}  p50 {:>9.3?}  p99 {:>9.3?}  max {:>9.3?}  {}",
        samples[0],
        percentile(&samples, 0.5),
        p99,
        samples[samples.len() - 1],
        match p99 <= BUDGET {
            true => "within 0.5 ms",
            false => "OVER 0.5 ms",
        }
    );
}

/// Types [`KEYSTROKES`] characters on a row in the middle of a `rows`-row
/// viewport, timing the builder, the consumer update, and both.
fn keystrokes(text: &str, rows: u32) {
    let mut buffer = Buffer::new(text);
    let mut display = DisplayMap::new(buffer.current(), 4);
    let top = (LINES / 2) as u32;
    let row = top + rows / 2;
    let mut caret = buffer.current().text().row_start(row) + 4;
    let (shown, mut long_rows) = frame(&display, caret, top, rows);
    let first = build(&display, &shown, &mut long_rows, caret);
    let mut tree = Tree::new(update(&first), true);
    let (mut built, mut updated, mut both) = (Vec::new(), Vec::new(), Vec::new());
    let (mut runs, mut bytes) = (0, 0);
    for keystroke in 0..WARMUP + KEYSTROKES {
        buffer.edit([(caret..caret, "x")], Origin::User);
        caret += 1;
        display.sync(buffer.current(), &mut Mono);
        let (shown, mut long_rows) = frame(&display, caret, top, rows);
        let started = Instant::now();
        let text = build(&display, &shown, &mut long_rows, caret);
        let after_build = Instant::now();
        let update = update(&text);
        tree.update_and_process_changes(update, &mut Ignore);
        let done = Instant::now();
        black_box(tree.state().focus());
        runs = text.runs.len();
        bytes = text.runs.iter().map(|run| run.text.len()).sum();
        if keystroke >= WARMUP {
            built.push(after_build - started);
            updated.push(done - after_build);
            both.push(done - started);
        }
    }
    println!("keystroke, {rows}-row viewport: {runs} runs, {bytes} bytes");
    report("  builder", built);
    report("  consumer update", updated);
    report("  builder + consumer update", both);
}

/// Scrolls a `rows`-row viewport down a row a sample, timing both.
fn scrolls(text: &str, rows: u32) {
    let buffer = Buffer::new(text);
    let display = DisplayMap::new(buffer.current(), 4);
    let start = (LINES / 4) as u32;
    let caret = buffer.current().text().row_start(start + rows / 2);
    let (shown, mut long_rows) = frame(&display, caret, start, rows);
    let mut tree = Tree::new(
        update(&build(&display, &shown, &mut long_rows, caret)),
        true,
    );
    let mut both = Vec::new();
    for step in 1..=(WARMUP + KEYSTROKES) as u32 {
        let (shown, mut long_rows) = frame(&display, caret, start + step, rows);
        let started = Instant::now();
        let text = build(&display, &shown, &mut long_rows, caret);
        tree.update_and_process_changes(update(&text), &mut Ignore);
        if step as usize > WARMUP {
            both.push(started.elapsed());
        }
    }
    println!("scroll by a row, {rows}-row viewport");
    report("  builder + consumer update", both);
}

/// Builds and loads the window of `text` shown from row 0, `rows` rows
/// tall, again and again: for rows long or full of tabs, which the Rust
/// file has few of.
fn rebuilds(name: &str, text: &str, rows: u32) {
    let buffer = Buffer::new(text);
    let display = DisplayMap::new(buffer.current(), 4);
    let (shown, mut long_rows) = frame(&display, 0, 0, rows);
    let mut tree = Tree::new(update(&build(&display, &shown, &mut long_rows, 0)), true);
    let mut both = Vec::new();
    let (mut runs, mut bytes) = (0, 0);
    for sample in 0..WARMUP + KEYSTROKES / 4 {
        let started = Instant::now();
        let text = build(&display, &shown, &mut long_rows, sample % 7);
        runs = text.runs.len();
        bytes = text.runs.iter().map(|run| run.text.len()).sum();
        tree.update_and_process_changes(update(&text), &mut Ignore);
        if sample >= WARMUP {
            both.push(started.elapsed());
        }
    }
    println!("{name}, {rows}-row viewport: {runs} runs, {bytes} bytes");
    report("  builder + consumer update", both);
}

fn main() {
    let text = rust_file();
    let lines = text.lines().count();
    println!(
        "a11y_text: {lines} lines ({} KiB) of fe-gpui's Rust source, cap {} rows / {} KiB",
        text.len() / 1024,
        Cap::EDITOR.rows,
        Cap::EDITOR.bytes / 1024
    );
    for rows in [100, 60] {
        keystrokes(&text, rows);
        scrolls(&text, rows);
    }
    // A 4 MiB line of accented letters, wrap off: one visible row cut at
    // the byte cap, measured char by char by its long-row index.
    let long = format!(
        "fn x() {{}}\n{}\n{}",
        "é".repeat(2 << 20),
        "}\n".repeat(200)
    );
    rebuilds("a 4 MiB Unicode row", &long, 60);
    // Rows of 1,000 tabs and letters, indented as generated code can be.
    let tabs = (0..2000)
        .map(|_| "\ta".repeat(500))
        .collect::<Vec<_>>()
        .join("\n");
    rebuilds("1,000-tab rows", &tabs, 60);
}
