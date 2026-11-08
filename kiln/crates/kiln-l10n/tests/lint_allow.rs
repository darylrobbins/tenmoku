// SPDX-License-Identifier: Apache-2.0

//! The allow-list's fixtures: `l10n-allow.toml`'s append-only `migrated`
//! list, `all = true`, and entries that must each give a reason. The fixture
//! crate is `tests/fixtures/allow`: `src/a.rs`, `src/b.rs`, `src/b/inner.rs`
//! and `src/renamed.rs` (through `#[path]`) each hold reported text, and
//! `src/tests.rs` is reached only under `cfg(test)`.

use std::path::PathBuf;

use kiln_l10n::lint::{LiteralReport, literals};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/allow")
}

fn run(allow_list: &str) -> LiteralReport {
    literals(fixture()).allow_list(allow_list).report()
}

fn files_reported(report: &LiteralReport) -> Vec<&str> {
    let mut files: Vec<&str> = report.findings.iter().map(|f| f.file.as_str()).collect();
    files.dedup();
    files
}

#[test]
fn with_migrated_only_the_listed_file_is_checked() {
    let report = run("migrated.toml");
    assert!(report.errors.is_empty(), "{report}");
    assert_eq!(report.checked, ["src/a.rs"]);
    assert_eq!(files_reported(&report), ["src/a.rs"]);
    assert_eq!(report.findings[0].text, "Open File");
}

#[test]
fn a_card_that_appends_a_line_switches_that_file_on() {
    let report = run("migrated-two.toml");
    assert!(report.errors.is_empty(), "{report}");
    assert_eq!(report.checked, ["src/a.rs", "src/b/inner.rs"]);
    assert_eq!(files_reported(&report), ["src/a.rs", "src/b/inner.rs"]);
}

#[test]
fn with_all_every_non_test_file_is_checked() {
    let report = run("all.toml");
    assert!(report.errors.is_empty(), "{report}");
    let mut checked = report.checked.clone();
    checked.sort();
    assert_eq!(
        checked,
        [
            "src/a.rs",
            "src/b.rs",
            "src/b/inner.rs",
            "src/lib.rs",
            "src/renamed.rs"
        ]
    );
    let mut files = files_reported(&report);
    files.sort();
    assert_eq!(
        files,
        ["src/a.rs", "src/b.rs", "src/b/inner.rs", "src/renamed.rs"]
    );
    assert!(
        report.findings.iter().all(|f| f.text != "Test Only"),
        "test-only modules are never checked: {report}"
    );
}

#[test]
fn file_and_literal_entries_with_reasons_are_allowed() {
    let report = run("allowed.toml");
    assert!(report.errors.is_empty(), "{report}");
    assert!(!report.checked.contains(&"src/renamed.rs".to_string()));
    let texts: Vec<&str> = report.findings.iter().map(|f| f.text.as_str()).collect();
    assert_eq!(texts, ["Open File", "Inner Text"]);
}

#[test]
fn an_entry_without_a_reason_is_an_error() {
    let report = run("file-without-reason.toml");
    assert_eq!(report.errors.len(), 1, "{report}");
    assert!(
        report.errors[0].contains("[[file]] `src/renamed.rs` has no reason"),
        "{report}"
    );
    assert!(!report.is_clean());

    let report = run("literal-without-reason.toml");
    assert_eq!(report.errors.len(), 1, "{report}");
    assert!(
        report.errors[0].contains("`Close File` in `src/b.rs` has no reason"),
        "{report}"
    );
}

#[test]
fn the_list_must_say_which_files_it_checks_and_only_one_way() {
    let report = run("neither.toml");
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("neither `all = true` nor `migrated")),
        "{report}"
    );
    let report = run("both.toml");
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("both `all` and `migrated`")),
        "{report}"
    );
}

#[test]
fn an_entry_naming_no_source_file_is_an_error() {
    let report = run("not-source.toml");
    assert_eq!(report.errors.len(), 2, "{report}");
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("`src/tests.rs` isn't non-test source")),
        "{report}"
    );
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("`src/missing.rs` isn't non-test source")),
        "{report}"
    );
}

#[test]
fn a_literal_entry_that_matches_nothing_is_an_error() {
    let report = run("stale.toml");
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("\"Gone Now\" in `src/a.rs` matches nothing")),
        "{report}"
    );
}

#[test]
fn a_missing_allow_list_is_an_error() {
    let report = run("nonexistent.toml");
    assert_eq!(report.errors.len(), 1, "{report}");
    assert!(report.checked.is_empty());
}

#[test]
#[should_panic(expected = "Open File")]
fn check_panics_with_the_findings() {
    literals(fixture()).allow_list("migrated.toml").check();
}
