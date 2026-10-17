//! Staging, undoing a stage of and reverting the change at the caret or
//! in a range, from a tab, the vim line, the palette's commands and a diff
//! view against the index, on repositories built with the `git` CLI in
//! temporary directories.

use std::{fs, path::Path, process::Command};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_git::Repo;
use fe_gpui::{
    DiffView, EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled,
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench};

/// Runs `git` in `dir`, with the developer's own config shut out: its
/// output.
fn git(dir: &Path, args: &[&str]) -> String {
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
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `a.rs`'s rows, `line 0` to `line 19`, with `changed N` in place of
/// those in `changed`.
fn rows(changed: &[u32]) -> String {
    (0..20)
        .map(|row| match changed.contains(&row) {
            true => format!("changed {row}\n"),
            false => format!("line {row}\n"),
        })
        .collect()
}

/// The three changes on disk: row 2, rows 8 and 9, rows 14 and 15.
const CHANGES: [&[u32]; 3] = [&[2], &[8, 9], &[14, 15]];

fn working() -> String {
    rows(&CHANGES.concat())
}

/// A repository whose `a.rs` is `rows(&[])` in HEAD and the index, and
/// `working()` on disk.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::write(root.join("a.rs"), rows(&[])).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("a.rs"), working()).unwrap();
    dir
}

/// A window of a workbench rooted at `root`, with vim on, and `a.rs` open
/// in a tab and focused: its editor.
fn window(
    cx: &mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, Entity<EditorView>, VisualTestContext) {
    window_on(cx, root, "a.rs")
}

/// The same, with `file` open.
fn window_on(
    cx: &mut TestAppContext,
    root: &Path,
    file: &str,
) -> (Entity<Workbench>, Entity<EditorView>, VisualTestContext) {
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
    let path = root.join(file);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the file is open");
    focus(&editor, cx);
    (workbench, editor, vcx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs workbench command `id` as the palette would, for the focused
/// editor.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
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

fn focus(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
}

/// `editor`'s vim message, emptied.
fn message(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            let message = editor.vim_state()?.message().map(str::to_owned);
            editor.set_vim_message("", cx);
            message.filter(|message| !message.is_empty())
        })
    })
}

/// What the window's shown toasts say, top first.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

/// `editor`'s buffer's text, and whether it's unsaved.
fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (String, bool) {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        (model.buffer().text().to_string(), model.is_dirty())
    })
}

fn index_text(root: &Path) -> String {
    let repo = Repo::discover(root).unwrap().unwrap();
    repo.index_text(&root.join("a.rs")).unwrap().unwrap()
}

/// The bytes the index has at `rela`, as `git cat-file` prints them: a
/// symlink's target.
fn index_blob(root: &Path, rela: &str) -> Vec<u8> {
    let output = Command::new("git")
        .args(["cat-file", "-p", &format!(":{rela}")])
        .current_dir(root)
        .output()
        .unwrap();
    output.stdout
}

/// The index's entry for `rela`: its mode, blob and path.
fn index_entry(root: &Path, rela: &str) -> String {
    git(root, &["ls-files", "--stage", "--", rela])
}

fn disk(root: &Path) -> String {
    fs::read_to_string(root.join("a.rs")).unwrap()
}

/// What `git diff --cached` adds and removes, its hunks' headers and
/// rows without context.
fn staged_diff(root: &Path) -> Vec<String> {
    let diff = git(root, &["diff", "--cached", "-U0"]);
    (diff.lines())
        .skip_while(|line| !line.starts_with("@@"))
        .map(|line| match line.starts_with("@@") {
            // Just the rows it names.
            true => line.split(" @@").next().unwrap().to_owned(),
            false => line.to_owned(),
        })
        .collect()
}

