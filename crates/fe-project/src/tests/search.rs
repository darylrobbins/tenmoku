// Match ranges are compared as lists of ranges, some of them one long.
#![allow(clippy::single_range_in_vec_init)]

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::*;

/// Writes each `(path, contents)` under `root`, with parent directories.
fn write(root: &Path, files: &[(&str, &[u8])]) {
    for (path, contents) in files {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, contents).unwrap();
    }
}

/// `count` files `f000.txt`, … each holding `lines` lines of `needle`.
fn many(root: &Path, count: usize, lines: usize) {
    let text = "needle\n".repeat(lines);
    for ix in 0..count {
        fs::write(root.join(format!("f{ix:04}.txt")), &text).unwrap();
    }
}

/// Runs a search to the end, returning its batches and summary.
fn run(
    root: &Path,
    query: &SearchQuery,
    options: &SearchOptions,
) -> (Vec<Vec<FileMatches>>, SearchSummary) {
    let mut batches = Vec::new();
    let summary = search(root, query, options, Arc::default(), |batch| {
        batches.push(batch)
    })
    .unwrap();
    (batches, summary)
}

/// Every delivered file, sorted by `rel`.
fn files(batches: Vec<Vec<FileMatches>>) -> Vec<FileMatches> {
    let mut files: Vec<FileMatches> = batches.into_iter().flatten().collect();
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    files
}

fn rels(files: &[FileMatches]) -> Vec<&str> {
    files.iter().map(|file| file.rel.as_str()).collect()
}

fn match_count(batch: &[FileMatches]) -> usize {
    batch
        .iter()
        .flat_map(|file| &file.lines)
        .map(|line| line.ranges.len())
        .sum()
}

fn threads(threads: usize) -> SearchOptions {
    SearchOptions {
        threads,
        ..SearchOptions::default()
    }
}

#[test]
fn literal_search_finds_every_match() {
    let (_dir, root) = temp();
    let a = "x foo.bar( y foo.bar(\nnothing here\nfoo.bar(\n";
    let b = "fooXbar( is not it\n  foo.bar(z)\n";
    write(
        &root,
        &[
            ("a.txt", a.as_bytes()),
            ("src/deep/b.rs", b.as_bytes()),
            ("src/none.rs", b"foo bar\n"),
        ],
    );
    let needle = "foo.bar(";
    let (batches, summary) = run(
        &root,
        &SearchQuery::literal(needle),
        &SearchOptions::default(),
    );
    let files = files(batches);
    assert_eq!(rels(&files), ["a.txt", "src/deep/b.rs"]);

    let texts = [a, b];
    for (file, text) in files.iter().zip(texts) {
        assert_eq!(file.path, root.join(&file.rel));
        for line in &file.lines {
            let start = usize::try_from(line.line_offset).unwrap();
            let line_text = text[start..].lines().next().unwrap();
            let number = text[..start].matches('\n').count() + 1;
            assert_eq!(line.line_number, u64::try_from(number).unwrap());
            assert_eq!(line.preview, line_text);
            assert_eq!(line.preview_start, 0);
            for range in &line.ranges {
                assert_eq!(&line_text[range.clone()], needle);
            }
        }
    }
    let shape = |file: &FileMatches| -> Vec<(u64, u64, Vec<std::ops::Range<usize>>)> {
        file.lines
            .iter()
            .map(|line| (line.line_number, line.line_offset, line.ranges.clone()))
            .collect()
    };
    assert_eq!(
        shape(&files[0]),
        [(1, 0, vec![2..10, 13..21]), (3, 35, vec![0..8])]
    );
    assert_eq!(shape(&files[1]), [(2, 19, vec![2..10])]);

    assert_eq!(summary.files_searched, 3);
    assert_eq!(summary.files_matched, 2);
    assert_eq!(summary.matches, 4);
    assert_eq!(summary.binary_skipped, 0);
    assert!(summary.errors.is_empty());
    assert!(!summary.truncated && !summary.cancelled);
}

