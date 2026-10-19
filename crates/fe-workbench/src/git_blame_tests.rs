//! Line blame after the caret row, its popover and its commit's change, on
//! repositories built with the `git` CLI in temporary directories, against
//! what `git blame` and `git log` say.

use std::{fs, path::Path, process::Command, time::Duration};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::decorations::{DecorationKind, Tint};
use fe_gpui::{
    DiffView, EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled,
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{EDIT_REST, REST, counts};
use crate::{
    Launch, Workbench,
    workbench::git_commands::{GitCommand, Routed},
};

#[path = "git_blame_head_race_tests.rs"]
mod head_race;

#[path = "git_blame_column_tests.rs"]
mod column_tests;

const DAY: i64 = 86_400;

/// Runs `git` in `dir`, with the developer's own config shut out, and
/// `env` set: what it printed.
fn git_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .envs(env.iter().copied())
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    git_env(dir, args, &[])
}

fn now() -> i64 {
    super::now()
}

/// Commits everything in `dir` as `author`, `days` days (and an hour) ago,
/// two hours east of UTC: the commit's id.
fn commit(dir: &Path, message: &str, author: &str, days: i64) -> String {
    let date = format!("@{} +0200", now() - days * DAY - 3600);
    let env = [
        ("GIT_AUTHOR_DATE", date.as_str()),
        ("GIT_COMMITTER_DATE", date.as_str()),
    ];
    let author = format!("{author} <{}@example.com>", author.to_lowercase());
    git(dir, &["add", "."]);
    let args = ["commit", "-q", "--author", &author, "-m", message];
    git_env(dir, &args, &env);
    git(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// `a.rs`'s rows at the first commit.
fn first() -> String {
    (0..14).map(|row| format!("line {row}\n")).collect()
}

/// `a.rs`'s rows at the second commit: row 2 rewritten.
fn second() -> String {
    first().replace("line 2\n", "THREE\n")
}

/// A repository whose `a.rs` Ann committed ten days ago, as its root
/// commit, and Bob changed row 2 of three days ago: the two commits' ids.
fn fixture() -> (tempfile::TempDir, [String; 2]) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::write(root.join("a.rs"), first()).unwrap();
    let c1 = commit(root, "Add a.rs\n\nWith fourteen rows.", "Ann", 10);
    fs::write(root.join("a.rs"), second()).unwrap();
    let c2 = commit(
        root,
        "Shout row two\n\nIt was too quiet.\nNow it isn't.",
        "Bob",
        3,
    );
    (dir, [c1, c2])
}

/// What `git blame` says of `row` of `path`: the summary, the author and
/// the author time.
fn git_blame(dir: &Path, path: &str, row: u32) -> (String, String, i64) {
    let range = format!("{},{}", row + 1, row + 1);
    let out = git(dir, &["blame", "--porcelain", "-L", &range, path]);
    let field = |name: &str| {
        (out.lines())
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(' '))
            .unwrap_or_else(|| panic!("{name} in {out}"))
            .to_owned()
    };
    let time = field("author-time").parse().unwrap();
    (field("summary"), field("author"), time)
}

/// What the annotation of `row` should say, from `git blame`.
fn expected(dir: &Path, row: u32) -> String {
    let (summary, author, time) = git_blame(dir, "a.rs", row);
    let days = (now() - time) / DAY;
    format!("{summary}, {author} ({days} days ago)")
}

/// A window of a workbench rooted at `root`, focused, with vim on, and
/// `file` open if given.
fn window(
    cx: &mut TestAppContext,
    root: &Path,
    file: Option<&str>,
) -> (Entity<Workbench>, VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let cx = &mut vcx;
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    if let Some(file) = file {
        open(&workbench, &root.join(file), cx);
    }
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    (workbench, vcx)
}

fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Lets `time` pass, then draws.
fn wait(time: Duration, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(time);
    draw(cx);
}

/// Lets the caret rest, and an edit's blame land.
fn settle(cx: &mut VisualTestContext) {
    wait(EDIT_REST.max(REST), cx);
}

/// Runs workbench command `id` as an unbound key would.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    send(id, cx);
    draw(cx);
}

/// Runs workbench command `id`, leaving what it starts in the background
/// to run.
fn send(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
}

/// Puts `editor`'s caret at the start of `row`, without letting anything
/// run.
fn put_caret(editor: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            let at = editor.model().text().row_start(row);
            editor.select_and_reveal(at..at, cx);
        })
    });
}

/// Types `:{line}` and Enter.
fn ex(line: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

/// The active tab's editor.
fn tab_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane().read(cx).active_editor().cloned()).expect("an editor tab")
    })
}

/// The active tab's title.
fn title(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.active_item().expect("a tab").tab_title(cx).to_string()
    })
}

fn tab_count(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tabs().len())
}

