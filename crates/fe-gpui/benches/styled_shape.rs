//! Shaping highlighted Rust with and without italic and bold spans (T1600):
//! 1,000 rows of this crate's `src/element.rs`, highlighted by fe-syntax,
//! their runs in the faces Yohen gives the captures (italic comments) or
//! all in the plain face, shaped as the text area shapes them:
//! [`fe_gpui::text_runs`] then the window text system's `layout_line`, in
//! Spline Sans Mono at 13 px with the real macOS text system.
//!
//! Each pass shapes the 1,000 rows with a new window text system, so none
//! comes from GPUI's per-frame line cache, as when a frame scrolls them all
//! into view. Plain and styled passes alternate, [`PAIRS`] of each, the
//! first of each pair taking turns, so load on the machine falls on both
//! alike; compare them by the fastest pass, by p50, and by the median of each
//! pair's ratio, which a change of load between pairs leaves out (PERF.md,
//! "Measuring"). The same is done for the rows with a styled span alone. The
//! budget is styled within 15% of plain. Nothing asserts it; the results go
//! in PERF.md ("Italic and bold spans").
//!
//! Run with `cargo bench -p fe-gpui --bench styled_shape`.

use std::{
    hint::black_box,
    sync::Arc,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_config::EditorPalette;
use fe_display::layout::StyleRun;
use fe_gpui::{Faces, SpanStyle, text_runs};
use gpui::{Hsla, TextRun, TextSystem, WindowTextSystem, hsla, px};

const ROWS: usize = 1_000;
/// Passes of each setting.
const PAIRS: usize = 200;
const SIZE: f32 = 13.;

/// One row's text and its runs.
type Row = (String, Vec<StyleRun<SpanStyle>>);

/// The first [`ROWS`] rows of `src/element.rs`, each with its highlights'
/// runs in the colours and faces Yohen gives them, the gaps plain.
fn highlighted_rows() -> Vec<Row> {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/element.rs"))
        .expect("the source reads");
    let text: String = source.split_inclusive('\n').take(ROWS).collect();
    assert_eq!(text.lines().count(), ROWS, "element.rs is long enough");
    let rust = fe_syntax::language_by_name("rust").expect("rust loads");
    let mut syntax = fe_syntax::Syntax::new(rust, &Buffer::new(&text).snapshot());
    assert!(syntax.parse_within(Duration::from_secs(60)), "it parses");
    let palette = EditorPalette::fastedit_dark();
    let styles: Vec<(&str, fe_config::FontStyle)> = (palette.styles.iter())
        .map(|(name, style)| (name.as_str(), *style))
        .collect();
    let styles = fe_syntax::resolve_theme(&styles);
    let plain = hsla(0., 0., 0.9, 1.);
    let mut rows = Vec::with_capacity(ROWS);
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let row = line.trim_end_matches('\n');
        let range = start..start + row.len();
        start += line.len();
        let mut runs = Vec::new();
        let mut at = range.start;
        for span in syntax.highlights(range.clone()) {
            let (from, to) = (span.range.start.max(at), span.range.end.min(range.end));
            if from >= to {
                continue;
            }
            if from > at {
                runs.push(StyleRun {
                    len: from - at,
                    style: SpanStyle::plain(plain),
                });
            }
            let ix = span.highlight.index();
            let color: Hsla = hsla(ix as f32 / 64. % 1., 0.6, 0.6, 1.);
            runs.push(StyleRun {
                len: to - from,
                style: SpanStyle::new(color, styles.get(ix).copied().flatten()),
            });
            at = to;
        }
        if at < range.end {
            runs.push(StyleRun {
                len: range.end - at,
                style: SpanStyle::plain(plain),
            });
        }
        rows.push((row.to_owned(), runs));
    }
    rows
}

/// `rows` with every run in the plain face, its colour kept.
fn unstyled(rows: &[Row]) -> Vec<Row> {
    (rows.iter())
        .map(|(text, runs)| {
            let runs = (runs.iter())
                .map(|run| StyleRun {
                    len: run.len,
                    style: SpanStyle::plain(run.style.color),
                })
                .collect();
            (text.clone(), runs)
        })
        .collect()
}