#[test]
fn search_skips_what_the_tree_hides() {
    let (_dir, root) = temp();
    let needle: &[u8] = b"# needle\n";
    write(
        &root,
        &[
            (".gitignore", b"# needle\n*.log\nbuild/\n"),
            (".ignore", b"# needle\nskip.txt\n"),
            ("sub/.gitignore", b"# needle\nsecret.txt\n"),
            ("a.txt", needle),
            ("x.log", needle),
            ("build/b.txt", needle),
            ("sub/secret.txt", needle),
            ("sub/keep.txt", needle),
            ("skip.txt", needle),
            (".hidden.txt", needle),
            (".config/c.txt", needle),
        ],
    );
    let expected = [
        ".config/c.txt",
        ".gitignore",
        ".hidden.txt",
        ".ignore",
        "a.txt",
        "sub/.gitignore",
        "sub/keep.txt",
    ];
    let finder = |root: &Path| {
        let mut paths = Vec::new();
        FinderIndex::walk(root, Arc::default(), |batch| paths.extend(batch));
        let mut paths: Vec<String> = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        paths.sort();
        paths
    };
    let query = SearchQuery::literal("needle");
    // Outside a git repo, then inside one, with `.git` holding the needle.
    for pass in 0..2 {
        if pass == 1 {
            write(
                &root,
                &[(".git/HEAD", needle), (".git/info/exclude", needle)],
            );
        }
        let (batches, summary) = run(&root, &query, &SearchOptions::default());
        let files = files(batches);
        assert_eq!(rels(&files), expected, "pass {pass}");
        assert_eq!(rels(&files), finder(&root), "pass {pass}");
        assert_eq!(summary.files_searched, expected.len(), "pass {pass}");
    }
}

#[test]
fn regex_case_and_whole_word_options() {
    let (_dir, root) = temp();
    write(
        &root,
        &[
            ("regex.txt", b"foo1 fo fooo2 fox\n"),
            ("case.txt", b"FOO\n"),
            ("word.txt", b"foobar\n_foo\nfoo.bar\n(foo)\n"),
        ],
    );
    let lines = |query: &SearchQuery| -> Vec<(String, u64, Vec<String>)> {
        let (batches, _) = run(&root, query, &SearchOptions::default());
        let mut lines: Vec<_> = files(batches)
            .into_iter()
            .flat_map(|file| {
                let rel = file.rel;
                file.lines.into_iter().map(move |line| {
                    let texts = line
                        .ranges
                        .iter()
                        .map(|range| line.preview[range.clone()].to_string())
                        .collect();
                    (rel.clone(), line.line_number, texts)
                })
            })
            .collect();
        lines.sort();
        lines
    };
    let owned = |rel: &str, line: u64, texts: &[&str]| {
        (
            rel.to_string(),
            line,
            texts
                .iter()
                .map(|text| text.to_string())
                .collect::<Vec<_>>(),
        )
    };

    let regex = SearchQuery {
        regex: true,
        ..SearchQuery::literal(r"fo+\d")
    };
    assert_eq!(lines(&regex), [owned("regex.txt", 1, &["foo1", "fooo2"])]);

    let insensitive = lines(&SearchQuery::literal("foo"));
    assert!(insensitive.contains(&owned("case.txt", 1, &["FOO"])));
    let sensitive = SearchQuery {
        case_sensitive: true,
        ..SearchQuery::literal("foo")
    };
    assert!(lines(&sensitive).iter().all(|(rel, ..)| rel != "case.txt"));

    let word = SearchQuery {
        whole_word: true,
        case_sensitive: true,
        ..SearchQuery::literal("foo")
    };
    assert_eq!(
        lines(&word),
        [
            owned("word.txt", 3, &["foo"]),
            owned("word.txt", 4, &["foo"])
        ]
    );

    let invalid = SearchQuery {
        regex: true,
        ..SearchQuery::literal("fo(o")
    };
    let mut calls = 0;
    let result = search(
        &root,
        &invalid,
        &SearchOptions::default(),
        Arc::default(),
        |_| calls += 1,
    );
    assert!(
        matches!(result, Err(SearchError::InvalidPattern(_))),
        "{result:?}"
    );
    assert_eq!(calls, 0);
}

