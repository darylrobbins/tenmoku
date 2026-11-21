//! The scan's literal prefilter: files without the pattern's literals are
//! never parsed.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use fe_project::PathFilter;
use fe_structural::{FileResult, Pattern, ScanOptions, ScanSummary, prefilter_literals, scan};

fn rust() -> &'static fe_syntax::Language {
    fe_syntax::language_by_name("rust").expect("a default feature of fe-syntax")
}

fn write(root: &Path, rel: &str, text: &str) {
    fs::write(root.join(rel), text).unwrap();
}

fn run(root: &Path, pattern: &Pattern) -> (Vec<FileResult>, ScanSummary) {
    let mut files = Vec::new();
    let summary = scan(
        root,
        pattern,
        None,
        &ScanOptions::default(),
        &PathFilter::default(),
        Arc::new(AtomicBool::new(false)),
        |batch| files.extend(batch),
    );
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    (files, summary)
}

#[test]
fn prefilter_skips_parsing_files_without_the_patterns_literals() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "both.rs", "fn a() { let v = x.unwrap_or(y); }\n");
    // `unwrap_or` and `x` both there, but not as a call: parsed, no match.
    write(root, "words.rs", "// x unwrap_or\nfn b() {}\n");
    write(root, "one.rs", "fn c() { x.unwrap(); }\n");
    write(root, "neither.rs", "fn d() {}\n");
    let pattern = Pattern::compile(rust(), "x.unwrap_or($D)").unwrap();
    assert_eq!(prefilter_literals(&pattern), ["unwrap_or", "x"]);
    let (files, summary) = run(root, &pattern);
    assert!(summary.prefiltered);
    assert_eq!(summary.files_of_language, 4);
    assert_eq!(summary.files_prefiltered_out, 2);
    assert_eq!(summary.files_parsed, 2);
    let rels: Vec<_> = files.iter().map(|file| file.rel.clone()).collect();
    assert_eq!(rels, [PathBuf::from("both.rs")]);
    assert_eq!(files[0].matches[0].captures, [("D".into(), "y".into())]);
}

#[test]
fn prefilter_needs_named_leaves_only() {
    // Punctuation and keywords match by kind, so they aren't required as
    // text; a comment in the pattern is skipped, so it isn't either.
    let pattern = Pattern::compile(rust(), "return foo($A) /* why */").unwrap();
    assert_eq!(prefilter_literals(&pattern), ["foo"]);
}

#[test]
fn pattern_of_only_metavariables_has_no_prefilter_and_reports_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "a.rs", "fn a() { f(1); }\n");
    write(root, "b.rs", "fn b() { g(2, 3); }\n");
    write(root, "c.rs", "fn c() {}\n");
    let pattern = Pattern::compile(rust(), "$F($A)").unwrap();
    assert!(prefilter_literals(&pattern).is_empty());
    let (files, summary) = run(root, &pattern);
    assert!(!summary.prefiltered);
    assert_eq!(summary.files_prefiltered_out, 0);
    assert_eq!(summary.files_parsed, 3);
    let rels: Vec<_> = files.iter().map(|file| file.rel.clone()).collect();
    assert_eq!(rels, [PathBuf::from("a.rs")]);
}
