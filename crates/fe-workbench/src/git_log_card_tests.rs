//! The card hovering a Git Log commit row shows: an initials avatar, the
//! author and dates, the whole message, its trailers, the refs, the files
//! changed, a copy button and a web link; and the rows, which no longer
//! carry the author and age. On repositories built with the `git` CLI in
//! temporary directories.

use std::{path::Path, time::Duration};

use fe_git::Repo;
use fe_ui::{Icon, ROW_HEIGHT};
use gpui::{Entity, Hsla, Modifiers, Pixels, Point, TestAppContext, VisualTestContext, point, px};

use super::{GitLogPanel, dispatch, draw, git, git_command, panel, rows, window};
use crate::Workbench;

/// Commits everything in `root` as `message` by `author` (`Name <email>`),
/// made `seconds` after the epoch at +02:00. Its id.
fn commit_by(root: &Path, author: &str, message: &str, seconds: i64) -> String {
    git(root, &["add", "-A"]);
    let date = format!("@{seconds} +0200");
    let output = git_command(root, &["commit", "-q", "--author", author, "-m", message])
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    git(root, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// A window on `root` with a.rs open and the Git Log panel shown, and the
/// panel.
fn log_window(
    cx: &mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, Entity<GitLogPanel>, VisualTestContext) {
    let (workbench, mut vcx) = window(cx, root, "a.rs");
    dispatch("git.log", &mut vcx);
    let panel = panel(&workbench, &mut vcx);
    (workbench, panel, vcx)
}

/// The middle of row `ix` of the panel's list, as last drawn, scrolled to
/// the top.
fn row_center(panel: &Entity<GitLogPanel>, ix: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    let list = panel.read_with(cx, |panel, _| panel.scroll.0.borrow().base_handle.bounds());
    point(
        list.left() + px(40.),
        list.top() + ROW_HEIGHT * (ix as f32 + 0.5),
    )
}

/// Moves the mouse off the list and its card, so the card hides.
fn leave(cx: &mut VisualTestContext) {
    let window = cx.update(|window, _| window.viewport_size());
    let away = point(px(1.), window.height - px(1.));
    cx.simulate_mouse_move(away, None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(1000));
    draw(cx);
}

/// Hovers row `ix` until its card shows and what it reads has landed.
fn hover(panel: &Entity<GitLogPanel>, ix: usize, cx: &mut VisualTestContext) {
    leave(cx);
    let before = built(panel, cx);
    let row = row_center(panel, ix, cx);
    cx.simulate_mouse_move(row, None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(500));
    draw(cx);
    draw(cx);
    assert_eq!(built(panel, cx), before + 1, "a card was built");
}

/// How many cards were built.
fn built(panel: &Entity<GitLogPanel>, cx: &mut VisualTestContext) -> usize {
    panel.read_with(cx, |panel, _| panel.card_probe().borrow().built)
}

/// `id`'s author date as `git log` prints it in `git.blame_line`'s
/// popover's format: `2023-11-14 22:13 +0200`.
fn cli_date(root: &Path, id: &str) -> String {
    let format = "--date=format:%Y-%m-%d %H:%M %z";
    let date = git(root, &["log", "-1", "--format=%ad", format, id]);
    date.trim().to_owned()
}

/// The editor palette's added and deleted colours, which a file row's
/// counts take.
fn added_and_deleted(cx: &mut VisualTestContext) -> (Hsla, Hsla) {
    cx.update(|_, cx| {
        let palette = fe_gpui::editor_palette(cx);
        let hsla = |color: fe_config::theme::Color| gpui::rgba(color.to_rgba_u32()).into();
        (
            hsla(palette.editor.change_added),
            hsla(palette.editor.error),
        )
    })
}

#[gpui::test]
fn contents(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    std::fs::write(root.join("a.rs"), "one\n").unwrap();
    let first = commit_by(root, "t <t@t>", "one line", 1_600_000_000);
    std::fs::write(root.join("a.rs"), "uno\ntwo\nthree\n").unwrap();
    std::fs::write(root.join("b.rs"), "b\n").unwrap();
    let message = "subject\n\nline one\nline two\n\npara\n";
    let author = "Ada King Lovelace <Ada@Example.com>";
    let second = commit_by(root, author, message, 1_700_000_000);
    let (_workbench, panel, mut cx) = log_window(cx, root);
    let cx = &mut cx;
    let probe = panel.read_with(cx, |panel, _| panel.card_probe());
    let now = panel.read_with(cx, |panel, _| panel.now);
    let (added, deleted) = added_and_deleted(cx);

    // Not before the delay.
    let row = row_center(&panel, 0, cx);
    cx.simulate_mouse_move(row, None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(250));
    draw(cx);
    assert_eq!(built(&panel, cx), 0, "not shown before the delay");

    // The author's initials in a circle coloured from their email; the
    // author; how long ago and the date; the whole message; and the
    // files changed with the lines added green and removed red.
    cx.executor().advance_clock(Duration::from_millis(100));
    draw(cx);
    draw(cx);
    assert_eq!(built(&panel, cx), 1);
    let ago = super::super::commit_rows::ago(now - 1_700_000_000);
    {
        let card = probe.borrow();
        assert_eq!(card.initials, "AL", "the first and last names' letters");
        assert_eq!(card.author, "Ada King Lovelace");
        assert!(ago.ends_with("years ago"), "{ago}");
        let dates = format!("{ago} ({})", cli_date(root, &second));
        assert_eq!(card.dates, dates);
        assert_eq!(card.message, "subject\nline one\nline two\n\npara");
        let totals = [
            ("2 files changed", None),
            ("4 insertions(+)", Some(added)),
            ("1 deletion(-)", Some(deleted)),
        ]
        .map(|(text, color)| (text.to_owned(), color));
        assert_eq!(card.totals, totals);
        let parts: Vec<_> = card.drawn.iter().map(|(name, _)| *name).collect();
        assert_eq!(parts, ["header", "scrolled", "totals", "footer"]);
        let scrolled: Vec<_> = card.scrolled.iter().map(|(name, _)| *name).collect();
        assert_eq!(scrolled, ["summary", "body", "refs"]);
    }
    let ada = probe.borrow().avatar.expect("drawn");

    // One file, one line: singular. A one-word name: its letter alone,
    // and another colour for another email.
    hover(&panel, 1, cx);
    {
        let card = probe.borrow();
        assert_eq!(card.initials, "T");
        assert_eq!(card.message, "one line");
        let totals = [
            ("1 file changed", None),
            ("1 insertion(+)", Some(added)),
            ("0 deletions(-)", Some(deleted)),
        ]
        .map(|(text, color)| (text.to_owned(), color));
        assert_eq!(card.totals, totals);
        assert_ne!(card.avatar, Some(ada), "another email, another colour");
        let dates = format!(
            "{} ({})",
            super::super::commit_rows::ago(now - 1_600_000_000),
            cli_date(root, &first)
        );
        assert_eq!(card.dates, dates);
    }

    // The colour is the email's, whatever its case, every time it's
    // shown, and nothing is fetched for it: the avatar is drawn here.
    hover(&panel, 0, cx);
    assert_eq!(probe.borrow().avatar, Some(ada), "stable");
    std::fs::write(root.join("c.rs"), "c\n").unwrap();
    commit_by(root, "Ada Byron <ada@example.com>", "third", 1_700_000_100);
    dispatch("git.log", cx);
    hover(&panel, 0, cx);
    let card = probe.borrow();
    assert_eq!(card.initials, "AB");
    assert_eq!(card.avatar, Some(ada), "the same email, the same colour");
}

#[gpui::test]
fn trailers_and_refs(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    std::fs::write(root.join("a.rs"), "one\n").unwrap();
    let message = "fix it\n\nbody text\n\n\
        Co-Authored-By: Claude Opus <noreply@anthropic.com>\n\
        Signed-off-by: Ada Lovelace <ada@example.com>\n";
    let id = commit_by(root, "t <t@t>", message, 1_700_000_000);
    git(root, &["tag", "v1"]);
    let (workbench, panel, mut cx) = log_window(cx, root);
    let cx = &mut cx;
    let probe = panel.read_with(cx, |panel, _| panel.card_probe());
    hover(&panel, 0, cx);
    let labels = rows(&workbench, cx)[0].0.clone();
    assert_eq!(labels, ["main", "tag v1"]);
    let card = probe.borrow();
    // The trailers are rows naming who they credit, out of the message.
    assert_eq!(card.message, "fix it\nbody text");
    let trailers = [
        ("Co-Authored-By", "Claude Opus"),
        ("Signed-off-by", "Ada Lovelace"),
    ]
    .map(|(key, name)| (key.to_owned(), name.to_owned()));
    assert_eq!(card.trailers, trailers);
    // The commit's ref labels, as its row shows them, and its short id.
    assert_eq!(card.refs, labels);
    assert_eq!(card.short, id[..7]);
    let parts: Vec<_> = card.drawn.iter().map(|(name, _)| *name).collect();
    assert_eq!(parts, ["header", "scrolled", "totals", "footer"]);
    let scrolled: Vec<_> = card.scrolled.iter().map(|(name, _)| *name).collect();
    assert_eq!(scrolled, ["summary", "body", "trailers", "refs"]);
}

#[gpui::test]
fn copy(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    std::fs::write(root.join("a.rs"), "one\n").unwrap();
    let id = commit_by(root, "t <t@t>", "one", 1_700_000_000);
    let (_workbench, panel, mut cx) = log_window(cx, root);
    let cx = &mut cx;
    let probe = panel.read_with(cx, |panel, _| panel.card_probe());
    hover(&panel, 0, cx);
    assert_eq!(probe.borrow().copy_icon, Some(Icon::Copy));
    let button = cx
        .debug_bounds("icon-button:commit-card-copy")
        .expect("the copy button is drawn");

    // Into the card, which stays, and a click on the button: the full id
    // is on the clipboard and the button shows a check.
    cx.simulate_mouse_move(button.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(500));
    draw(cx);
    cx.simulate_click(button.center(), Modifiers::none());
    draw(cx);
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some(id.as_str()));
    assert_eq!(id.len(), 40, "the full id");
    assert_eq!(probe.borrow().copy_icon, Some(Icon::Check));
    assert_eq!(built(&panel, cx), 1, "the same card");

    // For a moment.
    cx.executor().advance_clock(Duration::from_millis(1000));
    draw(cx);
    assert_eq!(probe.borrow().copy_icon, Some(Icon::Check));
    cx.executor().advance_clock(Duration::from_millis(600));
    draw(cx);
    assert_eq!(probe.borrow().copy_icon, Some(Icon::Copy));
}

#[gpui::test]
fn web_link(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    std::fs::write(root.join("a.rs"), "one\n").unwrap();
    let id = commit_by(root, "t <t@t>", "one", 1_700_000_000);
    git(
        root,
        &["remote", "add", "origin", "git@github.com:owner/repo.git"],
    );
    let (_workbench, panel, mut cx) = log_window(cx, root);
    let cx = &mut cx;
    let probe = panel.read_with(cx, |panel, _| panel.card_probe());
    let web_url = || {
        let repo = Repo::discover(root).unwrap().unwrap();
        repo.commit_web_url(&id)
    };

    // On github.com: a link to the commit's page, which a click opens.
    hover(&panel, 0, cx);
    let url = web_url().expect("github.com has commit pages");
    assert_eq!(url, format!("https://github.com/owner/repo/commit/{id}"));
    let link = Some(("Open on GitHub".to_owned(), url.clone()));
    assert_eq!(probe.borrow().link, link);
    let at = cx
        .debug_bounds("commit-card-web")
        .expect("the link is drawn");
    cx.simulate_mouse_move(at.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(500));
    draw(cx);
    cx.simulate_click(at.center(), Modifiers::none());
    draw(cx);
    assert_eq!(cx.opened_url(), Some(url));

    // Another host, or no remote: no link.
    let gitlab = "https://gitlab.com/owner/repo.git";
    git(root, &["remote", "set-url", "origin", gitlab]);
    hover(&panel, 0, cx);
    assert_eq!(web_url(), None);
    assert_eq!(probe.borrow().link, None);
    assert_eq!(cx.debug_bounds("commit-card-web"), None);
    git(root, &["remote", "remove", "origin"]);
    hover(&panel, 0, cx);
    assert_eq!(probe.borrow().link, None);
    assert_eq!(cx.debug_bounds("commit-card-web"), None);
    let parts: Vec<_> = probe.borrow().drawn.iter().map(|(name, _)| *name).collect();
    assert_eq!(parts.last(), Some(&"footer"), "the id and copy still show");
}

#[gpui::test]
fn rows_lose_metadata(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    std::fs::write(root.join("a.rs"), "one\n").unwrap();
    let first = commit_by(root, "Ada Lovelace <ada@x>", "one line", 1_700_000_000);
    std::fs::write(root.join("a.rs"), "two\n").unwrap();
    let second = commit_by(root, "Ada Lovelace <ada@x>", "subject", 1_700_000_000);
    let (workbench, mut cx) = window(cx, root, "a.rs");
    let cx = &mut cx;
    let summaries = [
        format!("{}  subject", &second[..7]),
        format!("{}  one line", &first[..7]),
    ];

    // The History panel's rows keep the author and age.
    dispatch("git.file_history", cx);
    let history = workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        let history = dock.history().cloned().expect("the History panel was made");
        history.read(cx).rows()
    });
    assert_eq!(history.len(), 2);
    for (row, summary) in history.iter().zip(&summaries) {
        assert_eq!(row[0], *summary);
        assert!(row[1].starts_with("Ada Lovelace, "), "{row:?}");
    }

    // The Git Log's are the graph, the ref labels and the summary alone.
    dispatch("git.log", cx);
    let [newest, oldest] = summaries;
    let log = vec![
        (vec!["main".to_owned()], vec![newest.clone()]),
        (Vec::new(), vec![oldest.clone()]),
    ];
    assert_eq!(rows(&workbench, cx), log);
    let panel = panel(&workbench, cx);
    let seen = panel.read_with(cx, |panel, _| panel.rows_seen());
    seen.borrow_mut().texts.clear();
    draw(cx);
    let drawn = seen.borrow().texts.clone();
    let drawn: Vec<_> = drawn.into_values().collect();
    assert_eq!(drawn, [vec![newest], vec![oldest]], "as drawn");
    assert!(
        seen.borrow().segments.contains_key(&0),
        "the graph is drawn"
    );
}