#[test]
fn binary_and_long_lines_stay_bounded() {
    let (_dir, root) = temp();
    // A 1 MB line of two-byte chars, with the match near its end.
    let mut long = "é".repeat(500_000);
    let at = long.len();
    long.push_str("needle");
    long.push_str(&"ü".repeat(10));
    long.push('\n');
    write(
        &root,
        &[
            ("binary.bin", b"needle\0needle\n"),
            ("long.txt", long.as_bytes()),
            ("invalid.txt", b"ok\nab\xffneedle\xc3\n"),
        ],
    );
    let (batches, summary) = run(
        &root,
        &SearchQuery::literal("needle"),
        &SearchOptions::default(),
    );
    let files = files(batches);
    assert_eq!(rels(&files), ["invalid.txt", "long.txt"]);
    assert_eq!(summary.binary_skipped, 1);
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);

    let [line] = &files[1].lines[..] else {
        panic!("{:?}", files[1].lines.len())
    };
    assert_eq!(line.ranges, [at..at + 6]);
    assert!(line.preview.len() <= 512, "{}", line.preview.len());
    assert!(long.is_char_boundary(line.preview_start));
    let start = line.preview_start;
    assert_eq!(
        &line.preview[at - start..at + 6 - start],
        "needle",
        "the preview holds the match"
    );
    assert_eq!(
        line.preview,
        long[start..start + line.preview.len()],
        "the preview is the line from preview_start"
    );

    let [line] = &files[0].lines[..] else {
        panic!("{:?}", files[0].lines.len())
    };
    assert_eq!(line.line_number, 2);
    assert_eq!(line.ranges, [3..9]);
    assert_eq!(line.preview, "ab\u{fffd}needle\u{fffd}");
}

#[test]
fn results_stream_in_bounded_batches() {
    let (_dir, root) = temp();
    many(&root, 300, 1);
    let query = SearchQuery::literal("needle");

    let (batches, summary) = run(&root, &query, &threads(1));
    let sizes: Vec<usize> = batches.iter().map(Vec::len).collect();
    assert_eq!(sizes, [64, 64, 64, 64, 44]);
    assert_eq!(summary.files_matched, 300);
    let mut sequential: Vec<String> = batches.into_iter().flatten().map(|f| f.rel).collect();
    let in_order = sequential.clone();
    sequential.sort();
    assert_eq!(in_order, sequential, "threads: 1 walks in path order");

    let (batches, summary) = run(&root, &query, &SearchOptions::default());
    for batch in &batches {
        assert!(batch.len() <= 64 && match_count(batch) <= 1_000);
    }
    assert_eq!(summary.files_matched, 300);
    let mut parallel: Vec<String> = batches.into_iter().flatten().map(|f| f.rel).collect();
    parallel.sort();
    assert_eq!(parallel, sequential, "every file once");
}

#[test]
fn match_cap_stops_the_search() {
    let (_dir, root) = temp();
    many(&root, 1_000, 100);
    let query = SearchQuery::literal("needle");
    for threads in [1, 4] {
        let options = SearchOptions {
            max_matches: 500,
            threads,
        };
        let (batches, summary) = run(&root, &query, &options);
        let delivered: usize = batches.iter().map(|batch| match_count(batch)).sum();
        assert_eq!(delivered, 500, "threads: {threads}");
        assert_eq!(summary.matches, 500);
        assert!(summary.truncated);
        if threads == 1 {
            assert_eq!(summary.files_searched, 5);
        } else {
            assert!(summary.files_searched <= 9, "{}", summary.files_searched);
        }
    }
}

#[test]
fn cancel_stops_the_search() {
    let (_dir, root) = temp();
    many(&root, 300, 1);
    let query = SearchQuery::literal("needle");

    let cancel = Arc::new(AtomicBool::new(true));
    let mut calls = 0;
    let summary = search(&root, &query, &threads(1), cancel, |_| calls += 1).unwrap();
    assert_eq!((summary.files_searched, calls), (0, 0));
    assert!(summary.cancelled);

    for options in [threads(1), SearchOptions::default()] {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut calls = 0;
        let summary = search(&root, &query, &options, cancel.clone(), |_| {
            calls += 1;
            cancel.store(true, Ordering::Relaxed);
        })
        .unwrap();
        assert_eq!(calls, 1, "threads: {}", options.threads);
        assert!(summary.cancelled);
        if options.threads == 1 {
            assert_eq!(summary.files_searched, 64);
        }
    }
}

#[test]
fn reaching_the_cap_stops_reading_the_file() {
    let (_dir, root) = temp();
    // After the match, a line longer than the heap limit, which would fail
    // the file if it were read.
    let mut text = b"needle\n".to_vec();
    text.resize(text.len() + (17 << 20), b'x');
    write(&root, &[("a.txt", &text)]);
    let options = SearchOptions {
        max_matches: 1,
        threads: 1,
    };
    let (batches, summary) = run(&root, &SearchQuery::literal("needle"), &options);
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    assert_eq!(summary.matches, 1);
    assert_eq!(files(batches)[0].lines[0].ranges, [0..6]);
    assert!(summary.truncated);
}