/// The active tab's diff view.
fn diff(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<DiffView> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let item = pane.active_item().expect("a tab");
        item.downcast_ref::<DiffView>()
            .cloned()
            .expect("a diff view")
    })
}

fn sides(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> [Entity<EditorView>; 2] {
    diff.read_with(cx, |diff, _| [diff.left().clone(), diff.right().clone()])
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string())
}

fn caret_row(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        model
            .text()
            .offset_to_point(model.primary_selection().head())
            .row
    })
}

/// Shows tab `ix` of the active pane, focused.
fn activate(workbench: &Entity<Workbench>, ix: usize, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(ix, window, cx)));
    draw(cx);
}

/// Each after-line decoration of `editor`: its row and text, all muted.
fn annotations(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(u32, String)> {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let text = model.text();
        (model.decorations(0..text.len()).into_iter())
            .filter_map(|decoration| match decoration.kind {
                DecorationKind::AfterLine(label) => {
                    assert_eq!(decoration.tint, Tint::Muted);
                    let row = text.offset_to_point(decoration.range.start).row;
                    Some((row, label.to_string()))
                }
                _ => None,
            })
            .collect()
    })
}

fn popover(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    editor.read_with(cx, |editor, _| editor.popover_text())
}

fn counted(cx: &mut VisualTestContext) -> super::Counts {
    cx.update(|_, cx| counts(cx))
}

#[gpui::test]
fn annotation(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [], "off by default");

    dispatch("git.toggle_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(0, expected(root, 0))]);
    assert_eq!(expected(root, 0), "Add a.rs, Ann (10 days ago)");

    // `j` moves it, once the caret rests.
    keys("j", cx);
    assert_eq!(annotations(&tab, cx), [], "hidden while the caret moves");
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(1, expected(root, 1))]);

    // So does a search.
    cx.simulate_keystrokes("/");
    cx.simulate_input("THR");
    keys("enter", cx);
    settle(cx);
    assert_eq!(caret_row(&tab, cx), 2);
    assert_eq!(annotations(&tab, cx), [(2, expected(root, 2))]);
    assert_eq!(expected(root, 2), "Shout row two, Bob (3 days ago)");

    // A visual selection over rows hides it; within its row, it shows.
    keys("V j", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), []);
    keys("escape k", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(2, expected(root, 2))]);

    // An edited row isn't committed; the rest keep theirs.
    keys("A ! escape", cx);
    settle(cx);
    assert_eq!(text(&tab, cx), second().replace("THREE", "THREE!"));
    assert_eq!(annotations(&tab, cx), [(2, "Not Committed Yet".to_owned())]);
    keys("k", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(1, expected(root, 1))]);

    // Off, it goes.
    dispatch("git.toggle_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), []);
    keys("j", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), []);
}

