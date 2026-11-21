//! Scanning a project for a pattern: the language filter, the walk's
//! ignore rules and path filter, streaming and cancelling, the caps.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use fe_project::PathFilter;
use fe_structural::{FileResult, Pattern, Rewrite, ScanOptions, ScanSummary, scan};

fn rust() -> &'static fe_syntax::Language {
    fe_syntax::language_by_name("rust").expect("a default feature of fe-syntax")
}

fn write(root: &Path, rel: &str, text: impl AsRef<[u8]>) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn run(
    root: &Path,
    pattern: &Pattern,
    options: &ScanOptions,
    filter: &PathFilter,
) -> (Vec<FileResult>, ScanSummary) {
    let mut files = Vec::new();
    let summary = scan(
        root,
        pattern,
        None,
        options,
        filter,
        Arc::new(AtomicBool::new(false)),
        |batch| files.extend(batch),
    );
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    (files, summary)
}

fn rels(files: &[FileResult]) -> Vec<PathBuf> {
    files.iter().map(|file| file.rel.clone()).collect()
}

#[test]
fn scan_finds_matches_across_files_of_the_chosen_language_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "a.rs", "fn main() {\n    let x = foo(1);\n}\n");
    write(root, "src/b.rs", "fn b() { foo(2); foo(3) }\n");
    write(root, "c.rs", "fn c() { bar(4); }\n");
    // The same text in Python: not searched.
    write(root, "d.py", "x = foo(1)\nfoo(2)\n");
    let pattern = Pattern::compile(rust(), "foo($A)").unwrap();
    let (files, summary) = run(
        root,
        &pattern,
        &ScanOptions::default(),
        &PathFilter::default(),
    );
    assert_eq!(
        rels(&files),
        [PathBuf::from("a.rs"), PathBuf::from("src/b.rs")]
    );
    let a = &files[0].matches;
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].first_line, "foo(1)");
    assert_eq!((a[0].start_line, a[0].start_col), (1, 12));
    assert_eq!(a[0].line_count, 1);
    assert_eq!(a[0].captures, [("A".to_owned(), "1".to_owned())]);
    assert_eq!(a[0].rewritten, None);
    let b: Vec<_> = files[1]
        .matches
        .iter()
        .map(|m| m.captures[0].1.as_str())
        .collect();
    assert_eq!(b, ["2", "3"]);
    assert_eq!(files[0].path, root.join("a.rs"));
    assert_eq!(summary.files_walked, 4);
    assert_eq!(summary.files_of_language, 3);
    assert_eq!(summary.matches, 3);
    assert!(!summary.cancelled && !summary.truncated);
}

#[test]
fn scan_reports_rewrites_and_multi_line_matches() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "a.rs", "fn a() {\n    foo(\n        1\n    );\n}\n");
    let pattern = Pattern::compile(rust(), "foo($A)").unwrap();
    let rewrite = Rewrite::compile(&pattern, "bar($A)").unwrap();
    let mut files = Vec::new();
    let summary = scan(
        root,
        &pattern,
        Some(&rewrite),
        &ScanOptions::default(),
        &PathFilter::default(),
        Arc::new(AtomicBool::new(false)),
        |batch| files.extend(batch),
    );
    assert_eq!(summary.matches, 1);
    let row = &files[0].matches[0];
    assert_eq!(row.first_line, "foo(");
    assert_eq!(row.line_count, 3);
    assert_eq!(row.rewritten.as_deref(), Some("bar(1)"));
    assert!(!row.flagged);

    // A rewrite for another pattern doesn't fit this one: nothing scanned.
    let other = Pattern::compile(rust(), "baz($B)").unwrap();
    let wrong = Rewrite::compile(&other, "qux($B)").unwrap();
    let summary = scan(
        root,
        &pattern,
        Some(&wrong),
        &ScanOptions::default(),
        &PathFilter::default(),
        Arc::new(AtomicBool::new(false)),
        |_| panic!("nothing is scanned"),
    );
    assert!(summary.rewrite_error.is_some());
    assert_eq!(summary.files_walked, 0);
}