#[test]
fn anchors_match_at_every_line_end() {
    let (_dir, root) = temp();
    write(
        &root,
        &[
            ("lf.txt", b"xfoo\nfoo\n"),
            ("crlf.txt", b"foox\r\nfoo\r\n"),
            ("end.txt", b"x\nfoo"),
            ("empty.txt", b"a\n\nb\n"),
        ],
    );
    let lines = |pattern: &str| -> Vec<(String, u64, Vec<std::ops::Range<usize>>)> {
        let query = SearchQuery {
            regex: true,
            ..SearchQuery::literal(pattern)
        };
        let (batches, _) = run(&root, &query, &SearchOptions::default());
        files(batches)
            .into_iter()
            .flat_map(|file| {
                let rel = file.rel;
                file.lines
                    .into_iter()
                    .map(move |line| (rel.clone(), line.line_number, line.ranges))
            })
            .collect()
    };
    let line = |rel: &str, number: u64, range: std::ops::Range<usize>| {
        (rel.to_string(), number, vec![range])
    };
    assert_eq!(
        lines("^foo$"),
        [
            line("crlf.txt", 2, 0..3),
            line("end.txt", 2, 0..3),
            line("lf.txt", 2, 0..3)
        ]
    );
    assert_eq!(lines("^$"), [line("empty.txt", 2, 0..0)]);
}

#[test]
fn cancel_stops_within_a_line() {
    let (_dir, root) = temp();
    write(&root, &[("a.txt", "a ".repeat(100_000).as_bytes())]);
    let cancel = Arc::new(AtomicBool::new(false));
    let seen = std::rc::Rc::new(std::cell::Cell::new(0));
    let hook = {
        let (cancel, seen) = (cancel.clone(), seen.clone());
        move || {
            seen.set(seen.get() + 1);
            if seen.get() == 10 {
                cancel.store(true, Ordering::Relaxed);
            }
        }
    };
    let options = SearchOptions {
        max_matches: usize::MAX,
        threads: 1,
    };
    crate::search::ON_MATCH.set(Some(Box::new(hook)));
    let mut calls = 0;
    let summary = search(&root, &SearchQuery::literal("a"), &options, cancel, |_| {
        calls += 1
    });
    crate::search::ON_MATCH.set(None);
    let summary = summary.unwrap();
    assert_eq!(seen.get(), 10, "no match is enumerated after cancel");
    assert_eq!(calls, 0);
    assert!(summary.cancelled);
}

#[test]
fn offsets_count_the_files_bytes() {
    let (_dir, root) = temp();
    let utf16 = |text: &str, big: bool| -> Vec<u8> {
        let mut bytes = if big {
            vec![0xFE, 0xFF]
        } else {
            vec![0xFF, 0xFE]
        };
        for unit in text.encode_utf16() {
            bytes.extend(if big {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            });
        }
        bytes
    };
    let text = "😀x\nneedle\n";
    write(
        &root,
        &[
            ("bom1.txt", b"\xEF\xBB\xBFneedle\n"),
            ("bom2.txt", b"\xEF\xBB\xBFx\nneedle\n"),
            ("utf16be.txt", &utf16(text, true)),
            ("utf16le.txt", &utf16(text, false)),
        ],
    );
    let (batches, _) = run(
        &root,
        &SearchQuery::literal("needle"),
        &SearchOptions::default(),
    );
    let found: Vec<_> = files(batches)
        .into_iter()
        .map(|file| {
            let [line] = &file.lines[..] else {
                panic!("{file:?}")
            };
            (
                file.rel,
                line.line_number,
                line.line_offset,
                line.ranges.clone(),
                line.preview.clone(),
            )
        })
        .collect();
    let row = |rel: &str, number, offset, range, preview: &str| {
        (
            rel.to_string(),
            number,
            offset,
            vec![range],
            preview.to_string(),
        )
    };
    assert_eq!(
        found,
        [
            // A UTF-8 BOM stays in the first line.
            row("bom1.txt", 1, 0, 3..9, "\u{feff}needle"),
            row("bom2.txt", 2, 5, 0..6, "needle"),
            // The BOM, then 😀x\n as four UTF-16 units.
            row("utf16be.txt", 2, 10, 0..6, "needle"),
            row("utf16le.txt", 2, 10, 0..6, "needle"),
        ]
    );
}

