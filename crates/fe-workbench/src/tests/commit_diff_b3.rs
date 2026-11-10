//! The commit diff in B3 (`docs/design/design-system.md`, Review diff): each
//! hunk under a header naming the symbols around it from the file's
//! outline and its count of changes, and a `−` or `+` before each changed
//! row, on a repository built with the `git` CLI in a temporary directory.

use std::{fs, path::Path, process::Command, sync::Arc};

use fe_git::{LogScope, Repo};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext, WeakEntity};

use crate::workbench::git_commands::git_log::{
    commit_diff::CommitDiff,
    commit_diff_view::{CommitDiffView, Drawn},
};

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
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
}

/// The file before: a method of an impl, and a function after it.
const BEFORE: &str = "struct MinimapTile;

impl MinimapTile {
    fn render(&self) -> u32 {
        let a = 1;
        let b = 2;
        a + b
    }
}

fn other() {}
";

/// [`BEFORE`] with two rows of `render` changed, `other` gone, and a
/// comment added at the end, in no symbol.
const AFTER: &str = "struct MinimapTile;

impl MinimapTile {
    fn render(&self) -> u32 {
        let a = 10;
        let b = 20;
        a + b
    }
}

// The end.
";

/// A repository whose newest commit changes `tile.rs` from [`BEFORE`] to
/// [`AFTER`].
fn fixture() -> tempfile::TempDir {
    fixture_of(BEFORE, AFTER)
}

/// A repository whose newest commit changes `tile.rs` from `before` to
/// `after`.
fn fixture_of(before: &str, after: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    fs::write(root.join("tile.rs"), before).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    fs::write(root.join("tile.rs"), after).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "change"]);
    dir
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// HEAD's change in `root`, in a window, its texts read and drawn.
fn view<'a>(
    root: &Path,
    cx: &'a mut TestAppContext,
) -> (Entity<CommitDiffView>, &'a mut VisualTestContext) {
    cx.update(fe_gpui::init);
    let repo = Arc::new(Repo::discover(root).unwrap().unwrap());
    let head = repo.log(LogScope::Head, 0, 1).unwrap().remove(0);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let diff = cx.new(|cx| CommitDiff::new(repo, head, cx));
        CommitDiffView::new(WeakEntity::new_invalid(), diff, cx)
    });
    for _ in 0..4 {
        draw(cx);
    }
    (view, cx)
}

/// The rows drawn last, by index.
fn drawn(view: &Entity<CommitDiffView>, cx: &mut VisualTestContext) -> Vec<(usize, Drawn)> {
    let seen = view.read_with(cx, |view, _| view.seen());
    let seen = seen.borrow();
    (seen.rows.iter())
        .map(|(ix, row)| (*ix, row.clone()))
        .collect()
}

#[gpui::test]
fn hunk_headers_name_their_symbol_and_changes(cx: &mut TestAppContext) {
    let dir = fixture();
    let (view, cx) = view(dir.path(), cx);
    let rows = drawn(&view, cx);
    let headers: Vec<_> = (rows.iter())
        .filter_map(|(ix, row)| match row {
            Drawn::Hunk(text) => Some((*ix, text.as_str())),
            _ => None,
        })
        .collect();
    // Two rows of `render`, each removed and added: in the impl's method.
    // Then `other` removed and the comment added: the comment is in no
    // symbol, so the hunk takes the one it removes rows from.
    assert_eq!(
        headers.iter().map(|(_, text)| *text).collect::<Vec<_>>(),
        ["impl MinimapTile › render · 4 changes", "other · 2 changes"],
        "{rows:?}"
    );
    // Each header right above its hunk's first row, a changed pair, a `−`
    // before the left's text and a `+` before the right's.
    for (ix, _) in &headers {
        let below = rows.iter().find(|(at, _)| *at == ix + 1);
        let Some((_, Drawn::Pair([Some(left), Some(right)]))) = below else {
            panic!("row {} is {below:?}", ix + 1);
        };
        assert_eq!((left.glyph, right.glyph), (Some("−"), Some("+")));
        assert!(left.tint.is_some() && right.tint.is_some());
    }
    // Unchanged rows have no glyph.
    let unchanged = (rows.iter()).find_map(|(_, row)| match row {
        Drawn::Pair([Some(left), Some(right)]) if left.tint.is_none() => Some((left, right)),
        _ => None,
    });
    let (left, right) = unchanged.expect("an unchanged row");
    assert_eq!((left.glyph, right.glyph), (None, None));
}

#[gpui::test]
fn rows_added_or_removed_above_a_symbol_are_not_in_it(cx: &mut TestAppContext) {
    let plain = "fn existing() {\n    1;\n}\n";
    let noted = "// A note.\nfn existing() {\n    1;\n}\n";
    for (before, after) in [(plain, noted), (noted, plain)] {
        let dir = fixture_of(before, after);
        let (view, cx) = view(dir.path(), cx);
        let headers: Vec<_> = (drawn(&view, cx).into_iter())
            .filter_map(|(_, row)| match row {
                Drawn::Hunk(text) => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(headers, ["1 change"], "{before:?} to {after:?}");
    }
}
