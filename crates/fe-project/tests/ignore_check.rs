//! `check_ignore_file` against small trees on disk.

#[cfg(unix)]
use std::{collections::BTreeSet, process::Command};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use fe_project::{
    IgnoreFinding, IgnoreFindingKind, IgnoreReport, IgnoreSeverity, check_ignore_file,
    check_ignore_file_capped, fixture,
};
use tempfile::TempDir;

/// A project holding `paths`: directories when they end in `/`, else empty
/// files.
fn project(paths: &[&str]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for path in paths {
        let full = dir.path().join(path);
        if path.ends_with('/') {
            fs::create_dir_all(full).unwrap();
        } else {
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, "").unwrap();
        }
    }
    dir
}

fn check(dir: &Path, name: &str, text: &str) -> IgnoreReport {
    check_ignore_file(&dir.join(name), text, &AtomicBool::new(false))
}

/// The findings as (line, kind).
fn kinds(report: &IgnoreReport) -> Vec<(usize, IgnoreFindingKind)> {
    report
        .findings
        .iter()
        .map(|finding| (finding.line, finding.kind.clone()))
        .collect()
}

fn paths(report: &IgnoreReport, line: usize) -> Vec<PathBuf> {
    report.matches[line - 1].paths.clone()
}

fn invalid_message(finding: &IgnoreFinding) -> &str {
    match &finding.kind {
        IgnoreFindingKind::Invalid(message) => message,
        other => panic!("not invalid: {other:?}"),
    }
}

#[test]
fn unused_pattern_is_reported() {
    let dir = project(&["src/a.txt"]);
    let text = "nothing_here/\nsrc/\n";
    let report = check(dir.path(), ".gitignore", text);
    assert_eq!(kinds(&report), [(1, IgnoreFindingKind::Unused)]);
    let finding = &report.findings[0];
    assert_eq!(&text[finding.range.clone()], "nothing_here/");
    assert_eq!(finding.severity, IgnoreSeverity::Hint);
    assert!(!report.truncated);
    assert!(!report.cancelled);
}

#[test]
fn a_pattern_for_an_ignored_directory_counts_as_used() {
    // The tree and the finder hide these paths; the walk doesn't.
    let dir = project(&["node_modules/x/index.js"]);
    let report = check(dir.path(), ".gitignore", "node_modules/\n");
    assert_eq!(kinds(&report), []);
    assert_eq!(paths(&report, 1), [PathBuf::from("node_modules")]);
}

#[test]
fn a_directory_pattern_does_not_match_a_file() {
    let dir = project(&["src"]);
    let report = check(dir.path(), ".gitignore", "src/\n");
    assert_eq!(kinds(&report), [(1, IgnoreFindingKind::Unused)]);
}

#[test]
fn git_metadata_is_not_walked() {
    let dir = project(&[".git/config", "sub/.git"]);
    let report = check(dir.path(), ".gitignore", "config\n.git\n");
    assert_eq!(
        kinds(&report),
        [
            (1, IgnoreFindingKind::Unused),
            (2, IgnoreFindingKind::Unused)
        ]
    );
}

#[test]
fn duplicate_and_shadowed_lines_are_reported() {
    let dir = project(&["a.log", "keep.log"]);
    let report = check(dir.path(), ".gitignore", "*.log\n!keep.log\n*.log\n");
    assert_eq!(
        kinds(&report),
        [
            (2, IgnoreFindingKind::Shadowed { by_line: 3 }),
            (3, IgnoreFindingKind::Duplicate { first_line: 1 }),
        ]
    );
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.severity == IgnoreSeverity::Warning)
    );

    let report = check(dir.path(), ".gitignore", "a.log  \nkeep.log\na.log\n");
    assert_eq!(
        kinds(&report),
        [(3, IgnoreFindingKind::Duplicate { first_line: 1 })]
    );
}