#[gpui::test]
fn crowded_card_keeps_its_footer(cx: &mut TestAppContext) {
    // A long message, eighty co-authors and eighty tags: more than the
    // window holds.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    std::fs::write(root.join("a.rs"), "one\n").unwrap();
    let body: Vec<String> = (1..=100).map(|line| format!("line {line}")).collect();
    let trailers: Vec<String> = (1..=80)
        .map(|n| format!("Co-authored-by: Person {n} <p{n}@x>"))
        .collect();
    let message = format!(
        "crowded\n\n{}\n\n{}\n",
        body.join("\n"),
        trailers.join("\n")
    );
    commit_by(root, "t <t@t>", &message, 1_700_000_000);
    for tag in 0..80 {
        git(root, &["tag", &format!("release-{tag:03}")]);
    }
    let (_workbench, panel, mut cx) = log_window(cx, root);
    let cx = &mut cx;
    let probe = panel.read_with(cx, |panel, _| panel.card_probe());
    hover(&panel, 0, cx);
    assert_eq!(probe.borrow().trailers.len(), 80);
    assert_eq!(probe.borrow().refs.len(), 81, "main and the tags");

    // The header, totals and footer are whole and in the window; what's
    // between them scrolls.
    let window = cx.update(|window, _| window.viewport_size());
    let parts = probe.borrow().drawn.clone();
    let names: Vec<_> = parts.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["header", "scrolled", "totals", "footer"]);
    for (name, part) in &parts {
        assert!(
            part.top() >= px(0.) && part.bottom() <= window.height,
            "{name}: {part:?} in {window:?}"
        );
    }
    let button = cx
        .debug_bounds("icon-button:commit-card-copy")
        .expect("the copy button is drawn");
    assert!(button.bottom() <= window.height, "{button:?}");
    let scrolled = parts[1].1;
    assert!(scrolled.size.height > px(40.), "the message keeps room");
    let scroll = probe.borrow().scroll.clone().unwrap();
    assert!(scroll.max_offset().y > px(1000.));

    // Scrolled to its end, past the trailers, the refs come into view.
    scroll.set_offset(point(px(0.), -scroll.max_offset().y));
    draw(cx);
    let sections = probe.borrow().scrolled.clone();
    let (_, refs) = sections.iter().find(|(name, _)| *name == "refs").unwrap();
    assert!(
        refs.bottom() <= scrolled.bottom() + px(1.) && refs.bottom() > scrolled.top(),
        "{refs:?} in {scrolled:?}"
    );
    let (_, trailers) = (sections.iter())
        .find(|(name, _)| *name == "trailers")
        .unwrap();
    assert!(
        trailers.bottom() <= refs.top() && trailers.top() < scrolled.top(),
        "the trailers scrolled past, above the refs: {trailers:?}"
    );
    let button = cx.debug_bounds("icon-button:commit-card-copy").unwrap();
    assert!(button.bottom() <= window.height, "the footer stays put");
}
