//! A commit's multi-file diff model: its sections, their texts read lazily
//! near the viewport, collapsing, and files it doesn't diff, on
//! repositories built with the `git` CLI in temporary directories.

use std::{fs, path::Path, process::Command, sync::Arc};

use fe_editor::text_diff::MAX_DIFF_BYTES;
use fe_git::{ChangeKind, LogScope, Repo};
use gpui::{AppContext as _, Entity, TestAppContext};

use super::{CommitDiff, LOADING, Line, NotShown, Row, Texts};

/// Runs `git` in `dir`, with the developer's own config shut out, and
/// gives what it printed.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn write(root: &Path, file: &str, contents: impl AsRef<[u8]>) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn commit(root: &Path, summary: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", summary]);
}

/// A repository whose newest commit edits `a.rs` (`2` replaced by `X`,
/// `4` added) and adds `n.rs`.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(root, "a.rs", "1\n2\n3\n");
    commit(root, "base");
    write(root, "a.rs", "1\nX\n3\n4\n");
    write(root, "n.rs", "new\n");
    commit(root, "change");
    dir
}

/// A repository whose newest commit edits `count` files of 6 lines each,
/// `f000.rs` on, changing each one's first line.
fn many(count: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    let name = |ix: usize| format!("f{ix:03}.rs");
    for ix in 0..count {
        write(root, &name(ix), "a\nb\nc\nd\ne\nf\n");
    }
    commit(root, "base");
    for ix in 0..count {
        write(root, &name(ix), "A\nb\nc\nd\ne\nf\n");
    }
    commit(root, "change");
    dir
}

fn repo(root: &Path) -> Arc<Repo> {
    Arc::new(Repo::discover(root).unwrap().unwrap())
}

/// The model of HEAD's diff in `root`, its viewport `rows` from the
/// start, nothing landed yet.
fn diff_of(
    cx: &mut TestAppContext,
    root: &Path,
    rows: std::ops::Range<usize>,
) -> Entity<CommitDiff> {
    let repo = repo(root);
    let head = repo.log(LogScope::Head, 0, 1).unwrap().remove(0);
    let diff = cx.new(|cx| CommitDiff::new(repo, head, cx));
    diff.update(cx, |diff, cx| diff.set_viewport(rows, cx));
    diff
}

/// Each row of the layout: `#` a header, `~` a section loading, `!` an
/// error, then a line's ` `, `-`, `+` or `=` (folded), then its text.
fn layout(diff: &Entity<CommitDiff>, cx: &mut TestAppContext) -> Vec<String> {
    diff.read_with(cx, |diff, _| {
        let rows = diff.rows().iter().enumerate();
        rows.map(|(ix, row)| {
            let mark = match *row {
                Row::Header(_) => "#",
                Row::Loading(_) => "~",
                Row::Failed(_) => "!",
                Row::Line(section, line) => match &diff.sections()[section].texts {
                    Texts::Read(diffed) => match diffed.lines[line] {
                        Line::Same { .. } => " ",
                        Line::Removed(_) => "-",
                        Line::Added(_) => "+",
                        Line::Hidden { .. } => "=",
                    },
                    _ => "?",
                },
            };
            format!("{mark} {}", diff.row_text(ix).unwrap())
        })
        .collect()
    })
}

/// Each section's two texts, once read.
fn texts(diff: &Entity<CommitDiff>, cx: &mut TestAppContext) -> Vec<Option<(String, String)>> {
    diff.read_with(cx, |diff, _| {
        let texts = diff.sections().iter().map(|section| match &section.texts {
            Texts::Read(diffed) => Some((
                diffed.left.text().to_owned(),
                diffed.right.text().to_owned(),
            )),
            _ => None,
        });
        texts.collect()
    })
}

#[gpui::test]
fn sections(cx: &mut TestAppContext) {
    let dir = fixture();
    let diff = diff_of(cx, dir.path(), 0..50);
    assert_eq!(
        diff.read_with(cx, |diff, _| diff
            .message()
            .map(|message| message.to_string())),
        Some(LOADING.to_owned())
    );
    cx.run_until_parked();
    let summary = diff.read_with(cx, |diff, _| {
        let sections = diff.sections().iter();
        let summary = sections.map(|section| {
            let name = section.path.file_name().unwrap().to_string_lossy();
            (name.into_owned(), section.kind, section.counts)
        });
        summary.collect::<Vec<_>>()
    });
    assert_eq!(
        summary,
        [
            ("a.rs".to_owned(), ChangeKind::Modified, Some((2, 1))),
            ("n.rs".to_owned(), ChangeKind::Added, Some((1, 0))),
        ]
    );
    assert_eq!(
        texts(&diff, cx),
        [
            Some(("1\n2\n3\n".to_owned(), "1\nX\n3\n4\n".to_owned())),
            Some((String::new(), "new\n".to_owned())),
        ]
    );
    assert_eq!(
        layout(&diff, cx),
        [
            "# a.rs", "  1", "- 2", "+ X", "  3", "+ 4", "# n.rs", "+ new"
        ]
    );

    // A commit deleting n.rs: its right text is empty.
    fs::remove_file(dir.path().join("n.rs")).unwrap();
    commit(dir.path(), "delete");
    let head = repo(dir.path())
        .log(LogScope::Head, 0, 1)
        .unwrap()
        .remove(0);
    diff.update(cx, |diff, cx| diff.show(head, cx));
    cx.run_until_parked();
    assert_eq!(
        texts(&diff, cx),
        [Some(("new\n".to_owned(), String::new()))]
    );
    assert_eq!(layout(&diff, cx), ["# n.rs", "- new"]);
}