#[gpui::test]
fn ex_and_popover(cx: &mut TestAppContext) {
    let (dir, [_, c2]) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);

    ex("Gitsigns toggle_current_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(0, expected(root, 0))]);
    ex("Gitsigns toggle_current_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), []);
    ex("Gitsigns toggle_current_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(0, expected(root, 0))]);
    ex("Gitsigns toggle_current_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), []);

    // The popover at the caret, line blame on or off.
    keys("2 j", cx);
    ex("Gitsigns blame_line", cx);
    let date = git(
        root,
        &[
            "log",
            "-1",
            "--format=%ad",
            "--date=format:%Y-%m-%d %H:%M %z",
            &c2,
        ],
    );
    let short = &c2[..7];
    assert_eq!(
        popover(&tab, cx).as_deref(),
        Some(
            format!(
                "**Bob** bob@example.com\n{} (3 days ago)\n\n\
                 Shout row two\n\nIt was too quiet.\nNow it isn't.\n\n{short}",
                date.trim()
            )
            .as_str()
        )
    );
    assert!(date.trim().ends_with("+0200"), "{date}");

    // It goes as the caret moves.
    keys("j", cx);
    assert_eq!(popover(&tab, cx), None);

    // From the palette, on a row not committed yet.
    keys("O x escape", cx);
    dispatch("git.blame_line", cx);
    assert_eq!(popover(&tab, cx).as_deref(), Some("Not Committed Yet"));
}

#[gpui::test]
fn open_commit(cx: &mut TestAppContext) {
    let (dir, [c1, c2]) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    let (s1, s2) = (&c1[..7], &c2[..7]);

    // Row 2's commit: the file at its parent against the file at it.
    keys("2 j", cx);
    dispatch("git.open_line_commit", cx);
    let by_c2 = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), format!("a.rs ({s2}^ ↔ {s2})"));
    let [left, right] = sides(&by_c2, cx);
    assert_eq!([text(&left, cx), text(&right, cx)], [first(), second()]);
    assert!(left.read_with(cx, |left, _| left.is_read_only()));
    assert!(right.read_with(cx, |right, _| right.is_read_only()));
    assert_eq!(caret_row(&right, cx), 2);

    // The root commit's: nothing against the file at it.
    activate(&workbench, 0, cx);
    keys("k k", cx);
    ex("Gitsigns show_commit", cx);
    let by_c1 = diff(&workbench, cx);
    assert_ne!(by_c1, by_c2);
    assert_eq!(title(&workbench, cx), format!("a.rs ({s1}^ ↔ {s1})"));
    let [left, right] = sides(&by_c1, cx);
    assert_eq!(
        [text(&left, cx), text(&right, cx)],
        [String::new(), first()]
    );
    assert_eq!(caret_row(&right, cx), 0);

    // Asked again: the one open, the caret on the row.
    activate(&workbench, 0, cx);
    keys("2 j", cx);
    dispatch("git.open_line_commit", cx);
    assert_eq!(diff(&workbench, cx), by_c2);
    assert_eq!(tab_count(&workbench, cx), 3);
    activate(&workbench, 0, cx);
    keys("g g", cx);

    // A row not committed yet: HEAD against the working tree.
    activate(&workbench, 0, cx);
    assert_eq!(tab_editor(&workbench, cx), tab);
    keys("A ! escape", cx);
    dispatch("git.open_line_commit", cx);
    assert_eq!(title(&workbench, cx), "a.rs (HEAD ↔ Working Tree)");
    let [left, right] = sides(&diff(&workbench, cx), cx);
    assert_eq!(text(&left, cx), second());
    assert_eq!(text(&right, cx), second().replacen("line 0", "line 0!", 1));
}

#[gpui::test]
fn cheap(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    fs::write(root.join("new.rs"), "untracked\n").unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    fs::write(elsewhere.path().join("x.rs"), "no repo\n").unwrap();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);

    dispatch("git.toggle_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(0, expected(root, 0))]);
    let before = counted(cx);
    assert_eq!((before.blames, before.shown), (1, 1));

    // A still caret: nothing blamed or shown again.
    for _ in 0..10 {
        wait(Duration::from_secs(1), cx);
    }
    assert_eq!(counted(cx), before);

    // Ten moves within the rest: shown once, at the last, and nothing
    // blamed, the buffer the same.
    for _ in 0..10 {
        cx.simulate_keystrokes("j");
        cx.executor().advance_clock(REST / 10);
        cx.run_until_parked();
    }
    draw(cx);
    assert_eq!(counted(cx), before);
    wait(REST, cx);
    assert_eq!(counted(cx).shown, before.shown + 1);
    assert_eq!(counted(cx).blames, before.blames);
    assert_eq!(annotations(&tab, cx), [(10, expected(root, 10))]);

    // Back to a row seen: still nothing blamed.
    keys("g g", cx);
    settle(cx);
    assert_eq!(counted(cx).blames, before.blames);
    assert_eq!(annotations(&tab, cx), [(0, expected(root, 0))]);

    // An untracked file, and one outside a repository, show nothing.
    open(&workbench, &root.join("new.rs"), cx);
    let untracked = tab_editor(&workbench, cx);
    assert_ne!(untracked, tab);
    settle(cx);
    assert_eq!(annotations(&untracked, cx), []);
    assert_eq!(annotations(&tab, cx), [], "only the active editor's");
    open(&workbench, &elsewhere.path().join("x.rs"), cx);
    let outside = tab_editor(&workbench, cx);
    settle(cx);
    assert_eq!(annotations(&outside, cx), []);
    ex("Gitsigns blame_line", cx);
    let message = outside.read_with(cx, |editor, _| {
        editor
            .vim_state()
            .and_then(|vim| vim.message().map(str::to_owned))
    });
    assert_eq!(message.as_deref(), Some("Not in a git repository"));

    // Back on the tracked file, its blame is kept: HEAD is read, not moved.
    let blames = counted(cx).blames;
    activate(&workbench, 0, cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(0, expected(root, 0))]);
    assert_eq!(counted(cx).blames, blames);
}

