//! Large-file open and save budgets (release, Apple Silicon):
//! - a 100 MB / 1M-line file opens through the streaming decoders well inside
//!   the 500 ms "interactive" budget;
//! - saving it back, encoded, is of the same order;
//! - row and UTF-16 lookups in it stay far below a frame.
//!
//! Run with `cargo bench -p fe-text`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use encoding_rs::{UTF_16LE, WINDOWS_1252};
use fe_text::{Bias, FileEncoding, LineEnding, Point, PointUtf16, Text};
use rand::{Rng, SeedableRng, rngs::StdRng};

const TARGET_LEN: usize = 100 * 1024 * 1024;

/// About 100 MB of code-like text in 1M lines, with some non-ASCII.
fn corpus() -> String {
    let lines = [
        "    let value = compute_something(argument_one, argument_two, &mut state)?; // update the state",
        "    if value.is_empty() { return Err(Error::new(\"émpty value 😀 encountered while parsing\")); }",
        "fn handle_request(request: Request, context: &Context) -> Result<Response, HandlerError> {",
        "        .map(|item| item.name.to_uppercase()).filter(|name| !name.starts_with('_')).collect()",
        "}",
    ];
    repeat_lines(&lines)
}

/// About 100 MB in 1M lines of ASCII plus Latin-1, which windows-1252 can hold.
fn latin1_corpus() -> String {
    let lines = [
        "    let value = compute_something(argument_one, argument_two, &mut state)?; // update the state",
        "    if value.is_empty() { return Err(Error::new(\"émpty value ü encountered while parsing\")); }",
        "fn handle_request(request: Request, context: &Context) -> Result<Response, HandlerError> {",
        "        .map(|item| item.name.to_uppercase()).filter(|name| !name.starts_with('_')).collect() // café",
        "}",
    ];
    repeat_lines(&lines)
}

fn repeat_lines(lines: &[&str]) -> String {
    let mut text = String::with_capacity(TARGET_LEN + 1024 * 1024);
    let mut ix = 0;
    while text.len() < TARGET_LEN {
        text.push_str(lines[ix % lines.len()]);
        text.push('\n');
        ix += 1;
    }
    text
}

fn utf16le_with_bom(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() * 2 + 2);
    bytes.extend_from_slice(b"\xFF\xFE");
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn random_boundary(rng: &mut impl Rng, text: &Text) -> usize {
    text.clip_offset(rng.random_range(0..=text.len()), Bias::Left)
}

fn open(c: &mut Criterion) {
    let source = corpus();
    let utf8 = source.as_bytes();
    let crlf = source.replace('\n', "\r\n").into_bytes();
    let utf16 = utf16le_with_bom(&source);
    let latin1_source = latin1_corpus();
    let (latin1, _, unmappable) = WINDOWS_1252.encode(&latin1_source);
    assert!(!unmappable);
    let latin1 = latin1.into_owned();
    drop(latin1_source);

    // Check each input takes the path its bench is named for.
    let decoded = Text::from_reader_detecting(utf16.as_slice()).unwrap();
    assert_eq!(decoded.encoding.encoding, UTF_16LE);
    let decoded = Text::from_reader_detecting(latin1.as_slice()).unwrap();
    assert_eq!(decoded.encoding.encoding, WINDOWS_1252);
    let decoded = Text::from_reader_detecting(crlf.as_slice()).unwrap();
    assert_eq!(decoded.line_ending, LineEnding::CrLf);

    let mut group = c.benchmark_group("open");
    group.sample_size(10);
    group.bench_function("from_reader 100MB", |b| {
        b.iter(|| Text::from_reader(black_box(utf8)).unwrap())
    });
    group.bench_function("detecting utf-8 100MB", |b| {
        b.iter(|| Text::from_reader_detecting(black_box(utf8)).unwrap())
    });
    group.bench_function("detecting utf-8 crlf 100MB", |b| {
        b.iter(|| Text::from_reader_detecting(black_box(crlf.as_slice())).unwrap())
    });
    group.bench_function("detecting utf-16le bom 100MB", |b| {
        b.iter(|| Text::from_reader_detecting(black_box(utf16.as_slice())).unwrap())
    });
    group.bench_function("detecting windows-1252 100MB", |b| {
        b.iter(|| Text::from_reader_detecting(black_box(latin1.as_slice())).unwrap())
    });
    group.finish();
}