#[test]
fn scan_respects_gitignore_and_path_filter() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", "target/\nskipped.rs\n");
    write(root, "target/debug/gen.rs", "fn g() { foo(1); }\n");
    write(root, "skipped.rs", "fn s() { foo(1); }\n");
    write(root, "src/kept.rs", "fn k() { foo(1); }\n");
    write(root, "src/tests/t.rs", "fn t() { foo(1); }\n");
    write(root, "examples/e.rs", "fn e() { foo(1); }\n");
    let pattern = Pattern::compile(rust(), "foo($A)").unwrap();
    let options = ScanOptions::default();

    let (files, summary) = run(root, &pattern, &options, &PathFilter::default());
    assert_eq!(
        rels(&files),
        [
            PathBuf::from("examples/e.rs"),
            PathBuf::from("src/kept.rs"),
            PathBuf::from("src/tests/t.rs"),
        ]
    );
    // The .gitignore itself is walked; the ignored files aren't.
    assert_eq!(summary.files_walked, 4);

    let filter = PathFilter::new(root, &["src/**"], &["tests/"]).unwrap();
    let (files, _) = run(root, &pattern, &options, &filter);
    assert_eq!(rels(&files), [PathBuf::from("src/kept.rs")]);
}

#[test]
fn scan_streams_batches_to_the_sink_per_file_and_stops_on_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for ix in 0..20 {
        let calls = "    foo(1);\n".repeat(ix % 3 + 1);
        write(
            root,
            &format!("f{ix:02}.rs"),
            format!("fn f() {{\n{calls}}}\n"),
        );
    }
    write(root, "none.rs", "fn n() {}\n");
    let pattern = Pattern::compile(rust(), "foo($A)").unwrap();

    // Every file with matches comes once, whole, over one or more batches.
    for threads in [1, 4] {
        let options = ScanOptions {
            threads: Some(threads),
            ..ScanOptions::default()
        };
        let mut batches = Vec::new();
        let summary = scan(
            root,
            &pattern,
            None,
            &options,
            &PathFilter::default(),
            Arc::new(AtomicBool::new(false)),
            |batch| batches.push(batch),
        );
        assert!(batches.iter().all(|batch| !batch.is_empty()));
        let mut files: Vec<_> = batches.into_iter().flatten().collect();
        files.sort_by(|a, b| a.rel.cmp(&b.rel));
        assert_eq!(files.len(), 20, "{threads} threads");
        for (ix, file) in files.iter().enumerate() {
            assert_eq!(file.rel, PathBuf::from(format!("f{ix:02}.rs")));
            assert_eq!(file.matches.len(), ix % 3 + 1);
        }
        assert_eq!(summary.matches, 39);
        assert!(!summary.cancelled);
    }

    // Cancelled from the sink: no batch comes after. More files than a
    // batch holds, so the first can't be all of them.
    for ix in 20..100 {
        write(root, &format!("f{ix:02}.rs"), "fn f() { foo(1); }\n");
    }
    for threads in [1, 4] {
        let options = ScanOptions {
            threads: Some(threads),
            ..ScanOptions::default()
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let mut calls = 0;
        let mut seen = 0;
        let summary = scan(
            root,
            &pattern,
            None,
            &options,
            &PathFilter::default(),
            cancel.clone(),
            |batch| {
                calls += 1;
                seen += batch.len();
                cancel.store(true, Ordering::Relaxed);
            },
        );
        assert_eq!(calls, 1, "{threads} threads");
        assert!(seen < 100);
        assert!(summary.matches < 119);
        assert!(summary.cancelled);
    }

    // Cancelled before the start: nothing at all.
    let summary = scan(
        root,
        &pattern,
        None,
        &ScanOptions::default(),
        &PathFilter::default(),
        Arc::new(AtomicBool::new(true)),
        |_| panic!("no batch once cancelled"),
    );
    assert!(summary.cancelled);
    assert_eq!(summary.files_parsed, 0);
}