#[gpui::test]
fn splits_show_one(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    dispatch("git.toggle_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(0, expected(root, 0))]);

    // The working side of a diff splits the tab's model: only the active
    // editor shows it, once.
    dispatch("git.diff_head", cx);
    let [_, right] = sides(&diff(&workbench, cx), cx);
    settle(cx);
    assert_eq!(annotations(&right, cx), [(0, expected(root, 0))]);
    assert_eq!(annotations(&tab, cx), []);
    activate(&workbench, 0, cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(0, expected(root, 0))]);
    assert_eq!(annotations(&right, cx), []);

    // Off, neither shows it.
    dispatch("git.toggle_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), []);
    assert_eq!(annotations(&right, cx), []);
}

#[gpui::test]
fn head_moves(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    keys("2 j", cx);
    dispatch("git.toggle_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(2, expected(root, 2))]);

    // HEAD moved while it was off: blamed again as it's turned on.
    dispatch("git.toggle_line_blame", cx);
    let date = format!("@{} +0200", now() - 3 * DAY - 3600);
    let env = [("GIT_COMMITTER_DATE", date.as_str())];
    git_env(
        root,
        &["commit", "-q", "--amend", "-m", "Shout louder"],
        &env,
    );
    dispatch("git.toggle_line_blame", cx);
    settle(cx);
    assert_eq!(expected(root, 2), "Shout louder, Bob (3 days ago)");
    assert_eq!(annotations(&tab, cx), [(2, expected(root, 2))]);

    // Untracked by a commit, the caret still: it goes.
    git(root, &["rm", "-q", "--cached", "a.rs"]);
    git(root, &["commit", "-q", "-m", "Forget a.rs"]);
    dispatch("git.refresh", cx);
    settle(cx);
    assert_eq!(caret_row(&tab, cx), 2);
    assert_eq!(annotations(&tab, cx), []);
}

#[gpui::test]
fn renamed_and_shifted(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let old: String = std::iter::once("dup\n".to_owned())
        .chain((1..10).map(|row| format!("x{row}\n")))
        .chain(std::iter::once("dup\n".to_owned()))
        .collect();
    fs::write(root.join("old.rs"), &old).unwrap();
    let c1 = commit(root, "Add old.rs", "Ann", 10);
    git(root, &["mv", "old.rs", "a.rs"]);
    commit(root, "Rename it", "Bob", 5);
    let top: String = (0..10).map(|row| format!("new {row}\n")).collect();
    fs::write(root.join("a.rs"), format!("{top}{old}")).unwrap();
    commit(root, "Ten rows on top", "Bob", 3);
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let s1 = &c1[..7];

    // The first `dup`, ten rows down now, from before the rename.
    keys("1 1 G", cx);
    let tab = tab_editor(&workbench, cx);
    assert_eq!(caret_row(&tab, cx), 10);
    dispatch("git.open_line_commit", cx);
    let toasts = workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.toasts().read(cx);
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect::<Vec<_>>()
    });
    eprintln!("toasts {toasts:?} title {}", title(&workbench, cx));
    let by_c1 = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), format!("a.rs ({s1}^ ↔ {s1})"));
    let [left, right] = sides(&by_c1, cx);
    assert_eq!(
        [text(&left, cx), text(&right, cx)],
        [String::new(), old.clone()]
    );
    assert_eq!(caret_row(&right, cx), 0);

    // The second, in the tab already open.
    activate(&workbench, 0, cx);
    keys("2 1 G", cx);
    dispatch("git.open_line_commit", cx);
    assert_eq!(diff(&workbench, cx), by_c1);
    assert_eq!(caret_row(&right, cx), 10);
}

#[gpui::test]
fn superseded_asks(cx: &mut TestAppContext) {
    let (dir, [c1, _]) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    let s1 = &c1[..7];

    // A popover for a row the caret has left: none.
    put_caret(&tab, 2, cx);
    send("git.blame_line", cx);
    put_caret(&tab, 3, cx);
    draw(cx);
    assert_eq!(popover(&tab, cx), None);

    // Two commits asked for: only the last opens.
    put_caret(&tab, 2, cx);
    send("git.open_line_commit", cx);
    put_caret(&tab, 0, cx);
    send("git.open_line_commit", cx);
    draw(cx);
    assert_eq!(tab_count(&workbench, cx), 2);
    assert_eq!(title(&workbench, cx), format!("a.rs ({s1}^ ↔ {s1})"));
}

/// The right side's caret row of the active diff reads as `row` of `tab`
/// does: the commit's row the buffer's row came from.
fn same_row_text(
    workbench: &Entity<Workbench>,
    tab: &Entity<EditorView>,
    row: u32,
    cx: &mut VisualTestContext,
) {
    let [_, right] = sides(&diff(workbench, cx), cx);
    let at = caret_row(&right, cx);
    let shown = text(&right, cx).lines().nth(at as usize).map(str::to_owned);
    let blamed = text(tab, cx).lines().nth(row as usize).map(str::to_owned);
    assert_eq!(shown, blamed);
}