#[test]
fn shadowing_needs_every_path_overridden_by_the_other_effect() {
    let dir = project(&["a.log", "b.log"]);
    // A negation decided by a later ignore, and an ignore by a later negation.
    let report = check(dir.path(), ".gitignore", "*.log\n!*.log\n");
    assert_eq!(
        kinds(&report),
        [(1, IgnoreFindingKind::Shadowed { by_line: 2 })]
    );
    // b.log is still decided by the first line.
    let report = check(dir.path(), ".gitignore", "*.log\n!a.log\n");
    assert_eq!(kinds(&report), []);
    // A later line of the same effect leaves the outcome as this line had it.
    let report = check(dir.path(), ".gitignore", "a.log\n*.log\n");
    assert_eq!(kinds(&report), []);
    // Reported by the last line that decides one of its paths.
    let report = check(dir.path(), ".gitignore", "*.log\n!a.log\n!b.log\n");
    assert_eq!(
        kinds(&report),
        [(1, IgnoreFindingKind::Shadowed { by_line: 3 })]
    );
    let report = check(dir.path(), ".gitignore", "*.log\n!b.log\n!a.log\n");
    assert_eq!(
        kinds(&report),
        [(1, IgnoreFindingKind::Shadowed { by_line: 3 })]
    );
}

#[test]
fn invalid_patterns_are_reported() {
    let dir = project(&["src/a.txt", "b.txt"]);
    let text = "[abc\nb.txt\nsrc/\n!src/a.txt\n";
    let report = check(dir.path(), ".gitignore", text);
    assert_eq!(report.findings.len(), 2, "{report:?}");

    let syntax = &report.findings[0];
    assert_eq!(syntax.line, 1);
    assert_eq!(&text[syntax.range.clone()], "[abc");
    assert_eq!(syntax.severity, IgnoreSeverity::Error);
    assert!(invalid_message(syntax).contains("unclosed"), "{syntax:?}");

    let negation = &report.findings[1];
    assert_eq!(negation.line, 4);
    assert_eq!(&text[negation.range.clone()], "!src/a.txt");
    assert_eq!(negation.severity, IgnoreSeverity::Warning);
    let message = invalid_message(negation);
    assert!(message.contains("re-include"), "{message}");
    assert!(message.contains("`src`"), "{message}");
    assert!(message.contains("line 3"), "{message}");
}

#[test]
fn reinclusion_is_decided_as_git_decides_the_parent() {
    let dir = project(&["src/a.txt", "src/b.txt"]);
    let reincludes = |text: &str| {
        check(dir.path(), ".gitignore", text)
            .findings
            .iter()
            .any(|finding| {
                matches!(&finding.kind, IgnoreFindingKind::Invalid(message)
                    if message.contains("re-include"))
            })
    };
    // No such path need exist.
    assert!(reincludes("src/\n!src/missing.txt\n"));
    // A literal parent before a wildcard.
    assert!(reincludes("src/\n!src/*.txt\n"));
    // An exclusion after the negation holds too.
    assert!(reincludes("!src/a.txt\nsrc/\n"));
    // A pattern that reaches only into an excluded directory.
    assert!(reincludes("src/\n!a.txt\n"));
    // The parent isn't excluded, or a later line lets it back in.
    assert!(!reincludes("src/*\n!src/a.txt\n"));
    assert!(!reincludes("src/\n!src/\n!src/a.txt\n"));
    assert!(!reincludes("src/\n!src/a.txt\n!src/\n"));
    // Not a negation.
    assert!(!reincludes("src/\nsrc/a.txt\n"));
}

#[test]
fn a_negation_that_reaches_a_path_outside_excluded_directories_stands() {
    let dir = project(&["build/a.txt", "a.txt", "b.txt"]);
    let report = check(dir.path(), ".gitignore", "build/\n*.txt\n!a.txt\n");
    assert_eq!(kinds(&report), []);
}

#[test]
fn nested_ignore_file_is_based_at_its_directory() {
    let dir = project(&["build/a.o", "sub/build/b.o", "sub/src/build/c.o"]);
    let report = check(
        dir.path(),
        "sub/.gitignore",
        "build/\n/src/build\n/build/\n",
    );
    assert_eq!(kinds(&report), []);
    assert_eq!(
        paths(&report, 1),
        [PathBuf::from("build"), PathBuf::from("src/build")]
    );
    assert_eq!(paths(&report, 2), [PathBuf::from("src/build")]);
    assert_eq!(paths(&report, 3), [PathBuf::from("build")]);

    // A root-level build/ doesn't count for a nested file.
    let dir = project(&["build/a.o", "sub/src/a.rs"]);
    let report = check(dir.path(), "sub/.gitignore", "build/\n");
    assert_eq!(kinds(&report), [(1, IgnoreFindingKind::Unused)]);
}

#[test]
fn a_repositorys_exclude_file_is_based_at_its_worktree() {
    let dir = project(&["src/a.txt", ".git/info/exclude"]);
    let report = check(dir.path(), ".git/info/exclude", "/src/\nmissing\n");
    assert_eq!(kinds(&report), [(2, IgnoreFindingKind::Unused)]);
}