#[gpui::test]
fn stage_hunk(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, editor, mut cx) = window(cx, root);
    let cx = &mut cx;

    // The caret in the second change, from the palette's command.
    keys("1 0 shift-g", cx);
    dispatch("git.stage_hunk", cx);
    assert_eq!(index_text(root), rows(&[8, 9]));
    assert_eq!(
        staged_diff(root),
        [
            "@@ -9,2 +9,2",
            "-line 8",
            "-line 9",
            "+changed 8",
            "+changed 9"
        ]
    );
    assert_eq!(text(&editor, cx), (working(), false));
    assert_eq!(disk(root), working());
    assert_eq!(toasts(&workbench, cx), Vec::<String>::new());

    // An unsaved edit at the caret is staged as typed.
    keys("1 5 shift-g A x escape", cx);
    let typed = working().replace("changed 14\n", "changed 14x\n");
    assert_eq!(text(&editor, cx), (typed.clone(), true));
    ex("Gitsigns stage_hunk", cx);
    let staged = rows(&[8, 9, 14, 15]).replace("changed 14\n", "changed 14x\n");
    assert_eq!(index_text(root), staged);
    assert_eq!(message(&editor, cx), None);
    assert_eq!(text(&editor, cx), (typed, true));
    assert_eq!(disk(root), working());

    // Nothing to stage on an unchanged row: on the vim line, or in a toast.
    keys("6 shift-g", cx);
    ex("Gitsigns stage_hunk", cx);
    let nothing = "No change to stage at this line";
    assert_eq!(message(&editor, cx).as_deref(), Some(nothing));
    dispatch("git.stage_hunk", cx);
    assert_eq!(toasts(&workbench, cx), [nothing]);
    assert_eq!(index_text(root), staged);
}

#[gpui::test]
fn visual_range(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::write(root.join("a.rs"), rows(&[])).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    // Four rows added below row 5: rows 6 to 9.
    let added = "new a\nnew b\nnew c\nnew d\n";
    let file = rows(&[]).replace("line 5\n", &format!("line 5\n{added}"));
    fs::write(root.join("a.rs"), &file).unwrap();
    let (_, editor, mut cx) = window(cx, root);
    let cx = &mut cx;

    // Rows 7 and 8, `new b` and `new c`.
    keys("8 shift-g shift-v j", cx);
    ex("Gitsigns stage_hunk", cx);
    let staged = rows(&[]).replace("line 5\n", "line 5\nnew b\nnew c\n");
    assert_eq!(index_text(root), staged);
    assert_eq!(message(&editor, cx), None);
    assert_eq!(text(&editor, cx), (file.clone(), false));

    // The other two, as a palette command in visual line mode.
    keys("7 shift-g shift-v", cx);
    dispatch("git.stage_hunk", cx);
    let staged = rows(&[]).replace("line 5\n", "line 5\nnew a\nnew b\nnew c\n");
    assert_eq!(index_text(root), staged);
    keys("escape 1 0 shift-g", cx);
    ex("Gitsigns stage_hunk", cx);
    assert_eq!(index_text(root), file);
}

#[gpui::test]
fn undo_stage(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (_, editor, mut cx) = window(cx, root);
    let cx = &mut cx;

    ex("Gitsigns undo_stage_hunk", cx);
    let nothing = "No staged change to undo";
    assert_eq!(message(&editor, cx).as_deref(), Some(nothing));

    keys("9 shift-g", cx);
    ex("Gitsigns stage_hunk", cx);
    assert_eq!(index_text(root), rows(&[8, 9]));
    ex("Gitsigns undo_stage_hunk", cx);
    assert_eq!(message(&editor, cx), None);
    assert_eq!(index_text(root), rows(&[]));

    // Each stage undone in turn, the last first.
    ex("Gitsigns stage_hunk", cx);
    keys("3 shift-g", cx);
    ex("Gitsigns stage_hunk", cx);
    assert_eq!(index_text(root), rows(&[2, 8, 9]));
    dispatch("git.unstage_hunk", cx);
    assert_eq!(index_text(root), rows(&[8, 9]));

    // Not after the index changed since.
    git(root, &["add", "a.rs"]);
    ex("Gitsigns undo_stage_hunk", cx);
    let changed = "Index changed since; not undone";
    assert_eq!(message(&editor, cx).as_deref(), Some(changed));
    assert_eq!(index_text(root), working());
    assert_eq!(text(&editor, cx), (working(), false));
}

#[gpui::test]
fn revert(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (_, editor, mut cx) = window(cx, root);
    let cx = &mut cx;

    // At a changed row, from the palette: HEAD's rows, no question.
    keys("9 shift-g", cx);
    dispatch("git.revert_hunk", cx);
    assert!(!cx.has_pending_prompt());
    assert_eq!(text(&editor, cx), (rows(&[2, 14, 15]), true));
    keys("u", cx);
    assert_eq!(text(&editor, cx), (working(), false));

    // On the vim line.
    keys("3 shift-g", cx);
    ex("Gitsigns reset_hunk", cx);
    assert!(!cx.has_pending_prompt());
    assert_eq!(message(&editor, cx), None);
    assert_eq!(text(&editor, cx), (rows(&[8, 9, 14, 15]), true));
    keys("u", cx);
    assert_eq!(text(&editor, cx), (working(), false));

    // A range over parts of two changes, rows 9 to 14, reverts both whole.
    keys("1 0 shift-g shift-v 1 5 shift-g", cx);
    ex("Gitsigns reset_hunk", cx);
    assert_eq!(text(&editor, cx), (rows(&[2]), true));
    keys("u", cx);
    assert_eq!(text(&editor, cx), (working(), false));

    // Nothing to revert on an unchanged row.
    keys("6 shift-g", cx);
    ex("Gitsigns reset_hunk", cx);
    let nothing = "No change to revert at this line";
    assert_eq!(message(&editor, cx).as_deref(), Some(nothing));
    assert_eq!(disk(root), working());
}