#[gpui::test]
fn duplicate_rows_traced(cx: &mut TestAppContext) {
    // The second `dup` of C survives D's change to the first and E's
    // deletions, as `git blame` traces it.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::write(root.join("a.rs"), "start\ndup\nx\ndup\nend\n").unwrap();
    let c = commit(root, "C", "Ann", 10);
    fs::write(root.join("a.rs"), "start\ngone\nx\ndup\nend\n").unwrap();
    commit(root, "D", "Bob", 5);
    fs::write(root.join("a.rs"), "start\ndup\nend\n").unwrap();
    commit(root, "E", "Bob", 3);
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    let short = &c[..7];

    keys("j", cx);
    dispatch("git.open_line_commit", cx);
    let by_c = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), format!("a.rs ({short}^ ↔ {short})"));
    let [_, right] = sides(&by_c, cx);
    assert_eq!(caret_row(&right, cx), 3);
    same_row_text(&workbench, &tab, 1, cx);

    // Again, in the tab open, from row 0 first.
    put_caret(&right, 0, cx);
    activate(&workbench, 0, cx);
    dispatch("git.open_line_commit", cx);
    assert_eq!(diff(&workbench, cx), by_c);
    assert_eq!(caret_row(&right, cx), 3);
}

#[gpui::test]
fn renamed_over_another_file(cx: &mut TestAppContext) {
    // C has `old.rs` and an unrelated `a.rs`; D moves `old.rs` over `a.rs`.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let old: String = (0..12).map(|row| format!("old row {row}\n")).collect();
    fs::write(root.join("old.rs"), &old).unwrap();
    fs::write(root.join("a.rs"), "unrelated\n").unwrap();
    commit(root, "C", "Ann", 10);
    git(root, &["rm", "-q", "a.rs"]);
    git(root, &["mv", "old.rs", "a.rs"]);
    commit(root, "D", "Bob", 3);
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);

    // Whichever commit git traces the row to, the change shown is of the
    // file the row was in there.
    keys("4 j", cx);
    dispatch("git.open_line_commit", cx);
    same_row_text(&workbench, &tab, 4, cx);
    let [_, right] = sides(&diff(&workbench, cx), cx);
    assert_eq!(text(&right, cx), old);
}

#[gpui::test]
fn asks_dropped_after_navigation(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    fs::write(root.join("b.rs"), "b\n").unwrap();
    let (workbench, mut cx) = window(cx, root, Some("b.rs"));
    let cx = &mut cx;
    open(&workbench, &root.join("a.rs"), cx);
    let tab = tab_editor(&workbench, cx);

    // Away and back before the commit's read lands: dropped.
    put_caret(&tab, 2, cx);
    send("git.open_line_commit", cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.activate(0, window, cx);
            pane.activate(1, window, cx);
        })
    });
    draw(cx);
    assert_eq!(tab_count(&workbench, cx), 2);
    assert_eq!(tab_editor(&workbench, cx), tab);

    // Asked again, it opens.
    dispatch("git.open_line_commit", cx);
    assert_eq!(tab_count(&workbench, cx), 3);
    same_row_text(&workbench, &tab, 2, cx);
}

#[test]
fn unquotes_git_paths() {
    let quoted = br#""q\"uo\\te caf\303\251\t.rs""#;
    assert_eq!(
        super::unquote(quoted),
        Path::new("q\"uo\\te caf\u{e9}\t.rs")
    );
    assert_eq!(super::unquote(b"plain.rs"), Path::new("plain.rs"));
}

#[gpui::test]
fn quoted_and_renamed_paths(cx: &mut TestAppContext) {
    // A name git quotes, with an accent, a quote and a backslash, changed
    // and then renamed.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let first: String = (0..6).map(|row| format!("r{row}\n")).collect();
    let second = first.replace("r2\n", "R2\n");
    let old = "q\"uo\\te caf\u{e9}.rs";
    fs::write(root.join(old), &first).unwrap();
    let c1 = commit(root, "Add it", "Ann", 10);
    fs::write(root.join(old), &second).unwrap();
    let c2 = commit(root, "Change r2", "Bob", 5);
    git(root, &["mv", old, "renam\u{e9}.rs"]);
    commit(root, "Rename it", "Bob", 3);
    let (workbench, mut cx) = window(cx, root, Some("renam\u{e9}.rs"));
    let cx = &mut cx;
    let (s1, s2) = (&c1[..7], &c2[..7]);

    // Both sides by the old name: `filename` and `previous`.
    keys("2 j", cx);
    dispatch("git.open_line_commit", cx);
    assert_eq!(
        title(&workbench, cx),
        format!("renam\u{e9}.rs ({s2}^ ↔ {s2})")
    );
    let [left, right] = sides(&diff(&workbench, cx), cx);
    assert_eq!([text(&left, cx), text(&right, cx)], [first.clone(), second]);
    assert_eq!(caret_row(&right, cx), 2);

    // The root commit's side.
    activate(&workbench, 0, cx);
    keys("g g", cx);
    dispatch("git.open_line_commit", cx);
    assert_eq!(
        title(&workbench, cx),
        format!("renam\u{e9}.rs ({s1}^ ↔ {s1})")
    );
    let [left, right] = sides(&diff(&workbench, cx), cx);
    assert_eq!([text(&left, cx), text(&right, cx)], [String::new(), first]);
}