#[test]
fn a_relative_path_is_based_at_its_directory() {
    // Tests run in the crate's directory.
    let cancel = AtomicBool::new(false);
    let unused = [(2, IgnoreFindingKind::Unused)];
    for (path, text, matched) in [
        (".gitignore", "/Cargo.toml\nno_such_path\n", "Cargo.toml"),
        ("./.gitignore", "/Cargo.toml\nno_such_path\n", "Cargo.toml"),
        ("src/.gitignore", "/lib.rs\nno_such_path\n", "lib.rs"),
        ("./src/.gitignore", "/lib.rs\nno_such_path\n", "lib.rs"),
    ] {
        let report = check_ignore_file(Path::new(path), text, &cancel);
        assert_eq!(kinds(&report), unused, "{path}");
        assert_eq!(paths(&report, 1), [PathBuf::from(matched)], "{path}");
    }
}

#[test]
fn truncated_or_cancelled_scan_reports_no_unused_lines() {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 2, 3).unwrap();
    let file = dir.path().join(".gitignore");
    let text = "nothing_here/\nignored/\n!ignored_1.txt\ndir_0000\ndir_0000\n[abc\n";
    let not_cancelled = AtomicBool::new(false);

    // These checks need no walk, so they stand however the walk ends.
    let without_a_walk = [
        (5, IgnoreFindingKind::Duplicate { first_line: 4 }),
        (
            6,
            IgnoreFindingKind::Invalid("unclosed character class; missing ']'".to_string()),
        ),
    ];

    let report = check_ignore_file_capped(&file, text, &not_cancelled, 2);
    assert!(report.truncated);
    assert!(!report.cancelled);
    assert_eq!(kinds(&report), without_a_walk);
    assert_eq!(report.matches[0].total, 0);

    let report = check_ignore_file(&file, text, &AtomicBool::new(true));
    assert!(report.cancelled);
    assert!(!report.truncated);
    assert_eq!(kinds(&report), without_a_walk);

    // At the cap exactly, nothing was left out. The tree has 2 directories of
    // 3 files, `ignored` with 10, `.gitignore`.
    let report = check_ignore_file_capped(&file, text, &not_cancelled, 2 + 6 + 1 + 10 + 1);
    assert!(!report.truncated);
    assert_eq!(report.findings[0].kind, IgnoreFindingKind::Unused);
    let report = check_ignore_file_capped(&file, text, &not_cancelled, 2 + 6 + 1 + 10);
    assert!(report.truncated);
}

#[cfg(unix)]
#[test]
fn an_unreadable_directory_leaves_the_scan_incomplete() {
    use std::os::unix::fs::PermissionsExt;

    let dir = project(&["locked/a.txt"]);
    let locked = dir.path().join("locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read_dir(&locked).is_ok();
    let report = check(dir.path(), ".gitignore", "a.txt\n");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        return; // Permissions don't bind (root).
    }
    assert!(report.truncated);
    assert_eq!(kinds(&report), []);
}

#[test]
fn a_missing_directory_reports_no_unused_lines() {
    let dir = tempfile::tempdir().unwrap();
    let report = check(&dir.path().join("gone"), ".gitignore", "a\n");
    assert!(report.truncated);
    assert_eq!(kinds(&report), []);
}

#[test]
fn dockerignore_is_checked_with_docker_rules() {
    let dir = project(&["src/a.txt"]);
    let text = "nothing_here/\n[abc\nx\nx\n";
    for name in [
        ".dockerignore",
        "Dockerfile.dockerignore",
        ".containerignore",
    ] {
        let report = check(dir.path(), name, text);
        assert_eq!(
            kinds(&report),
            [
                (1, IgnoreFindingKind::Unused),
                (
                    2,
                    IgnoreFindingKind::Invalid("syntax error in pattern".into())
                ),
                (3, IgnoreFindingKind::Unused),
                (4, IgnoreFindingKind::Duplicate { first_line: 3 }),
            ],
            "{name}"
        );
    }
    for name in [".gitignore", ".ignore", ".npmignore", ".eslintignore"] {
        let report = check(dir.path(), name, text);
        assert_eq!(report.findings.len(), 4, "{name}: {report:?}");
    }
}

