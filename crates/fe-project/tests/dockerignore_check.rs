//! `check_ignore_file` on container ignore files: Docker's matching.

use std::{fs, path::Path, sync::atomic::AtomicBool};

use fe_project::{IgnoreFindingKind, IgnoreReport, check_ignore_file};
use tempfile::TempDir;

fn project(paths: &[&str]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for path in paths {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, "").unwrap();
    }
    dir
}

fn check(dir: &Path, text: &str) -> IgnoreReport {
    check_ignore_file(&dir.join(".dockerignore"), text, &AtomicBool::new(false))
}

fn kinds(report: &IgnoreReport) -> Vec<(usize, IgnoreFindingKind)> {
    report
        .findings
        .iter()
        .map(|f| (f.line, f.kind.clone()))
        .collect()
}

fn invalid(message: &str) -> IgnoreFindingKind {
    IgnoreFindingKind::Invalid(message.into())
}

#[test]
fn patterns_are_anchored_and_cleaned() {
    let dir = project(&["top.log", "a/top.log", "dist/x"]);
    let report = check(dir.path(), "*.log\n**/*.log\n/dist\n./dist\ndist/\n");
    assert_eq!(report.matches[0].total, 1);
    assert_eq!(report.matches[1].total, 2);
    assert_eq!(
        kinds(&report),
        [
            (4, IgnoreFindingKind::Duplicate { first_line: 3 }),
            (5, IgnoreFindingKind::Duplicate { first_line: 3 }),
        ]
    );
}

#[test]
fn go_match_syntax_and_errors() {
    let dir = project(&["1x", "ax", "a/b/c", "a/c"]);
    let text = "[^a-z]x\n[abc\nfoo\\\n!\n**/c\n";
    let report = check(dir.path(), text);
    assert_eq!(report.matches[0].total, 1);
    assert_eq!(report.matches[4].total, 2);
    assert_eq!(
        kinds(&report),
        [
            (2, invalid("syntax error in pattern")),
            (3, invalid("syntax error in pattern")),
            (4, invalid("illegal exclusion pattern")),
        ]
    );
}

#[test]
fn exception_reincludes_inside_excluded_directory() {
    let dir = project(&["docs/README.md", "docs/other.md"]);
    let report = check(dir.path(), "docs\n!docs/README.md\n");
    assert_eq!(kinds(&report), []);
    assert_eq!(report.matches[1].total, 1);
}

#[test]
fn unused_and_shadowed_use_docker_order() {
    let dir = project(&["a.txt", "b.txt"]);
    let report = check(dir.path(), "build\na.txt\n!a.txt\nb.txt\n!a.txt\n");
    assert_eq!(
        kinds(&report),
        [
            (1, IgnoreFindingKind::Unused),
            (2, IgnoreFindingKind::Shadowed { by_line: 5 }),
            (5, IgnoreFindingKind::Duplicate { first_line: 3 }),
        ]
    );
}

#[test]
fn container_files_share_the_rules() {
    let dir = project(&["a.txt"]);
    for name in [
        ".dockerignore",
        "Dockerfile.dockerignore",
        ".containerignore",
    ] {
        let report =
            check_ignore_file(&dir.path().join(name), "*/a.txt\n", &AtomicBool::new(false));
        assert_eq!(kinds(&report), [(1, IgnoreFindingKind::Unused)], "{name}");
    }
    let report = check_ignore_file(
        &dir.path().join(".gitignore"),
        "*/a.txt\n",
        &AtomicBool::new(false),
    );
    assert_eq!(kinds(&report), [(1, IgnoreFindingKind::Unused)]);
    let report = check_ignore_file(
        &dir.path().join(".gitignore"),
        "*.txt\n",
        &AtomicBool::new(false),
    );
    assert_eq!(kinds(&report), []);
}

#[test]
fn double_star_without_a_slash_is_a_run_of_characters() {
    // Windows drops a name's trailing space, so there the class pads its start.
    let (padded, class) = if cfg!(windows) {
        (" padded", "[ ]padded")
    } else {
        ("padded ", "padded[ ]")
    };
    let dir = project(&["top.log", "a/b/deep.log", padded, "x/barfoo"]);
    let report = check(dir.path(), &format!("**.log\n**foo\n{class}\n"));
    assert_eq!(report.matches[0].total, 2);
    assert_eq!(report.matches[1].total, 1);
    assert_eq!(report.matches[2].total, 1);
    assert_eq!(kinds(&report), []);
}