#[gpui::test]
fn crlf_file(cx: &mut TestAppContext) {
    // Committed with CRLF line endings, as they are.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "core.autocrlf", "false"]);
    fs::write(root.join("a.rs"), "a\r\nb\r\nc\r\n").unwrap();
    commit(root, "C1", "Ann", 10);
    fs::write(root.join("a.rs"), "a\r\nB\r\nc\r\n").unwrap();
    let c2 = commit(root, "C2", "Bob", 3);
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    let s2 = &c2[..7];

    keys("j", cx);
    dispatch("git.toggle_line_blame", cx);
    settle(cx);
    assert_eq!(
        annotations(&tab, cx),
        [(1, "C2, Bob (3 days ago)".to_owned())]
    );
    dispatch("git.open_line_commit", cx);
    assert_eq!(title(&workbench, cx), format!("a.rs ({s2}^ ↔ {s2})"));
    let [left, right] = sides(&diff(&workbench, cx), cx);
    let rows = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        text(editor, cx)
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(rows(&left, cx), ["a", "b", "c"]);
    assert_eq!(rows(&right, cx), ["a", "B", "c"]);
    assert_eq!(caret_row(&right, cx), 1);
}

#[gpui::test]
fn lone_carriage_return(cx: &mut TestAppContext) {
    // HEAD's first row holds a lone `\r`, which blame doesn't split at:
    // the buffer's third row is HEAD's second.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "core.autocrlf", "false"]);
    fs::write(root.join("a.rs"), "a\rb\nc\n").unwrap();
    let c = commit(root, "C", "Ann", 10);
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    let short = &c[..7];

    dispatch("git.toggle_line_blame", cx);
    keys("2 j", cx);
    settle(cx);
    let row = caret_row(&tab, cx);
    assert_eq!(text(&tab, cx).lines().nth(row as usize), Some("c"));
    // The annotation and the commit opened agree.
    assert_eq!(
        annotations(&tab, cx),
        [(row, "C, Ann (10 days ago)".to_owned())]
    );
    dispatch("git.open_line_commit", cx);
    assert_eq!(title(&workbench, cx), format!("a.rs ({short}^ ↔ {short})"));
    let [_, right] = sides(&diff(&workbench, cx), cx);
    let at = caret_row(&right, cx);
    assert_eq!(text(&right, cx).lines().nth(at as usize), Some("c"));
}

#[test]
fn rows_matched_as_blame_matches_them() {
    use super::head_row;
    // A `\r` ending the text is dropped, as before a `\n`.
    assert_eq!(head_row("a\r", "a\n", 0), Some(0));
    // No row after a final `\n`: the buffer's added first row shifts x.
    assert_eq!(head_row("x\n", "\nx", 1), Some(0));
    assert_eq!(head_row("x\n", "\nx", 0), None);
    // A row within HEAD's first, after a lone `\r`, is no row of its own.
    assert_eq!(head_row("a\rb\nc\n", "a\nb\nc\n", 2), Some(1));
    // A character standing for `\r` in neither text: no false match.
    assert_eq!(head_row("a\rb\n", "a\u{f8ff}b\n", 0), None);
    assert_eq!(head_row("a\u{f8ff}b\n", "a\u{f8ff}b\n", 0), Some(0));
    // Under the size limit, however many `\r`s: still diffed.
    let crs = "\r".repeat(2 * 1024 * 1024);
    assert_eq!(head_row(&format!("a{crs}\nx\n"), "new\nq\nx\n", 2), Some(1));
    // Past the buffer's rows: none.
    assert_eq!(head_row("a\n", "a\n", 1), None);
    // Every control byte in the texts: rows still match.
    let controls: String = ((1u8..=31).map(char::from))
        .filter(|byte| !matches!(byte, '\t' | '\n' | '\r'))
        .collect();
    let head = format!("a\rb\n{controls}\nc\n");
    let text = format!("a\nb\n{controls}\nc\n");
    assert_eq!(head_row(&head, &text, 2), Some(1));
    assert_eq!(head_row(&head, &text, 3), Some(2));
    // A row that reads as another's stand-in would: still different.
    assert_eq!(head_row("a\rb\na\u{1}b\n", "z\na\u{1}b\n", 1), Some(1));
    assert_eq!(head_row("a\u{1}b\n", "a\rb\n", 0), None);
    // HEAD over 5 MiB as it is, under it read as the buffer: not blamed.
    let rows = 5 * 1024 * 1024 / 3 + 1;
    let (crlf, lf) = ("x\r\n".repeat(rows), "x\n".repeat(rows));
    assert!(crlf.len() > 5 * 1024 * 1024 && lf.len() < 5 * 1024 * 1024);
    assert_eq!(head_row(&crlf, &lf, 0), None);
    // Binary, as blame finds it: not blamed.
    assert_eq!(head_row("a\0\n", "a\0\n", 0), None);
}