#[test]
fn previews_keep_the_match_and_the_line() {
    let (_dir, root) = temp();
    // Invalid bytes before the match: each becomes a 3-byte replacement
    // character, so these lines' previews are longer than the bytes.
    let mut long = vec![b'a'; 600];
    long.extend([0xFF; 253]);
    long.extend(b"needle");
    long.extend([b'b'; 600]);
    long.push(b'\n');
    let mut short = vec![0xFF; 200];
    short.extend(b"needle\n");
    write(
        &root,
        &[
            ("long.txt", &long),
            ("short.txt", &short),
            // A `\r` without a `\n` after it is part of the line.
            ("cr.txt", b"x\nneedle\r"),
        ],
    );
    let (batches, _) = run(
        &root,
        &SearchQuery::literal("needle"),
        &SearchOptions::default(),
    );
    let files = files(batches);
    assert_eq!(rels(&files), ["cr.txt", "long.txt", "short.txt"]);
    assert_eq!(files[0].lines[0].preview, "needle\r");
    for file in &files[1..] {
        let preview = &file.lines[0].preview;
        assert!(preview.len() <= 512, "{}: {}", file.rel, preview.len());
        assert!(preview.contains("needle"), "{}", file.rel);
    }
}

#[test]
fn previews_are_whole_chars_and_bounded_by_their_size() {
    let (_dir, root) = temp();
    // A match far longer than the preview.
    let mut long = vec![b'x'; 4 << 20];
    long.push(b'\n');
    // The window after a match at the start ends inside the emoji.
    let emoji = format!("{}😀tail\n", "a".repeat(509));
    write(
        &root,
        &[
            ("long.txt", &long),
            ("emoji.txt", emoji.as_bytes()),
            ("e.txt", "é\n".as_bytes()),
        ],
    );
    let only = |pattern: &str, rel: &str| -> LineMatch {
        let query = SearchQuery {
            regex: true,
            ..SearchQuery::literal(pattern)
        };
        let (batches, summary) = run(&root, &query, &SearchOptions::default());
        assert!(summary.errors.is_empty(), "{:?}", summary.errors);
        let file = files(batches)
            .into_iter()
            .find(|file| file.rel == rel)
            .unwrap();
        file.lines.into_iter().next().unwrap()
    };

    let line = only("^x*$", "long.txt");
    assert_eq!(line.ranges, [0..4 << 20]);
    assert_eq!((line.preview, line.preview_start), ("x".repeat(512), 0));

    let line = only("^", "emoji.txt");
    assert_eq!((line.preview, line.preview_start), ("a".repeat(509), 0));

    // A byte-wise match inside a char: the preview keeps the char whole.
    let line = only("(?-u:.)", "e.txt");
    assert_eq!(line.ranges, [0..1, 1..2]);
    assert_eq!(line.preview, "é");
}

#[test]
fn a_lone_carriage_return_is_content() {
    let (_dir, root) = temp();
    write(
        &root,
        &[
            ("end.txt", b"x\nneedle\r"),
            ("mid.txt", b"a\rb\n"),
            ("crlf.txt", b"needle\r\n"),
            ("lines.txt", b"a\nb\n"),
        ],
    );
    let found = |query: &SearchQuery| -> Vec<(String, Vec<std::ops::Range<usize>>)> {
        let (batches, _) = run(&root, query, &SearchOptions::default());
        files(batches)
            .into_iter()
            .flat_map(|file| {
                let rel = file.rel;
                file.lines
                    .into_iter()
                    .map(move |line| (rel.clone(), line.ranges))
            })
            .collect()
    };
    // In crlf.txt the `\r` is the terminator's.
    assert_eq!(
        found(&SearchQuery::literal("needle\r")),
        [("end.txt".to_string(), vec![0..7])]
    );
    let regex = SearchQuery {
        regex: true,
        ..SearchQuery::literal("a.b")
    };
    assert_eq!(found(&regex), [("mid.txt".to_string(), vec![0..3])]);
}