#[gpui::test]
fn diff_view_put(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, _, mut cx) = window(cx, root);
    let cx = &mut cx;
    ex("Gdiffsplit", cx);
    let diff = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        (pane.active_item())
            .and_then(|item| item.downcast_ref::<DiffView>().cloned())
            .expect("a diff view")
    });
    let [left, right] = diff.read_with(cx, |diff, _| [diff.left().clone(), diff.right().clone()]);
    let left_text = |cx: &mut VisualTestContext| text(&left, cx).0;
    focus(&right, cx);

    // `dp` on the right side's second change stages it whole.
    keys("9 shift-g d p", cx);
    assert_eq!(message(&right, cx), None);
    assert_eq!(index_text(root), rows(&[8, 9]));
    assert_eq!(left_text(cx), rows(&[8, 9]));
    assert_eq!(text(&right, cx), (working(), false));

    // `:diffput` on the third.
    keys("1 6 shift-g", cx);
    ex("diffput", cx);
    assert_eq!(message(&right, cx), None);
    assert_eq!(index_text(root), rows(&[8, 9, 14, 15]));
    assert_eq!(left_text(cx), rows(&[8, 9, 14, 15]));

    // And `git.stage_hunk` there, the first.
    keys("3 shift-g", cx);
    dispatch("git.stage_hunk", cx);
    assert_eq!(index_text(root), working());
    assert_eq!(left_text(cx), working());
    assert_eq!(disk(root), working());

    // From the git side, nothing.
    focus(&left, cx);
    ex("Gitsigns stage_hunk", cx);
    let git_side = "Stage from the Working Tree side";
    assert_eq!(message(&left, cx).as_deref(), Some(git_side));
}

#[gpui::test]
fn stage_through_a_symlink(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::write(root.join("a.rs"), rows(&[])).unwrap();
    std::os::unix::fs::symlink("a.rs", root.join("l.rs")).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("a.rs"), working()).unwrap();
    let (_, editor, mut cx) = window_on(cx, root, "l.rs");
    let cx = &mut cx;
    assert_eq!(text(&editor, cx), (working(), false));

    // The file the link points to takes the change; the link stays a link.
    keys("9 shift-g", cx);
    ex("Gitsigns stage_hunk", cx);
    assert_eq!(message(&editor, cx), None);
    assert_eq!(index_text(root), rows(&[8, 9]));
    assert_eq!(index_blob(root, "l.rs"), b"a.rs");
    assert!(index_entry(root, "l.rs").starts_with("120000 "));
    ex("Gitsigns undo_stage_hunk", cx);
    assert_eq!(message(&editor, cx), None);
    assert_eq!(index_text(root), rows(&[]));
    assert_eq!(index_blob(root, "l.rs"), b"a.rs");
}

/// `text` after a Latin-1 `café` row.
fn latin1(text: &str) -> Vec<u8> {
    [b"caf\xe9\n".as_slice(), text.as_bytes()].concat()
}

/// A repository whose `a.rs` is `latin1(&rows(&[]))` in HEAD and the
/// index, and `file` on disk.
fn latin1_fixture(file: &[u8]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::write(root.join("a.rs"), latin1(&rows(&[]))).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("a.rs"), file).unwrap();
    dir
}

const NOT_UTF8: &str = "Can't stage part of a file that isn't UTF-8";

#[gpui::test]
fn stage_refuses_an_index_not_utf8(cx: &mut TestAppContext) {
    // The buffer's UTF-8, the index's blob Latin-1: staging an ASCII change
    // elsewhere would rewrite its `é`.
    let utf8 = format!("café\n{}", working());
    let dir = latin1_fixture(utf8.as_bytes());
    let root = dir.path();
    let (_, editor, mut cx) = window(cx, root);
    let cx = &mut cx;
    assert_eq!(text(&editor, cx).0, utf8);
    keys("1 0 shift-g", cx);
    ex("Gitsigns stage_hunk", cx);
    assert_eq!(message(&editor, cx).as_deref(), Some(NOT_UTF8));
    assert_eq!(index_blob(root, "a.rs"), latin1(&rows(&[])));
}