/// One pass: every row's text runs, then shaped, with a new window text
/// system.
fn pass(text_system: &Arc<TextSystem>, faces: &Faces, rows: &[Row]) -> Duration {
    let shaper = WindowTextSystem::new(text_system.clone());
    let mut out: Vec<TextRun> = Vec::new();
    let started = Instant::now();
    for (text, runs) in rows {
        text_runs(text, runs, faces, &mut out);
        black_box(shaper.layout_line(text, px(SIZE), &out, None));
    }
    started.elapsed()
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

fn report(name: &str, mut passes: Vec<Duration>) -> (Duration, Duration) {
    let count = passes.len();
    passes.sort();
    let (fastest, p50, max) = (
        passes[0],
        passes[passes.len() / 2],
        passes[passes.len() - 1],
    );
    println!(
        "{name:>8}: fastest {:.3} ms, p50 {:.3} ms, max {:.3} ms over {count} passes",
        ms(fastest),
        ms(p50),
        ms(max),
    );
    (fastest, p50)
}

/// Times `plain` and `styled` in alternate passes, and prints what they
/// took.
fn compare(
    text_system: &Arc<TextSystem>,
    faces: &Faces,
    name: &str,
    plain: &[Row],
    styled: &[Row],
) {
    println!("{name} ({} rows):", plain.len());
    // Warm: fonts resolved and their glyphs loaded.
    for _ in 0..3 {
        pass(text_system, faces, plain);
        pass(text_system, faces, styled);
    }
    let (mut plain_passes, mut styled_passes, mut ratios) = (Vec::new(), Vec::new(), Vec::new());
    for ix in 0..PAIRS {
        let (p, s) = match ix % 2 {
            0 => {
                let p = pass(text_system, faces, plain);
                (p, pass(text_system, faces, styled))
            }
            _ => {
                let s = pass(text_system, faces, styled);
                (pass(text_system, faces, plain), s)
            }
        };
        plain_passes.push(p);
        styled_passes.push(s);
        ratios.push(ms(s) / ms(p));
    }
    let (plain_fastest, plain_p50) = report("plain", plain_passes);
    let (styled_fastest, styled_p50) = report("styled", styled_passes);
    ratios.sort_by(f64::total_cmp);
    let ratio = |styled: Duration, plain: Duration| (ms(styled) / ms(plain) - 1.) * 100.;
    println!(
        "  styled vs plain: fastest {:+.1}%, p50 {:+.1}%, median pair {:+.1}% (budget +15%)",
        ratio(styled_fastest, plain_fastest),
        ratio(styled_p50, plain_p50),
        (ratios[ratios.len() / 2] - 1.) * 100.,
    );
}

fn main() {
    // `cargo bench` passes `--bench`; there are no cases to filter.
    let platform = gpui_platform::current_platform(true);
    let text_system = Arc::new(TextSystem::new(platform.text_system()));
    text_system
        .add_fonts(fe_fonts::all())
        .expect("the bundled fonts register");
    let font = gpui::font(fe_fonts::CODE);
    let faces = Faces::new(&text_system, &font, px(SIZE));

    let styled = highlighted_rows();
    let plain = unstyled(&styled);
    let changing = (styled.iter())
        .filter(|(_, runs)| runs.iter().any(|run| run.style.changes_face()))
        .count();
    println!("{changing} of {ROWS} rows have a styled span");

    let styled_only: Vec<Row> = (styled.iter())
        .filter(|(_, runs)| runs.iter().any(|run| run.style.changes_face()))
        .cloned()
        .collect();
    let plain_only = unstyled(&styled_only);
    compare(&text_system, &faces, "all rows", &plain, &styled);
    compare(
        &text_system,
        &faces,
        "styled rows",
        &plain_only,
        &styled_only,
    );
    // Kept: its text system holds on to what the platform made.
    std::mem::forget(platform);
}