/// Previews of generated lines mixing ASCII, multi-byte chars and invalid
/// bytes, against lossy decoding of the whole line.
#[test]
fn previews_agree_with_decoding_the_whole_line() {
    const PIECES: [&[u8]; 7] = [
        b"a",
        b"needle",
        "é".as_bytes(),
        "😀".as_bytes(),
        b"\xFF",
        b"\x80",
        b"\xE2\x82",
    ];
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = |below: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        usize::try_from(seed % u64::try_from(below).unwrap()).unwrap()
    };
    for _ in 0..2_000 {
        let mut line = Vec::new();
        for _ in 0..next(700) {
            line.extend(PIECES[next(PIECES.len())]);
        }
        let start = next(line.len() + 1);
        let first = start..start + next(line.len() - start + 1).min(20);
        let (preview, preview_start) = crate::search::preview(&line, &first);
        let whole = String::from_utf8_lossy(&line);
        let before = String::from_utf8_lossy(&line[..preview_start]);
        assert!(preview.len() <= 512, "{line:?} {first:?}");
        assert!(preview_start <= first.start, "{line:?} {first:?}");
        assert!(
            whole[before.len()..].starts_with(&preview),
            "the preview is the line's own decoding: {line:?} {first:?}"
        );
        if whole.len() <= 512 {
            assert_eq!((&*preview, preview_start), (&*whole, 0));
        }
    }
}

#[test]
fn searches_from_a_relative_root_filter_as_from_an_absolute_one() {
    let (_dir, root) = temp();
    write(
        &root,
        &[
            (".gitignore", b"*.log\n"),
            ("a.rs", b"needle\n"),
            ("b.log", b"needle\n"),
            ("sub/c.rs", b"needle\n"),
        ],
    );
    let query = SearchQuery::literal("needle");
    for options in [threads(1), threads(0)] {
        for from in [root.clone(), relative(&root)] {
            let (batches, _) = run(&from, &query, &options);
            assert_eq!(
                rels(&files(batches)),
                ["a.rs", "sub/c.rs"],
                "{}",
                from.display()
            );
        }
    }
}

/// Inline flags and text anchors see each line as the line matcher does:
/// without its terminator, with a `\r` not before `\n` kept as content. This
/// holds in UTF-16 files too.
#[test]
fn inline_flags_and_text_anchors_see_lines_without_terminators() {
    let (_dir, root) = temp();
    let utf16: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain("x\r\nfoo\r\n".encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    write(
        &root,
        &[
            ("mid.txt", b"a\rb\n"),
            ("end.txt", b"x\nneedle\r"),
            ("terminator.txt", b"needle\r\n"),
            ("crlf.txt", b"x\r\nfoo\r\n"),
            ("lf.txt", b"x\nfoo\n"),
            ("utf16.txt", &utf16),
        ],
    );
    let lines = |pattern: &str| -> Vec<(String, u64, Vec<std::ops::Range<usize>>)> {
        let query = SearchQuery {
            regex: true,
            ..SearchQuery::literal(pattern)
        };
        let (batches, _) = run(&root, &query, &SearchOptions::default());
        files(batches)
            .into_iter()
            .flat_map(|file| {
                let rel = file.rel;
                file.lines
                    .into_iter()
                    .map(move |line| (rel.clone(), line.line_number, line.ranges))
            })
            .collect()
    };
    let line = |rel: &str, number: u64, range: std::ops::Range<usize>| {
        (rel.to_string(), number, vec![range])
    };

    // `.` without dot-all still matches a `\r` that isn't the terminator's,
    // within a line or at the end of an unterminated one.
    for pattern in [r"(?-s)a.b", r"(?-s:a.b)"] {
        assert_eq!(lines(pattern), [line("mid.txt", 1, 0..3)], "{pattern}");
    }
    for pattern in [r"(?-s)needle.", r"(?-s:needle.)"] {
        assert_eq!(lines(pattern), [line("end.txt", 2, 0..7)], "{pattern}");
    }

    // Text anchors are at the line's ends, inside the terminator.
    for pattern in [
        r"foo\z",
        r"(?-m:foo$)",
        r"(?-m)foo$",
        r"\Afoo",
        r"(?-R)foo$",
    ] {
        assert_eq!(
            lines(pattern),
            [
                line("crlf.txt", 2, 0..3),
                line("lf.txt", 2, 0..3),
                line("utf16.txt", 2, 0..3),
            ],
            "{pattern}"
        );
    }
    assert_eq!(lines(r"needle\r\z"), [line("end.txt", 2, 0..7)]);
}