#[test]
fn scan_stops_at_max_matches_and_sets_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for ix in 0..10 {
        write(
            root,
            &format!("f{ix}.rs"),
            "fn f() { foo(1); foo(2); foo(3); }\n",
        );
    }
    let pattern = Pattern::compile(rust(), "foo($A)").unwrap();
    for threads in [1, 4] {
        let options = ScanOptions {
            threads: Some(threads),
            max_matches: 7,
            ..ScanOptions::default()
        };
        let (files, summary) = run(root, &pattern, &options, &PathFilter::default());
        let reported: usize = files.iter().map(|file| file.matches.len()).sum();
        assert_eq!(reported, 7, "{threads} threads");
        assert_eq!(summary.matches, 7);
        assert!(summary.truncated);
        assert!(!summary.cancelled);
        if threads == 1 {
            // In path order: two whole files, then the first match of the third.
            assert_eq!(
                files.iter().map(|f| f.matches.len()).collect::<Vec<_>>(),
                [3, 3, 1]
            );
            assert_eq!(files[2].matches[0].captures[0].1, "1");
        }
    }

    // Exactly as many as the cap: not truncated.
    let options = ScanOptions {
        threads: Some(1),
        max_matches: 30,
        ..ScanOptions::default()
    };
    let (_, summary) = run(root, &pattern, &options, &PathFilter::default());
    assert_eq!(summary.matches, 30);
    assert!(!summary.truncated);
}

#[test]
fn scan_skips_files_over_the_size_cap_and_unparsable_files_and_reports_them_in_the_summary() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "ok.rs", "fn ok() { foo(1); }\n");
    let big = format!("fn big() {{ foo(1); }}\n{}", "// padding\n".repeat(200));
    write(root, "big.rs", &big);
    // Not UTF-8: a stray byte, and UTF-16 with its byte order mark.
    write(root, "latin1.rs", b"fn l() { foo(1); } // caf\xe9\n");
    let utf16: Vec<u8> = "fn u() { foo(1); }\n"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    write(root, "utf16.rs", [&[0xff, 0xfe][..], &utf16].concat());
    // UTF-16 without a byte order mark, both orders: its ASCII is valid
    // UTF-8 between the zero bytes.
    write(root, "utf16le.rs", &utf16);
    let utf16be: Vec<u8> = "fn u() { foo(1); }\n"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    write(root, "utf16be.rs", &utf16be);
    // A file the parser gives up on.
    fe_structural::testing::fail_parses_named("gives_up.rs");
    write(root, "gives_up.rs", "fn g() { foo(1); }\n");
    let pattern = Pattern::compile(rust(), "foo($A)").unwrap();
    let options = ScanOptions {
        max_file_bytes: 1024,
        ..ScanOptions::default()
    };
    let (files, summary) = run(root, &pattern, &options, &PathFilter::default());
    assert_eq!(rels(&files), [PathBuf::from("ok.rs")]);
    assert_eq!(summary.files_of_language, 7);
    assert_eq!(summary.too_large, 1);
    assert_eq!(summary.non_utf8, 4);
    assert_eq!(summary.files_parsed, 2);
    assert_eq!(summary.parse_failed, 1);
    assert_eq!(summary.timed_out, 0);
    assert!(summary.errors.is_empty());

    // With no time to parse in, every file runs out of it.
    let options = ScanOptions {
        file_deadline: Duration::ZERO,
        ..options
    };
    let (files, summary) = run(root, &pattern, &options, &PathFilter::default());
    assert!(files.is_empty());
    assert_eq!(summary.timed_out, 2);
    assert_eq!(summary.parse_failed, 0);
    assert_eq!(summary.too_large, 1);
    assert_eq!(summary.non_utf8, 4);
}

#[test]
fn scan_with_one_thread_is_in_path_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let names = ["a.rs", "b/a.rs", "b/c/z.rs", "b/d.rs", "c.rs", "z/a.rs"];
    // Written out of order, so the order isn't the directory's creation order.
    for name in names.iter().rev() {
        write(root, name, "fn f() { foo(1); }\n");
    }
    let pattern = Pattern::compile(rust(), "foo($A)").unwrap();
    let options = ScanOptions {
        threads: Some(1),
        ..ScanOptions::default()
    };
    let mut order = Vec::new();
    scan(
        root,
        &pattern,
        None,
        &options,
        &PathFilter::default(),
        Arc::new(AtomicBool::new(false)),
        |batch| order.extend(batch.into_iter().map(|file| file.rel)),
    );
    let expected: Vec<PathBuf> = names.iter().map(PathBuf::from).collect();
    assert_eq!(order, expected);
}