#[test]
// Windows strips a trailing space from a file name, which these fixtures need.
#[cfg(unix)]
fn line_endings_comments_and_byte_order_marks_are_read_as_git_reads_them() {
    let dir = project(&["a.txt", "#b", "c ", "!d"]);
    let text = "\u{feff}missing  \r\n# comment\r\n\r\n   \n\\#b\n\\!d\nc\\ \na.txt\r\nnope";
    let report = check(dir.path(), ".gitignore", text);
    assert_eq!(
        kinds(&report),
        [
            (1, IgnoreFindingKind::Unused),
            (9, IgnoreFindingKind::Unused)
        ]
    );
    assert_eq!(&text[report.findings[0].range.clone()], "missing");
    assert_eq!(&text[report.findings[1].range.clone()], "nope");
    assert_eq!(report.matches.len(), 9);
    assert_eq!(report.matches[1].total, 0);
    assert_eq!(paths(&report, 5), [PathBuf::from("#b")]);
    assert_eq!(paths(&report, 6), [PathBuf::from("!d")]);
    assert_eq!(paths(&report, 7), [PathBuf::from("c ")]);
    assert_eq!(paths(&report, 8), [PathBuf::from("a.txt")]);

    assert_eq!(check(dir.path(), ".gitignore", ""), IgnoreReport::default());
}

#[test]
fn matches_list_the_first_paths_and_count_them_all() {
    let names: Vec<String> = (0..60).map(|ix| format!("f{ix:02}.log")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let dir = project(&names);
    let report = check(dir.path(), ".gitignore", "*.log\n");
    let matched = &report.matches[0];
    assert_eq!(matched.total, 60);
    assert_eq!(matched.paths.len(), 50);
    assert_eq!(matched.paths[0], PathBuf::from("f00.log"));
    assert_eq!(matched.paths[49], PathBuf::from("f49.log"));
}

/// What git's own matcher takes each of a flat directory's files to match, one
/// pattern at a time, for what the check's rewriting has to agree with.
#[test]
// Windows strips a trailing space from a file name, which these fixtures need.
#[cfg(unix)]
fn single_patterns_match_what_git_matches() {
    let files = [
        "a.txt",
        "b.txt",
        "a1",
        "ab",
        "x.log",
        "y.tmp",
        "foo{",
        "{a,b}.txt",
        "]",
        "a]",
        "a-",
        "b ",
        "!c",
        "#c",
        "c",
    ];
    let dir = project(&files);
    let patterns = [
        "*.txt",
        "a?",
        "foo{",
        "{a,b}.txt",
        "*.{log,tmp}",
        "a[1]",
        "a[!1]",
        "a[^1]",
        "a[[:digit:]]",
        "a[[:alpha:]]",
        "a[[:punct:]]",
        "a[\\]]",
        "a[]]",
        "a[\\-]",
        "[\\!a]1",
        "[\\^a]1",
        "[\\!]c",
        "[\\!\\-]c",
        "a[[:graph:]]",
        "[\\a]1",
        "a[b-]",
        "b\\ ",
        "b\\   ",
        "\\!c",
        "\\#c",
        "/c",
        "**/c",
        "c/",
        "!c",
        "!",
        "/",
        "//",
        "!/",
        "foo\\",
        "[abc",
        "a[[:nope:]]",
        "[z-a]1",
    ];
    let git = |args: &[&str]| {
        Command::new("git")
            .args(["-c", "core.ignoreCase=false"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .current_dir(dir.path())
            .output()
            .unwrap()
    };
    assert!(git(&["init", "-q", "."]).status.success());
    for pattern in patterns {
        fs::write(dir.path().join(".gitignore"), format!("{pattern}\n")).unwrap();
        // `-v` shows a negated match too, so that `!c` is comparable; `-n`
        // lists the rest, with an empty source: `::\t<path>`.
        let output = git(&[
            &["check-ignore", "--no-index", "-v", "-n", "--"],
            &files[..],
        ]
        .concat());
        // 1 means nothing was ignored.
        assert!(
            output.status.code().is_some_and(|code| code <= 1),
            "{output:?}"
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let expected: BTreeSet<PathBuf> = stdout
            .lines()
            .filter(|line| !line.starts_with("::\t"))
            .map(|line| PathBuf::from(line.rsplit('\t').next().unwrap()))
            .collect();

        let report = check(dir.path(), ".gitignore", &format!("{pattern}\n"));
        let actual: BTreeSet<PathBuf> = report.matches[0]
            .paths
            .iter()
            .filter(|path| files.iter().any(|file| Path::new(file) == *path))
            .cloned()
            .collect();
        assert_eq!(actual, expected, "pattern {pattern:?}");
    }
}