#[test]
fn rows_matched_however_many_carriage_returns() {
    // Review 9mw7fb's F13: 3000 rows with a lone `\r`, then `tail`, then
    // rows of growing `\u{1}` runs. The buffer reads each lone `\r` as a
    // row break; `tail` is still HEAD's row 3000.
    let mut head = "\rX\n".repeat(3000);
    head.push_str("tail\n");
    for k in 0..3000 {
        head.push_str(&format!("\u{1}X{}\n", "\u{1}".repeat(k)));
    }
    let text = head.replace("\r\n", "\n").replace('\r', "\n");
    assert!(head.len() < 5 * 1024 * 1024);
    assert_eq!(super::head_row(&head, &text, 6000), Some(3000));
}

#[test]
fn side_rows_split_as_editors_do() {
    use super::side_row;
    assert_eq!(side_row("a\r\r\nc\n", 1), 2);
    assert_eq!(side_row("a\r\nc\n", 1), 1);
    assert_eq!(side_row("a\rb\nc\n", 1), 2);
    assert_eq!(side_row("a\nb\n", 0), 0);
}

#[gpui::test]
fn repeated_carriage_returns(cx: &mut TestAppContext) {
    // An empty row by a lone `\r` before a CRLF: the commit's caret goes to
    // `c`, in a new tab and in the one open.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "core.autocrlf", "false"]);
    fs::write(root.join("a.rs"), "a\r\r\nc\n").unwrap();
    let c = commit(root, "C", "Ann", 10);
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    let short = &c[..7];

    keys("2 j", cx);
    assert_eq!(text(&tab, cx).lines().nth(2), Some("c"));
    dispatch("git.open_line_commit", cx);
    assert_eq!(title(&workbench, cx), format!("a.rs ({short}^ ↔ {short})"));
    let by_c = diff(&workbench, cx);
    let [_, right] = sides(&by_c, cx);
    assert_eq!(caret_row(&right, cx), 2);
    assert_eq!(text(&right, cx).lines().nth(2), Some("c"));

    put_caret(&right, 0, cx);
    activate(&workbench, 0, cx);
    dispatch("git.open_line_commit", cx);
    assert_eq!(diff(&workbench, cx), by_c);
    assert_eq!(caret_row(&right, cx), 2);
}

/// Points `editor` at `path` as a Save As does, leaving the file alone.
fn save_as(editor: &Entity<EditorView>, path: &Path, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            editor.model_mut().set_path(Some(path.to_owned()));
            cx.notify();
        })
    });
}

#[gpui::test]
fn save_as_round_trip_keeps_cached_blame(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    fs::write(root.join("b.rs"), "b\n").unwrap();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    keys("2 j", cx);
    dispatch("git.toggle_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(2, expected(root, 2))]);

    // Save As b.rs and back before its blame is made: it's dropped, not
    // kept over a.rs's.
    save_as(&tab, &root.join("b.rs"), cx);
    save_as(&tab, &root.join("a.rs"), cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(2, expected(root, 2))]);
}

/// Reads HEAD for the marked editor, as a git refresh does.
fn check_head_now(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| workbench.update(cx, super::check_head));
}

/// Moves HEAD's commit, a reworded message and a new committer date.
fn amend(root: &Path) {
    let date = format!("@{} +0200", now() - 3 * DAY - 3600);
    let env = [("GIT_COMMITTER_DATE", date.as_str())];
    git_env(
        root,
        &["commit", "-q", "--amend", "-m", "Shout louder"],
        &env,
    );
}