#[gpui::test]
fn stage_refuses_a_file_not_utf8(cx: &mut TestAppContext) {
    let dir = latin1_fixture(&latin1(&working()));
    let root = dir.path();
    let (_, editor, mut cx) = window(cx, root);
    let cx = &mut cx;
    keys("1 0 shift-g", cx);
    ex("Gitsigns stage_hunk", cx);
    assert_eq!(message(&editor, cx).as_deref(), Some(NOT_UTF8));
    assert_eq!(index_blob(root, "a.rs"), latin1(&rows(&[])));
}

#[gpui::test]
fn undo_stage_through_a_clean_filter(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    // A filter that strips trailing blanks as git adds the file.
    git(
        root,
        &["config", "filter.strip.clean", "sed -e 's/[[:space:]]*$//'"],
    );
    fs::write(root.join(".git/info/attributes"), "a.rs filter=strip\n").unwrap();
    let file = working().replace("changed 2\n", "changed 2  \n");
    let file = file.replace("changed 8\n", "changed 8  \n");
    fs::write(root.join("a.rs"), &file).unwrap();
    let original = index_entry(root, "a.rs");
    let (_, editor, mut cx) = window(cx, root);
    let cx = &mut cx;

    // Undone at once: the index has what git wrote, filtered.
    keys("9 shift-g", cx);
    ex("Gitsigns stage_hunk", cx);
    assert_eq!(index_text(root), rows(&[8, 9]));
    ex("Gitsigns undo_stage_hunk", cx);
    assert_eq!(message(&editor, cx), None);
    assert_eq!(index_entry(root, "a.rs"), original);

    // Two, undone in turn, back to the very blob that was there.
    ex("Gitsigns stage_hunk", cx);
    keys("3 shift-g", cx);
    ex("Gitsigns stage_hunk", cx);
    assert_eq!(index_text(root), rows(&[2, 8, 9]));
    let both = index_entry(root, "a.rs");
    ex("Gitsigns undo_stage_hunk", cx);
    assert_eq!(index_text(root), rows(&[8, 9]));
    ex("Gitsigns undo_stage_hunk", cx);
    assert_eq!(message(&editor, cx), None);
    assert_eq!(index_entry(root, "a.rs"), original);
    assert_ne!(both, original);
}

#[gpui::test]
fn undo_stage_after_an_outside_deletion(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (_, editor, mut cx) = window(cx, root);
    let cx = &mut cx;
    keys("9 shift-g", cx);
    ex("Gitsigns stage_hunk", cx);
    assert_eq!(index_text(root), rows(&[8, 9]));

    // Its deletion staged outside: the index changed, and the stage is
    // forgotten.
    git(root, &["rm", "-q", "-f", "--cached", "a.rs"]);
    ex("Gitsigns undo_stage_hunk", cx);
    let changed = "Index changed since; not undone";
    assert_eq!(message(&editor, cx).as_deref(), Some(changed));
    assert_eq!(index_entry(root, "a.rs"), "");
    ex("Gitsigns undo_stage_hunk", cx);
    let nothing = "No staged change to undo";
    assert_eq!(message(&editor, cx).as_deref(), Some(nothing));
}

#[gpui::test]
fn visual_line_ending_on_an_empty_row(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::write(root.join("a.rs"), rows(&[])).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    // Changes: row 2; row 4, emptied; rows 8 and 9, the second emptied.
    let empty = |text: String| text.replace("line 4\n", "\n").replace("line 9\n", "\n");
    let file = empty(rows(&[2, 8]));
    fs::write(root.join("a.rs"), &file).unwrap();
    let (_, editor, mut cx) = window(cx, root);
    let cx = &mut cx;

    // From the palette in visual line mode over rows 8 and 9.
    keys("9 shift-g shift-v j", cx);
    dispatch("git.stage_hunk", cx);
    let staged = rows(&[8]).replace("line 9\n", "\n");
    assert_eq!(index_text(root), staged);

    // Over rows 2 to 4, an unchanged row between the changes.
    keys("escape 3 shift-g shift-v j j", cx);
    dispatch("git.revert_hunk", cx);
    assert_eq!(text(&editor, cx), (staged, true));
    keys("escape u", cx);
    assert_eq!(text(&editor, cx), (file, false));
}