#[gpui::test]
fn folds(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    let text: String = (1..=20).map(|row| format!("{row}\n")).collect();
    write(root, "a.rs", &text);
    commit(root, "base");
    write(root, "a.rs", text.replace("\n10\n", "\nten\n"));
    commit(root, "change");
    let diff = diff_of(cx, root, 0..50);
    cx.run_until_parked();
    // Three unchanged rows kept on either side of the hunk, the rest folded.
    assert_eq!(
        layout(&diff, cx),
        [
            "# a.rs",
            "= 6 hidden lines",
            "  7",
            "  8",
            "  9",
            "- 10",
            "+ ten",
            "  11",
            "  12",
            "  13",
            "= 7 hidden lines",
        ]
    );
}

#[gpui::test]
fn lazy(cx: &mut TestAppContext) {
    // Before anything is read each section is a header and a loading row,
    // so section 30's rows are 60 and 61.
    let dir = many(60);
    let diff = diff_of(cx, dir.path(), 60..62);
    cx.run_until_parked();
    let started = diff.read_with(cx, |diff, _| diff.started().to_owned());
    // Nearest the viewport first: the section in it, then those a row away,
    // then three rows away.
    assert_eq!(started[..4], [30, 29, 31, 28]);
    // Only those near it are read; the rest still show they're loading.
    assert!(started.len() < 60, "{started:?}");
    let rows = layout(&diff, cx);
    assert_eq!(rows[..2], ["# f000.rs".to_owned(), format!("~ {LOADING}")]);
    assert_eq!(rows.last().unwrap(), &format!("~ {LOADING}"));
    let read = texts(&diff, cx);
    assert!(read[30].is_some() && read[0].is_none() && read[59].is_none());

    // Scrolled to the end: its reads start, and land on the background
    // executor, showing loading until they do.
    let end = diff.read_with(cx, |diff, _| diff.rows().len());
    diff.update(cx, |diff, cx| diff.set_viewport(end - 2..end, cx));
    assert!(texts(&diff, cx)[59].is_none());
    assert_eq!(layout(&diff, cx).last().unwrap(), &format!("~ {LOADING}"));
    cx.run_until_parked();
    let rows = layout(&diff, cx);
    assert_eq!(
        rows[rows.len() - 7..],
        [
            "# f059.rs",
            "- a",
            "+ A",
            "  b",
            "  c",
            "  d",
            "= 2 hidden lines"
        ]
    );
}

#[gpui::test]
fn failed_read(cx: &mut TestAppContext) {
    // a.rs's new blob is gone from the object store.
    let dir = fixture();
    let blob = git(dir.path(), &["rev-parse", "HEAD:a.rs"]);
    let blob = blob.trim();
    let object = dir
        .path()
        .join(".git/objects")
        .join(&blob[..2])
        .join(&blob[2..]);
    fs::remove_file(object).unwrap();
    let diff = diff_of(cx, dir.path(), 0..50);
    cx.run_until_parked();
    let rows = layout(&diff, cx);
    assert_eq!(rows[0], "# a.rs");
    assert!(rows[1].starts_with("! "), "{rows:?}");
    assert_eq!(rows[2..], ["# n.rs", "+ new"]);
}

#[gpui::test]
fn collapse(cx: &mut TestAppContext) {
    let dir = fixture();
    let diff = diff_of(cx, dir.path(), 0..50);
    cx.run_until_parked();
    let full = layout(&diff, cx);

    diff.update(cx, |diff, cx| diff.toggle(0, cx));
    assert_eq!(layout(&diff, cx), ["# a.rs", "# n.rs", "+ new"]);
    diff.update(cx, |diff, cx| diff.toggle(0, cx));
    assert_eq!(layout(&diff, cx), full);

    diff.update(cx, |diff, cx| diff.collapse_all(cx));
    assert_eq!(layout(&diff, cx), ["# a.rs", "# n.rs"]);
    diff.update(cx, |diff, cx| diff.expand_all(cx));
    assert_eq!(layout(&diff, cx), full);

    // Kept through reading the same commit again.
    diff.update(cx, |diff, cx| diff.set_collapsed(1, true, cx));
    diff.update(cx, |diff, cx| diff.reload(cx));
    cx.run_until_parked();
    assert_eq!(
        layout(&diff, cx),
        [&full[..6], &["# n.rs".to_owned()]].concat()
    );
    let head = diff.read_with(cx, |diff, _| diff.commit().clone());
    diff.update(cx, |diff, cx| diff.show(head, cx));
    cx.run_until_parked();
    assert!(diff.read_with(cx, |diff, _| diff.is_collapsed(1) && !diff.is_collapsed(0)));
}

#[gpui::test]
fn big(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(root, "bin.dat", b"\0one\0");
    write(root, "big.txt", "small\n");
    commit(root, "base");
    write(root, "bin.dat", b"\0two\0");
    write(root, "big.txt", "x\n".repeat(MAX_DIFF_BYTES / 2 + 1));
    commit(root, "change");
    let diff = diff_of(cx, root, 0..50);
    cx.run_until_parked();
    assert_eq!(
        layout(&diff, cx),
        [
            format!("# big.txt · {}", NotShown::TooLarge.text()),
            format!("# bin.dat · {}", NotShown::Binary.text()),
        ]
    );

    // 200 sections lay out, only those near the viewport read.
    let dir = many(200);
    let diff = diff_of(cx, dir.path(), 0..20);
    cx.run_until_parked();
    let (headers, started) = diff.read_with(cx, |diff, _| {
        let headers = diff
            .rows()
            .iter()
            .filter(|row| matches!(row, Row::Header(_)));
        (headers.count(), diff.started().len())
    });
    assert_eq!(headers, 200);
    assert!(started < 20, "{started} of 200 sections read");
}