#[gpui::test]
fn head_change_during_blame_revalidates(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    fs::write(root.join("b.rs"), "b\n").unwrap();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    keys("2 j", cx);
    dispatch("git.toggle_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(2, expected(root, 2))]);

    // HEAD moves while the blame of another file is pending, which going back
    // to the cached file cancels: the cached blame is of the old HEAD, and
    // is made again.
    amend(root);
    save_as(&tab, &root.join("b.rs"), cx);
    check_head_now(&workbench, cx);
    save_as(&tab, &root.join("a.rs"), cx);
    settle(cx);
    assert_eq!(expected(root, 2), "Shout louder, Bob (3 days ago)");
    assert_eq!(annotations(&tab, cx), [(2, expected(root, 2))]);
}

#[gpui::test]
fn head_check_survives_cancelled_blame(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    fs::write(root.join("b.rs"), "b\n").unwrap();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    keys("2 j", cx);
    dispatch("git.toggle_line_blame", cx);
    settle(cx);

    // A read deferred by a pending blame of b.rs, which is cancelled by
    // going back to a.rs's cached one: the read still runs.
    amend(root);
    save_as(&tab, &root.join("b.rs"), cx);
    check_head_now(&workbench, cx);
    save_as(&tab, &root.join("a.rs"), cx);
    settle(cx);
    assert_eq!(expected(root, 2), "Shout louder, Bob (3 days ago)");
    assert_eq!(annotations(&tab, cx), [(2, expected(root, 2))]);
}

/// Asks to open the caret row's commit, and runs tasks until the HEAD diff
/// behind an uncommitted row's open has been asked for, and no further.
fn ask_open(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    workbench.update(cx, |workbench, _| workbench.git_routed.clear());
    send("git.open_line_commit", cx);
    started(workbench, cx);
}

/// Runs tasks until the HEAD diff has been asked for.
fn started(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let asked = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, _| {
            (workbench.git_routed.iter()).any(
                |routed| matches!(routed, Routed::Op(op) if op.command == GitCommand::DiffHead),
            )
        })
    };
    for _ in 0..5000 {
        if asked(cx) {
            return;
        }
        // Idle while a read runs on its own thread.
        if !cx.executor().tick() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    panic!("the HEAD diff was never asked for");
}

/// Types `text` at `editor`'s caret, without letting anything run.
fn type_in(editor: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            editor.model_mut().insert(text);
            cx.notify();
        })
    });
}

#[gpui::test]
fn superseded_diff_open_does_not_steal_focus(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    fs::write(root.join("b.rs"), "b\n").unwrap();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    keys("A ! escape", cx);
    let focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| tab.focus_handle(cx).is_focused(window))
    };
    assert!(focused(cx));

    // Row 0 isn't committed: its change opens against HEAD, unless asked
    // about something else, moved off, edited or saved as meanwhile.
    let supersede: [&dyn Fn(&mut VisualTestContext); 4] = [
        &|cx| send("git.blame_line", cx),
        &|cx| put_caret(&tab, 1, cx),
        &|cx| type_in(&tab, "?", cx),
        &|cx| save_as(&tab, &root.join("b.rs"), cx),
    ];
    for (n, supersede) in supersede.into_iter().enumerate() {
        put_caret(&tab, 0, cx);
        ask_open(&workbench, cx);
        supersede(cx);
        draw(cx);
        assert_eq!(tab_count(&workbench, cx), 1, "superseded by {n}");
        assert!(focused(cx), "superseded by {n}");
    }

    // Not superseded, it opens.
    save_as(&tab, &root.join("a.rs"), cx);
    put_caret(&tab, 0, cx);
    dispatch("git.open_line_commit", cx);
    assert_eq!(tab_count(&workbench, cx), 2);
    assert_eq!(title(&workbench, cx), "a.rs (HEAD ↔ Working Tree)");
}

#[gpui::test]
fn pane_focus_round_trip_drops_pending_requests(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    keys("A ! escape", cx);
    dispatch("pane.split_right", cx);
    let panes = workbench.read_with(cx, |workbench, cx| workbench.panes(cx));
    let [first, second] = panes.try_into().expect("two panes");
    let active = |pane: &Entity<crate::Pane>, cx: &mut VisualTestContext| {
        let pane = pane.clone();
        cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| workbench.activate(pane, window, cx))
        });
    };
    put_caret(&tab_editor(&workbench, cx), 0, cx);
    ask_open(&workbench, cx);
    active(&first, cx);
    active(&second, cx);
    draw(cx);
    let tabs = |pane: &Entity<crate::Pane>, cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, _| pane.tabs().len())
    };
    assert_eq!([tabs(&first, cx), tabs(&second, cx)], [1, 1]);

    // Asked again, it opens.
    dispatch("git.open_line_commit", cx);
    assert_eq!(tabs(&second, cx), 2);

    // The round trip within one update, which notifies once, drops it too.
    activate(&workbench, 0, cx);
    put_caret(&tab_editor(&workbench, cx), 0, cx);
    ask_open(&workbench, cx);
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.activate(first.clone(), window, cx);
            workbench.activate(second.clone(), window, cx);
        })
    });
    draw(cx);
    assert_eq!(tabs(&second, cx), 2);
}

#[gpui::test]
fn head_check_deferred_runs_when_blame_lands(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    fs::write(root.join("c.rs"), "c\n").unwrap();
    let (workbench, mut cx) = window(cx, root, Some("c.rs"));
    let cx = &mut cx;

    // HEAD is read as c.rs's blame, which finds it untracked, is pending:
    // the read waits for the blame to land, then runs, and, as the blame
    // found nothing, blames again, without another refresh.
    send("git.toggle_line_blame", cx);
    check_head_now(&workbench, cx);
    assert_eq!(counted(cx).blames, 0, "pending");
    settle(cx);
    assert_eq!(counted(cx).blames, 2, "blamed, then again");
}