fn save(c: &mut Criterion) {
    let (text, _) = Text::from_file_contents(&corpus());
    let (latin1_text, _) = Text::from_file_contents(&latin1_corpus());
    let utf16le = FileEncoding {
        encoding: UTF_16LE,
        bom: true,
    };
    let windows_1252 = FileEncoding {
        encoding: WINDOWS_1252,
        bom: false,
    };
    let mut out = Vec::with_capacity(2 * TARGET_LEN + 4 * 1024 * 1024);

    let mut group = c.benchmark_group("save");
    group.sample_size(10);
    group.bench_function("write_to lf 100MB", |b| {
        b.iter(|| {
            out.clear();
            text.write_to(LineEnding::Lf, &mut out).unwrap();
            black_box(out.len())
        })
    });
    group.bench_function("write_to crlf 100MB", |b| {
        b.iter(|| {
            out.clear();
            text.write_to(LineEnding::CrLf, &mut out).unwrap();
            black_box(out.len())
        })
    });
    group.bench_function("write_encoded utf-16le 100MB", |b| {
        b.iter(|| {
            out.clear();
            text.write_encoded(LineEnding::Lf, utf16le, &mut out)
                .unwrap();
            black_box(out.len())
        })
    });
    group.bench_function("write_encoded windows-1252 100MB", |b| {
        b.iter(|| {
            out.clear();
            latin1_text
                .write_encoded(LineEnding::Lf, windows_1252, &mut out)
                .unwrap();
            black_box(out.len())
        })
    });
    group.finish();
}

fn lookup(c: &mut Criterion) {
    let (text, _) = Text::from_file_contents(&corpus());
    let mut rng = StdRng::seed_from_u64(1);
    let rows: Vec<u32> = (0..1000)
        .map(|_| rng.random_range(0..text.row_count()))
        .collect();
    let offsets: Vec<usize> = (0..1000)
        .map(|_| random_boundary(&mut rng, &text))
        .collect();
    let points: Vec<Point> = offsets.iter().map(|&o| text.offset_to_point(o)).collect();
    let points_utf16: Vec<PointUtf16> = offsets
        .iter()
        .map(|&o| text.offset_to_point_utf16(o))
        .collect();

    let mut group = c.benchmark_group("lookup");
    group.bench_function("row_start x1000", |b| {
        b.iter(|| {
            rows.iter()
                .map(|&r| text.row_start(black_box(r)))
                .sum::<usize>()
        })
    });
    group.bench_function("row_len x1000", |b| {
        b.iter(|| {
            rows.iter()
                .map(|&r| text.row_len(black_box(r)))
                .sum::<u32>()
        })
    });
    group.bench_function("offset_to_point x1000", |b| {
        b.iter(|| {
            offsets
                .iter()
                .map(|&o| black_box(text.offset_to_point(black_box(o))).row)
                .sum::<u32>()
        })
    });
    group.bench_function("point_to_offset x1000", |b| {
        b.iter(|| {
            points
                .iter()
                .map(|&p| text.point_to_offset(black_box(p)))
                .sum::<usize>()
        })
    });
    group.bench_function("offset_to_point_utf16 x1000", |b| {
        b.iter(|| {
            offsets
                .iter()
                .map(|&o| black_box(text.offset_to_point_utf16(black_box(o))).row)
                .sum::<u32>()
        })
    });
    group.bench_function("point_utf16_to_offset x1000", |b| {
        b.iter(|| {
            points_utf16
                .iter()
                .map(|&p| text.point_utf16_to_offset(black_box(p)))
                .sum::<usize>()
        })
    });
    group.finish();
}

criterion_group!(benches, open, save, lookup);
criterion_main!(benches);
