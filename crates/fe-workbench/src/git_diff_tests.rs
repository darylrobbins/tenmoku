//! Diff views of a file against the index, HEAD or another file, opened
//! from the palette, the vim line, the Changes panel and the change bar,
//! on repositories built with the `git` CLI in temporary directories; and
//! vim's diff commands in their sides.

use std::{fs, path::Path, process::Command};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{
    DiffView, EditorSettings, EditorTheme, EditorView,
    commands::{EditorViewEvent, RunCommand},
    set_vim_enabled,
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext, px, size};
use kiln_shell::PaneView as _;

use crate::{
    Launch, Workbench,
    workbench::git_commands::{GitCommand, GitOp},
};

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
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
}

/// `a.rs`'s rows, `line 0` to `line 19`, with `changed` in place of some.
fn rows(changed: &[(usize, &str)]) -> String {
    (0..20)
        .map(|row| match changed.iter().find(|(at, _)| *at == row) {
            Some((_, text)) => format!("{text}\n"),
            None => format!("line {row}\n"),
        })
        .collect()
}

fn head() -> String {
    rows(&[])
}

/// HEAD's text with a staged edit on row 2.
fn index() -> String {
    rows(&[(2, "staged 2")])
}

/// The index's text with an unstaged edit on row 10.
fn working() -> String {
    rows(&[(2, "staged 2"), (10, "unstaged 10")])
}

const B: &str = "b one\nb two\n";
const D: &str = "d\n";

/// A repository with `a.rs` staged and edited again, `b.rs` as committed
/// and `d.rs` deleted.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::write(root.join("a.rs"), head()).unwrap();
    fs::write(root.join("b.rs"), B).unwrap();
    fs::write(root.join("d.rs"), D).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("a.rs"), index()).unwrap();
    git(root, &["add", "a.rs"]);
    fs::write(root.join("a.rs"), working()).unwrap();
    fs::remove_file(root.join("d.rs")).unwrap();
    dir
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
        let path = root.join(file);
        let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    (workbench, vcx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
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

/// A repository with `files` committed, as `(path, text)`.
fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    for (path, text) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    dir
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

/// The active tab's diff view, if it shows one.
fn active_diff(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<DiffView>> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.active_item()?.downcast_ref::<DiffView>().cloned()
    })
}

fn diff(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<DiffView> {
    active_diff(workbench, cx).expect("a diff view is the active tab")
}

/// The active tab's editor, if it's an editor.
fn tab_editor(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<EditorView>> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.pane().read(cx).active_editor().cloned()
    })
}

fn sides(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> [Entity<EditorView>; 2] {
    diff.read_with(cx, |diff, _| [diff.left().clone(), diff.right().clone()])
}

/// `editor`'s buffer's text, which a hidden editor's model catches up with
/// only as it's drawn.
fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string())
}

fn texts(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> [String; 2] {
    sides(diff, cx).map(|side| text(&side, cx))
}

fn read_only(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> [bool; 2] {
    sides(diff, cx).map(|side| side.read_with(cx, |side, _| side.is_read_only()))
}

fn shares_buffer(
    a: &Entity<EditorView>,
    b: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> bool {
    cx.update(|_, cx| a.read(cx).model().shares_buffer_with(b.read(cx).model()))
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

fn focus(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
}

#[gpui::test]
fn opens_index_and_head(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx).expect("a.rs is open");

    dispatch("git.diff_index", cx);
    let by_index = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), "a.rs (Index ↔ Working Tree)");
    assert_eq!(texts(&by_index, cx), [index(), working()]);
    assert_eq!(read_only(&by_index, cx), [true, false]);
    let [_, right] = sides(&by_index, cx);
    assert!(
        shares_buffer(&right, &tab, cx),
        "the right side is the open buffer"
    );
    assert!(cx.update(|window, cx| right.focus_handle(cx).is_focused(window)));

    // From the diff view, its file.
    dispatch("git.diff_head", cx);
    let by_head = diff(&workbench, cx);
    assert_ne!(by_head, by_index);
    assert_eq!(title(&workbench, cx), "a.rs (HEAD ↔ Working Tree)");
    assert_eq!(texts(&by_head, cx), [head(), working()]);
    assert_eq!(tab_count(&workbench, cx), 3);

    // Asking again shows the same tab.
    activate(&workbench, 0, cx);
    dispatch("git.diff_index", cx);
    assert_eq!(diff(&workbench, cx), by_index);
    activate(&workbench, 0, cx);
    dispatch("git.diff_head", cx);
    assert_eq!(diff(&workbench, cx), by_head);
    assert_eq!(tab_count(&workbench, cx), 3);

    // And typed on the vim line.
    activate(&workbench, 0, cx);
    ex("Gdiffsplit", cx);
    assert_eq!(diff(&workbench, cx), by_index);
    activate(&workbench, 0, cx);
    ex("Gdiffsplit HEAD", cx);
    assert_eq!(diff(&workbench, cx), by_head);
    assert_eq!(tab_count(&workbench, cx), 3);

    // An edit in either editor shows in the other.
    keys("i x escape", cx);
    assert_eq!(text(&tab, cx), format!("x{}", working()));
}

#[gpui::test]
fn outside_a_repository(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "a\n").unwrap();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx).unwrap();
    ex("Gdiffsplit", cx);
    assert_eq!(
        message(&tab, cx).as_deref(),
        Some("Not in a git repository")
    );
    dispatch("git.diff_head", cx);
    assert_eq!(toasts(&workbench, cx), ["Not in a git repository"]);
    assert_eq!(tab_count(&workbench, cx), 1);
}

#[gpui::test]
fn panel_and_bar(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx).unwrap();

    // The change bar: HEAD against the buffer, the right caret on the
    // change's first row, centred.
    cx.update(|_, cx| {
        tab.update(cx, |_, cx| {
            cx.emit(EditorViewEvent::ChangeBarClicked { rows: 9..12 })
        })
    });
    draw(cx);
    let by_bar = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), "a.rs (HEAD ↔ Working Tree)");
    assert_eq!(texts(&by_bar, cx), [head(), working()]);
    let [_, right] = sides(&by_bar, cx);
    assert!(shares_buffer(&right, &tab, cx));
    assert_eq!(caret_row(&right, cx), 9);
    let (top, shown) = right.read_with(cx, |right, _| right.viewport_rows().expect("drawn"));
    assert!(top <= 9 && 9 < top + shown, "row 9 in {top}+{shown}");
    ex("q", cx);
    assert!(active_diff(&workbench, cx).is_none());

    // Enter on a Changes panel row: a staged file's HEAD against the
    // index, neither side editable.
    dispatch("git.show_changes", cx);
    keys("down", cx);
    keys("enter", cx);
    let staged = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), "a.rs (HEAD ↔ Index)");
    assert_eq!(texts(&staged, cx), [head(), index()]);
    assert_eq!(read_only(&staged, cx), [true, true]);

    // A changed file's: the index against the file.
    dispatch("git.show_changes", cx);
    keys("down", cx);
    keys("enter", cx);
    let by_panel = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), "a.rs (Index ↔ Working Tree)");
    assert_eq!(texts(&by_panel, cx), [index(), working()]);

    // A deleted file's: the index against nothing, neither side editable.
    dispatch("git.show_changes", cx);
    keys("down", cx);
    keys("enter", cx);
    let deleted = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), "d.rs (Index ↔ Working Tree)");
    assert_eq!(texts(&deleted, cx), [D.to_owned(), String::new()]);
    assert_eq!(read_only(&deleted, cx), [true, true]);

    // Alt-Enter opens the file itself; a deleted one, nothing.
    dispatch("git.show_changes", cx);
    keys("alt-enter", cx);
    assert_eq!(diff(&workbench, cx), deleted);
    keys("up", cx);
    keys("alt-enter", cx);
    assert_eq!(tab_editor(&workbench, cx), Some(tab));
}

#[gpui::test]
fn panel_opens_a_file_no_tab_has(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), None);
    let cx = &mut cx;
    // The changed row, after the staged one.
    dispatch("git.show_changes", cx);
    keys("down down", cx);
    keys("enter", cx);
    let by_panel = diff(&workbench, cx);
    assert_eq!(texts(&by_panel, cx), [index(), working()]);
    assert_eq!(tab_count(&workbench, cx), 1);

    // Its right side is the file, saved as it.
    let [_, right] = sides(&by_panel, cx);
    focus(&right, cx);
    keys("g g i x escape", cx);
    ex("w", cx);
    let saved = fs::read_to_string(dir.path().join("a.rs")).unwrap();
    assert_eq!(saved, format!("x{}", working()));
}

#[gpui::test]
fn compare_file(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx).unwrap();

    dispatch("diff.compare_file", cx);
    cx.simulate_input("b.rs");
    draw(cx);
    keys("enter", cx);
    let picked = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), "a.rs (b.rs ↔ a.rs)");
    assert_eq!(texts(&picked, cx), [B.to_owned(), working()]);
    assert_eq!(read_only(&picked, cx), [true, false]);
    let [_, right] = sides(&picked, cx);
    assert!(shares_buffer(&right, &tab, cx));

    activate(&workbench, 0, cx);
    ex("diffsplit b.rs", cx);
    let typed = diff(&workbench, cx);
    assert_ne!(typed, picked);
    assert_eq!(title(&workbench, cx), "a.rs (b.rs ↔ a.rs)");
    assert_eq!(texts(&typed, cx), [B.to_owned(), working()]);
    assert_eq!(read_only(&typed, cx), [true, false]);

    activate(&workbench, 0, cx);
    ex("diffsplit nope.rs", cx);
    let missing = format!(
        "Can't open {}: No such file",
        dir.path().join("nope.rs").display()
    );
    assert_eq!(message(&tab, cx), Some(missing));
}

#[gpui::test]
fn vim_diff_commands(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx).unwrap();
    ex("Gdiffsplit HEAD", cx);
    let by_head = diff(&workbench, cx);
    let [left, right] = sides(&by_head, cx);
    let reverted = rows(&[(2, "staged 2")]);

    // `do` on the right side's change at row 10 takes HEAD's row; `u`
    // undoes it.
    keys("1 1 shift-g", cx);
    keys("d o", cx);
    assert_eq!(text(&right, cx), reverted);
    keys("u", cx);
    assert_eq!(text(&right, cx), working());

    // The left side can't change.
    focus(&left, cx);
    keys("1 1 shift-g", cx);
    keys("d o", cx);
    let e21 = "E21: Cannot make changes, 'modifiable' is off";
    assert_eq!(message(&left, cx).as_deref(), Some(e21));
    ex("w", cx);
    assert_eq!(message(&left, cx).as_deref(), Some(e21));
    assert_eq!(text(&left, cx), head());

    // `dp` from it puts its row into the right side.
    keys("d p", cx);
    assert_eq!(text(&right, cx), reverted);
    assert_eq!(text(&tab, cx), reverted, "the buffer is the file's");

    // Nothing can be put into HEAD.
    focus(&right, cx);
    keys("3 shift-g", cx);
    keys("d p", cx);
    let e793 = "E793: No other buffer in diff mode is modifiable";
    assert_eq!(message(&right, cx).as_deref(), Some(e793));
    assert_eq!(text(&left, cx), head());

    // `:q` closes the diff tab: its changes live on in the file's tab.
    ex("q", cx);
    assert!(active_diff(&workbench, cx).is_none());
    assert_eq!(tab_count(&workbench, cx), 1);
    assert_eq!(tab_editor(&workbench, cx), Some(tab));
}

#[gpui::test]
fn quit_refuses_a_dirty_diff_no_tab_shows(cx: &mut TestAppContext) {
    let dir = fixture();
    // Another file's tab, so the window stays open.
    let (workbench, mut cx) = window(cx, dir.path(), Some("b.rs"));
    let cx = &mut cx;
    let other = tab_editor(&workbench, cx).unwrap();
    dispatch("git.show_changes", cx);
    keys("down down", cx);
    keys("enter", cx);
    let [_, right] = sides(&diff(&workbench, cx), cx);
    focus(&right, cx);
    keys("i x escape", cx);
    ex("q", cx);
    let e37 = "E37: No write since last change (add ! to override)";
    assert_eq!(message(&right, cx).as_deref(), Some(e37));
    assert!(active_diff(&workbench, cx).is_some());
    ex("q!", cx);
    assert_eq!(tab_count(&workbench, cx), 1);
    assert_eq!(tab_editor(&workbench, cx), Some(other));
}

#[gpui::test]
fn refresh_and_navigation(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx).unwrap();
    ex("Gdiffsplit", cx);
    let by_index = diff(&workbench, cx);
    let [left, _] = sides(&by_index, cx);
    assert_eq!(text(&left, cx), index());

    // Staged outside: the refresh reads the index again.
    git(dir.path(), &["add", "a.rs"]);
    dispatch("git.refresh", cx);
    assert_eq!(text(&left, cx), working());

    // In a tab, its changes against HEAD, wrapping.
    activate(&workbench, 0, cx);
    assert_eq!(caret_row(&tab, cx), 0);
    dispatch("git.next_change", cx);
    assert_eq!(caret_row(&tab, cx), 2);
    dispatch("git.next_change", cx);
    assert_eq!(caret_row(&tab, cx), 10);
    assert!(toasts(&workbench, cx).is_empty());
    dispatch("git.next_change", cx);
    assert_eq!(caret_row(&tab, cx), 2);
    assert_eq!(toasts(&workbench, cx), ["Wrapped to first change"]);
    ex("Gitsigns prev_hunk", cx);
    assert_eq!(caret_row(&tab, cx), 10);
    assert_eq!(message(&tab, cx).as_deref(), Some("Wrapped to last change"));
    dispatch("git.previous_change", cx);
    assert_eq!(caret_row(&tab, cx), 2);
}

/// Opens the diff of `file`, under the root, against HEAD.
fn open_head_diff(workbench: &Entity<Workbench>, file: &str, cx: &mut VisualTestContext) {
    let path = workbench.read_with(cx, |workbench, _| workbench.root().unwrap().join(file));
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.open_head_diff(path, window, cx)
        })
    });
    draw(cx);
}

/// Opens `a.rs`'s diff against HEAD as a Changes panel row did, no tab
/// having the file open: its working side is the only editor of it.
fn diff_only(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<DiffView> {
    open_head_diff(workbench, "a.rs", cx);
    let diff = diff(workbench, cx);
    assert_eq!(title(workbench, cx), "a.rs (HEAD ↔ Working Tree)");
    diff
}

#[gpui::test]
fn write_all_and_quit_all_count_a_diff_only_file(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("b.rs"));
    let cx = &mut cx;
    let [left, right] = sides(&diff_only(&workbench, cx), cx);
    focus(&right, cx);
    keys("g g i x escape", cx);

    // From the clean left side: the working side's changes would be lost.
    focus(&left, cx);
    ex("qa", cx);
    let e162 = message(&left, cx).expect("an answer");
    assert!(
        e162.starts_with("E162: No write since last change"),
        "{e162}"
    );

    ex("wa", cx);
    let saved = fs::read_to_string(dir.path().join("a.rs")).unwrap();
    assert_eq!(saved, format!("x{}", working()));
    let dirty = right.read_with(cx, |right, _| right.model().is_dirty());
    assert!(!dirty);
}

#[gpui::test]
fn a_diff_only_buffer_is_the_files_open_buffer(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), None);
    let cx = &mut cx;
    let by_head = diff_only(&workbench, cx);
    let [_, right] = sides(&by_head, cx);
    focus(&right, cx);
    keys("g g i x escape", cx);

    // Another base, asked for from the diff: the same buffer, edits and all.
    ex("Gdiffsplit", cx);
    let by_index = diff(&workbench, cx);
    assert_ne!(by_index, by_head);
    let [_, other] = sides(&by_index, cx);
    assert!(shares_buffer(&other, &right, cx));
    assert_eq!(texts(&by_index, cx), [index(), format!("x{}", working())]);

    // A tab opened on the file shares it too.
    ex("e a.rs", cx);
    let tab = tab_editor(&workbench, cx).expect("a.rs opened in a tab");
    assert!(shares_buffer(&tab, &right, cx));
    assert_eq!(text(&tab, cx), format!("x{}", working()));
}

#[gpui::test]
fn disk_changes_reload_a_diff_only_buffer(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), None);
    let cx = &mut cx;
    let [_, right] = sides(&diff_only(&workbench, cx), cx);
    fs::write(dir.path().join("a.rs"), "changed outside\n").unwrap();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.check_open_files(window, cx))
    });
    draw(cx);
    assert_eq!(text(&right, cx), "changed outside\n");
}

#[gpui::test]
fn a_file_whose_folder_was_deleted_compares_with_nothing(cx: &mut TestAppContext) {
    let dir = repo(&[("c/e.rs", "e\n")]);
    fs::remove_dir_all(dir.path().join("c")).unwrap();
    let (workbench, mut cx) = window(cx, dir.path(), None);
    let cx = &mut cx;
    dispatch("git.show_changes", cx);
    keys("down", cx);
    keys("enter", cx);
    let deleted = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), "e.rs (Index ↔ Working Tree)");
    assert_eq!(texts(&deleted, cx), ["e\n".to_owned(), String::new()]);
    assert_eq!(read_only(&deleted, cx), [true, true]);
}

#[gpui::test]
fn deleting_and_restoring_the_file_rebuilds_its_diff(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), None);
    let cx = &mut cx;
    let shown = diff_only(&workbench, cx);
    let path = dir.path().join("a.rs");

    // Asked for as deleted (its Changes row once the list catches up): the
    // clean diff is rebuilt in its tab.
    fs::remove_file(&path).unwrap();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.open_head_diff(path.clone(), window, cx)
        })
    });
    draw(cx);
    let deleted = diff(&workbench, cx);
    assert_ne!(deleted, shown);
    assert_eq!(tab_count(&workbench, cx), 1);
    assert_eq!(texts(&deleted, cx), [head(), String::new()]);
    assert_eq!(read_only(&deleted, cx), [true, true]);

    // A refresh finds it restored.
    fs::write(&path, working()).unwrap();
    dispatch("git.refresh", cx);
    let restored = diff(&workbench, cx);
    assert_ne!(restored, deleted);
    assert_eq!(tab_count(&workbench, cx), 1);
    assert_eq!(texts(&restored, cx), [head(), working()]);
    assert_eq!(read_only(&restored, cx), [true, false]);

    // And deleted again.
    fs::remove_file(&path).unwrap();
    dispatch("git.refresh", cx);
    assert_eq!(texts(&diff(&workbench, cx), cx), [head(), String::new()]);

    // A diff with unsaved changes stays.
    fs::write(&path, working()).unwrap();
    dispatch("git.refresh", cx);
    let [_, right] = sides(&diff(&workbench, cx), cx);
    focus(&right, cx);
    keys("i x escape", cx);
    let edited = diff(&workbench, cx);
    fs::remove_file(&path).unwrap();
    dispatch("git.refresh", cx);
    assert_eq!(diff(&workbench, cx), edited);
}

#[gpui::test]
fn only_a_views_last_read_lands(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    ex("Gdiffsplit", cx);
    let by_index = diff(&workbench, cx);
    let view = by_index.downgrade();
    let [left, _] = sides(&by_index, cx);
    let (first, second) = cx.update(|_, cx| {
        (
            super::start_read(&view, cx).unwrap(),
            super::start_read(&view, cx).unwrap(),
        )
    });
    let landed = cx.update(|_, cx| super::land_read(&view, second, "second\n", None, cx));
    assert!(landed);
    let landed = cx.update(|_, cx| super::land_read(&view, first, "first\n", None, cx));
    assert!(!landed, "an older read lands after a newer one");
    assert_eq!(text(&left, cx), "second\n");

    // An open that lands after a refresh began reads its git side again.
    activate(&workbench, 0, cx);
    send("git.diff_head", cx);
    let reads = cx.update(|_, cx| super::state(cx).reads);
    workbench.update(cx, |workbench, cx| workbench.refresh_git_diffs(cx));
    draw(cx);
    let by_head = diff(&workbench, cx);
    assert_eq!(texts(&by_head, cx), [head(), working()]);
    let read_again = cx.update(|_, cx| super::state(cx).reads);
    // The refresh read the Index diff's, and the HEAD diff's was read again.
    assert_eq!(read_again, reads + 2);
}

#[gpui::test]
fn next_change_passes_a_deletion_at_the_end(cx: &mut TestAppContext) {
    let dir = repo(&[("a.rs", "old\nkeep\nremoved")]);
    fs::write(dir.path().join("a.rs"), "new\nkeep").unwrap();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx).unwrap();
    // The deletion, after the last row, puts the caret on the row above.
    dispatch("git.next_change", cx);
    assert_eq!(caret_row(&tab, cx), 1);
    dispatch("git.next_change", cx);
    assert_eq!(caret_row(&tab, cx), 0);
    assert_eq!(toasts(&workbench, cx), ["Wrapped to first change"]);
    ex("Gitsigns prev_hunk", cx);
    assert_eq!(caret_row(&tab, cx), 1);
    assert_eq!(message(&tab, cx).as_deref(), Some("Wrapped to last change"));
    ex("Gitsigns prev_hunk", cx);
    assert_eq!(caret_row(&tab, cx), 0);
}

#[gpui::test]
fn files_too_large_to_edit_are_not_compared(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("b.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx).unwrap();
    // a.rs is larger than this, b.rs smaller.
    crate::loader::test_support::lower_threshold(dir.path(), 64, false);

    dispatch("git.show_changes", cx);
    keys("down", cx);
    keys("enter", cx);
    assert_eq!(toasts(&workbench, cx), ["Too large to compare: a.rs"]);
    assert_eq!(tab_count(&workbench, cx), 1);

    activate(&workbench, 0, cx);
    ex("diffsplit a.rs", cx);
    assert_eq!(
        message(&tab, cx).as_deref(),
        Some("Too large to compare: a.rs")
    );
    assert_eq!(tab_count(&workbench, cx), 1);
}

#[gpui::test]
fn only_the_last_diff_asked_for_is_shown(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let titles = |pane: &Entity<crate::Pane>, cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, cx| {
            (pane.tabs().iter())
                .map(|tab| tab.item().tab_title(cx).to_string())
                .collect::<Vec<_>>()
        })
    };

    // Asked for in one pane, landing after another is made active: in the
    // pane it was asked in, not shown over the new one.
    let first = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    send("git.diff_index", cx);
    send("pane.split_right", cx);
    draw(cx);
    let second = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_ne!(second, first);
    assert_eq!(titles(&first, cx), ["a.rs", "a.rs (Index ↔ Working Tree)"]);
    assert_eq!(titles(&second, cx), ["a.rs"]);
    let shown = first.read_with(cx, |pane, _| pane.active());
    assert_eq!(shown, Some(0), "not shown over the tab navigated from");

    // Two asked for at once: the last is shown, whichever lands first.
    send("git.diff_head", cx);
    workbench.update_in(cx, |workbench, window, cx| {
        let op = GitOp {
            command: GitCommand::CompareFile,
            args: "b.rs".to_owned(),
            range: None,
            editor: workbench.active_editor(cx).cloned(),
            typed: false,
        };
        workbench.run_git_op(op, window, cx);
    });
    draw(cx);
    assert_eq!(title(&workbench, cx), "a.rs (b.rs ↔ a.rs)");
    // In the order they landed.
    let mut landed = titles(&second, cx);
    landed.sort();
    assert_eq!(
        landed,
        ["a.rs", "a.rs (HEAD ↔ Working Tree)", "a.rs (b.rs ↔ a.rs)"]
    );
}

#[gpui::test]
fn change_bar_opens_side_by_side(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx).unwrap();
    // Narrow enough that a diff would start inline.
    cx.simulate_resize(size(px(600.), px(600.)));
    draw(cx);
    let click = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            tab.update(cx, |_, cx| {
                cx.emit(EditorViewEvent::ChangeBarClicked { rows: 10..11 })
            })
        });
        draw(cx);
    };
    click(cx);
    let by_bar = diff(&workbench, cx);
    assert!(!by_bar.read_with(cx, |diff, _| diff.is_inline()));
    let [_, right] = sides(&by_bar, cx);
    assert_eq!(caret_row(&right, cx), 10);

    // One shown inline is shown side by side again.
    by_bar.update_in(cx, |diff, window, cx| diff.set_inline(true, window, cx));
    activate(&workbench, 0, cx);
    click(cx);
    assert_eq!(diff(&workbench, cx), by_bar);
    assert!(!by_bar.read_with(cx, |diff, _| diff.is_inline()));
}

#[gpui::test]
fn a_tab_opened_through_a_symlink_shares_a_diff_only_buffer(cx: &mut TestAppContext) {
    let dir = fixture();
    std::os::unix::fs::symlink(dir.path().join("a.rs"), dir.path().join("alias.rs")).unwrap();
    let (workbench, mut cx) = window(cx, dir.path(), None);
    let cx = &mut cx;
    let [_, right] = sides(&diff_only(&workbench, cx), cx);
    focus(&right, cx);
    keys("g g i x escape", cx);

    // Typed in the diff: the open file's spelling is kept.
    ex("e alias.rs", cx);
    let tab = tab_editor(&workbench, cx).expect("opened in a tab");
    assert!(shares_buffer(&tab, &right, cx));

    // Opened by the other spelling in another pane.
    dispatch("pane.split_right", cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let alias = dir.path().join("alias.rs");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(alias, window, cx)));
    draw(cx);
    let other = tab_editor(&workbench, cx).expect("opened in a tab");
    assert!(shares_buffer(&other, &right, cx));
    assert_eq!(text(&other, cx), format!("x{}", working()));
}

#[gpui::test]
fn a_tab_opened_from_an_index_diff_keeps_its_own_comparison(cx: &mut TestAppContext) {
    // The staged and unstaged edits on the same row.
    let dir = repo(&[("a.rs", &head())]);
    fs::write(dir.path().join("a.rs"), index()).unwrap();
    git(dir.path(), &["add", "a.rs"]);
    let unstaged = rows(&[(2, "unstaged 2")]);
    fs::write(dir.path().join("a.rs"), &unstaged).unwrap();
    let (workbench, mut cx) = window(cx, dir.path(), None);
    let cx = &mut cx;
    let by_head = diff_only(&workbench, cx);
    let [_, right] = sides(&by_head, cx);
    focus(&right, cx);
    ex("Gdiffsplit", cx);
    let by_index = diff(&workbench, cx);
    let [_, index_right] = sides(&by_index, cx);
    ex("e a.rs", cx);
    let tab = tab_editor(&workbench, cx).expect("opened in a tab");
    // Its change against HEAD, which git gives it.
    let changed = tab.read_with(cx, |tab, _| {
        (tab.model().row_changes().iter())
            .map(|change| (change.rows.start, change.rows.end))
            .collect::<Vec<_>>()
    });
    assert_eq!(changed, [(2, 3)]);

    // `do` in the Index diff still takes the index's row.
    let ix = workbench.read_with(cx, |workbench, cx| {
        let tabs = workbench.pane().read(cx).tabs();
        (tabs.iter())
            .position(|tab| tab.item().downcast_ref::<DiffView>() == Some(&by_index))
            .unwrap()
    });
    activate(&workbench, ix, cx);
    focus(&index_right, cx);
    keys("3 shift-g", cx);
    keys("d o", cx);
    assert_eq!(text(&index_right, cx), index());
}

#[gpui::test]
fn a_superseded_or_closed_rebuild_lands_nowhere(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), None);
    let cx = &mut cx;
    let shown = diff_only(&workbench, cx);
    let path = dir.path().join("a.rs");
    fs::remove_file(&path).unwrap();
    let rebuild = |read: u64| super::Request {
        path: path.clone(),
        base: super::Base::Head,
        deleted: Some(true),
        then: None,
        op: super::clicked(GitCommand::DiffHead),
        replaces: Some((shown.downgrade(), read)),
        asked: false,
        live: false,
    };

    // A later read starts before the rebuild lands.
    let first = cx.update(|_, cx| super::start_read(&shown.downgrade(), cx).unwrap());
    let request = rebuild(first);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.request_git_diff(request, window, cx);
        super::start_read(&shown.downgrade(), cx);
    });
    draw(cx);
    assert_eq!(diff(&workbench, cx), shown);

    // The tab closes before it lands.
    let read = cx.update(|_, cx| super::start_read(&shown.downgrade(), cx).unwrap());
    let request = rebuild(read);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.request_git_diff(request, window, cx);
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| {
            pane.take_item(0, window, cx);
        });
    });
    draw(cx);
    assert_eq!(tab_count(&workbench, cx), 0);
}

#[gpui::test]
fn a_dirty_rebuild_keeps_its_working_side_and_takes_the_git_text(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), None);
    let cx = &mut cx;
    let shown = diff_only(&workbench, cx);
    let [left, right] = sides(&shown, cx);
    // HEAD moves.
    let path = dir.path().join("a.rs");
    fs::write(&path, "new head\n").unwrap();
    git(dir.path(), &["commit", "-q", "-am", "move"]);
    fs::write(&path, working()).unwrap();

    let read = cx.update(|_, cx| super::start_read(&shown.downgrade(), cx).unwrap());
    let request = super::Request {
        path,
        base: super::Base::Head,
        deleted: Some(true),
        then: None,
        op: super::clicked(GitCommand::DiffHead),
        replaces: Some((shown.downgrade(), read)),
        asked: false,
        live: false,
    };
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.request_git_diff(request, window, cx);
        // An edit while it reads.
        right.update(cx, |right, _| right.model_mut().insert("x"));
    });
    draw(cx);
    assert_eq!(diff(&workbench, cx), shown);
    assert_eq!(text(&left, cx), "new head\n");
}

#[gpui::test]
fn git_text_too_large_to_edit_is_not_read(cx: &mut TestAppContext) {
    let big = "x".repeat(200) + "\n";
    let dir = repo(&[("big.rs", &big)]);
    fs::remove_file(dir.path().join("big.rs")).unwrap();
    let (workbench, mut cx) = window(cx, dir.path(), None);
    let cx = &mut cx;
    crate::loader::test_support::lower_threshold(dir.path(), 64, false);
    dispatch("git.show_changes", cx);
    keys("down", cx);
    keys("enter", cx);
    assert_eq!(toasts(&workbench, cx), ["Too large to compare: big.rs"]);
    assert_eq!(tab_count(&workbench, cx), 0);
}

#[gpui::test]
fn a_file_opened_after_a_diff_was_asked_for_keeps_focus(cx: &mut TestAppContext) {
    let dir = fixture();
    fs::write(dir.path().join("e.rs"), "e\n").unwrap();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let open = |file: &str, cx: &mut VisualTestContext| {
        let path = dir.path().join(file);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    };
    let shown_path = |cx: &mut VisualTestContext| {
        tab_editor(&workbench, cx).and_then(|editor| {
            editor.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned))
        })
    };

    send("git.diff_index", cx);
    open("b.rs", cx);
    draw(cx);
    assert_eq!(shown_path(cx), Some(dir.path().join("b.rs")));
    assert_eq!(tab_count(&workbench, cx), 3, "the diff opened all the same");

    activate(&workbench, 0, cx);
    workbench.update_in(cx, |workbench, window, cx| {
        let op = GitOp {
            command: GitCommand::CompareFile,
            args: "b.rs".to_owned(),
            range: None,
            editor: workbench.active_editor(cx).cloned(),
            typed: false,
        };
        workbench.run_git_op(op, window, cx);
    });
    open("e.rs", cx);
    draw(cx);
    assert_eq!(shown_path(cx), Some(dir.path().join("e.rs")));
    assert_eq!(tab_count(&workbench, cx), 5);
}

#[gpui::test]
fn git_text_with_crlf_or_cr_line_endings(cx: &mut TestAppContext) {
    let dir = repo(&[("a.rs", "one\r\ntwo\r\n"), ("c.rs", "x\ry\r")]);
    fs::write(dir.path().join("a.rs"), "one\r\nthree\r\n").unwrap();
    fs::write(dir.path().join("c.rs"), "x\rz\r").unwrap();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    for line in ["Gdiffsplit", "Gdiffsplit HEAD"] {
        activate(&workbench, 0, cx);
        ex(line, cx);
        let [left, right] = sides(&diff(&workbench, cx), cx);
        assert_eq!(text(&left, cx), "one\ntwo\n", ":{line}");
        assert_eq!(text(&right, cx), "one\nthree\n", ":{line}");
    }
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = dir.path().join("c.rs");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    for line in ["Gdiffsplit", "Gdiffsplit HEAD"] {
        ex(line, cx);
        let [left, _] = sides(&diff(&workbench, cx), cx);
        assert_eq!(text(&left, cx), "x\ny\n", ":{line}");
        let ix = workbench.read_with(cx, |workbench, cx| {
            let tabs = workbench.pane().read(cx).tabs();
            (tabs.iter()).position(|tab| tab.path.as_deref() == Some(&dir.path().join("c.rs")))
        });
        activate(&workbench, ix.unwrap(), cx);
    }
}

#[gpui::test]
fn a_staged_deletion_of_a_file_still_there_shows_it(cx: &mut TestAppContext) {
    let dir = fixture();
    git(dir.path(), &["rm", "-q", "-f", "--cached", "a.rs"]);
    let (workbench, mut cx) = window(cx, dir.path(), None);
    let cx = &mut cx;
    // Staged, HEAD's text against none in the index.
    dispatch("git.show_changes", cx);
    keys("down", cx);
    keys("enter", cx);
    let staged = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), "a.rs (HEAD ↔ Index)");
    assert_eq!(texts(&staged, cx), [head(), String::new()]);
    assert_eq!(read_only(&staged, cx), [true, true]);
    // And untracked, the file against none in the index.
    dispatch("git.show_changes", cx);
    keys("down", cx);
    keys("enter", cx);
    let shown = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), "a.rs (Index ↔ Working Tree)");
    assert_eq!(texts(&shown, cx), [String::new(), working()]);
    assert_eq!(read_only(&shown, cx), [true, false]);
}

#[gpui::test]
fn a_working_sides_change_bar_opens_head(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    ex("Gdiffsplit", cx);
    let [_, right] = sides(&diff(&workbench, cx), cx);
    cx.update(|_, cx| {
        right.update(cx, |_, cx| {
            cx.emit(EditorViewEvent::ChangeBarClicked { rows: 10..11 })
        })
    });
    draw(cx);
    let by_head = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), "a.rs (HEAD ↔ Working Tree)");
    let [_, head_right] = sides(&by_head, cx);
    assert_eq!(caret_row(&head_right, cx), 10);
}

#[gpui::test]
fn put_takes_exactly_the_hunk_picked(cx: &mut TestAppContext) {
    let dir = repo(&[("a.rs", "a\nremoved1\nb\nremoved2\nc\n")]);
    fs::write(dir.path().join("a.rs"), "a\nb\nc\n").unwrap();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    ex("Gdiffsplit HEAD", cx);
    let [left, right] = sides(&diff(&workbench, cx), cx);
    focus(&left, cx);
    keys("2 shift-g", cx);
    keys("d p", cx);
    assert_eq!(text(&right, cx), "a\nremoved1\nb\nc\n");
    focus(&right, cx);
    keys("u", cx);
    assert_eq!(text(&right, cx), "a\nb\nc\n", "one undo step");
}

#[gpui::test]
fn an_editors_buffer_is_compared_whether_or_not_its_file_is_there(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx).unwrap();

    // A named file not saved yet.
    ex("e new.rs", cx);
    keys("i hello escape", cx);
    for line in ["Gdiffsplit", "Gdiffsplit HEAD"] {
        ex(line, cx);
        let shown = diff(&workbench, cx);
        assert_eq!(
            texts(&shown, cx),
            [String::new(), "hello".to_owned()],
            ":{line}"
        );
        assert_eq!(read_only(&shown, cx), [true, false], ":{line}");
        let ix = workbench.read_with(cx, |workbench, cx| {
            let tabs = workbench.pane().read(cx).tabs();
            (tabs.iter()).position(|tab| tab.path.as_deref() == Some(&dir.path().join("new.rs")))
        });
        activate(&workbench, ix.unwrap(), cx);
    }

    // A file deleted from disk with unsaved changes in its tab.
    activate(&workbench, 0, cx);
    keys("g g i x escape", cx);
    fs::remove_file(dir.path().join("a.rs")).unwrap();
    for line in ["Gdiffsplit", "Gdiffsplit HEAD"] {
        activate(&workbench, 0, cx);
        ex(line, cx);
        let [_, right] = sides(&diff(&workbench, cx), cx);
        assert!(shares_buffer(&right, &tab, cx), ":{line}");
        assert_eq!(text(&right, cx), format!("x{}", working()), ":{line}");
    }
}

#[gpui::test]
fn next_change_counts_afresh_once_the_changes_move(cx: &mut TestAppContext) {
    let two = |changed: &[(usize, &str)]| -> String {
        (0..20)
            .map(|row| match changed.iter().find(|(at, _)| *at == row) {
                Some((_, text)) => format!("{text}\n"),
                None => format!("{row:02}\n"),
            })
            .collect()
    };
    let dir = repo(&[("a.rs", &two(&[]))]);
    fs::write(dir.path().join("a.rs"), two(&[(2, "x2"), (10, "xx")])).unwrap();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx).unwrap();
    dispatch("git.next_change", cx);
    dispatch("git.next_change", cx);
    assert_eq!(caret_row(&tab, cx), 10);
    // Row 2 back as it was and row 15 changed, the caret's offset kept.
    tab.update(cx, |tab, cx| {
        let edits = [(6..8, "02"), (45..47, "xx")];
        tab.model_mut()
            .buffer_mut()
            .edit(edits, fe_buffer::Origin::User);
        cx.notify();
    });
    draw(cx);
    dispatch("git.next_change", cx);
    assert_eq!(caret_row(&tab, cx), 15);
    assert!(toasts(&workbench, cx).is_empty());
}

#[gpui::test]
fn a_diffs_source_keeps_up_and_goes_as_it_closes(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("b.rs"));
    let cx = &mut cx;
    let [_, right] = sides(&diff_only(&workbench, cx), cx);
    focus(&right, cx);
    keys("g g i x escape", cx);
    let source = cx.update(|_, cx| {
        let views = &super::state(cx).views;
        views[0]
            .source
            .as_ref()
            .map(|source| source.text().to_string())
    });
    assert_eq!(source, Some(format!("x{}", working())));
    ex("q!", cx);
    let views = cx.update(|_, cx| super::state(cx).views.len());
    assert_eq!(views, 0);
}

#[gpui::test]
fn a_change_bar_click_reveals_in_the_diff_that_replaces_an_old_one(cx: &mut TestAppContext) {
    let dir = fixture();
    fs::remove_file(dir.path().join("a.rs")).unwrap();
    let (workbench, mut cx) = window(cx, dir.path(), None);
    let cx = &mut cx;
    cx.simulate_resize(size(px(600.), px(600.)));
    draw(cx);
    open_head_diff(&workbench, "a.rs", cx);
    let deleted = diff(&workbench, cx);
    assert_eq!(texts(&deleted, cx), [head(), String::new()]);

    fs::write(dir.path().join("a.rs"), working()).unwrap();
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = dir.path().join("a.rs");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let tab = tab_editor(&workbench, cx).unwrap();
    cx.update(|_, cx| {
        tab.update(cx, |_, cx| {
            cx.emit(EditorViewEvent::ChangeBarClicked { rows: 9..12 })
        })
    });
    draw(cx);
    let rebuilt = diff(&workbench, cx);
    assert_ne!(rebuilt, deleted);
    assert_eq!(texts(&rebuilt, cx), [head(), working()]);
    let [_, right] = sides(&rebuilt, cx);
    assert_eq!(caret_row(&right, cx), 9);
    assert!(!rebuilt.read_with(cx, |diff, _| diff.is_inline()));
}

#[gpui::test]
fn put_keeps_the_separators_at_the_end(cx: &mut TestAppContext) {
    // HEAD's text, the file's, the keys to the left side's row, and the
    // file's text after `dp` there.
    let cases = [
        ("a\nremoved", "a", "2 shift-g", "a\nremoved"),
        ("a", "a\nadded", "g g", "a"),
        ("a\n", "a", "shift-g", "a\n"),
        ("a", "a\n", "g g", "a"),
    ];
    for (head, file, to_row, put) in cases {
        let dir = repo(&[("a.rs", head)]);
        fs::write(dir.path().join("a.rs"), file).unwrap();
        let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
        let cx = &mut cx;
        ex("Gdiffsplit HEAD", cx);
        let [left, right] = sides(&diff(&workbench, cx), cx);
        focus(&left, cx);
        keys(to_row, cx);
        keys("d p", cx);
        assert_eq!(text(&right, cx), put, "{head:?} into {file:?}");
        focus(&right, cx);
        keys("u", cx);
        assert_eq!(text(&right, cx), file, "{head:?} into {file:?}, undone");
    }
}

#[gpui::test]
fn an_editor_asking_again_rebuilds_a_diff_of_its_deleted_file(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx).unwrap();
    for line in ["Gdiffsplit", "Gdiffsplit HEAD"] {
        activate(&workbench, 0, cx);
        ex(line, cx);
    }
    // Deleted with the tab clean: both compare with nothing.
    fs::remove_file(dir.path().join("a.rs")).unwrap();
    dispatch("git.refresh", cx);
    assert_eq!(read_only(&diff(&workbench, cx), cx), [true, true]);

    activate(&workbench, 0, cx);
    keys("g g i x escape", cx);
    for line in ["Gdiffsplit", "Gdiffsplit HEAD"] {
        activate(&workbench, 0, cx);
        ex(line, cx);
        let shown = diff(&workbench, cx);
        let [_, right] = sides(&shown, cx);
        assert_eq!(read_only(&shown, cx), [true, false], ":{line}");
        assert!(shares_buffer(&right, &tab, cx), ":{line}");
        assert_eq!(text(&right, cx), format!("x{}", working()), ":{line}");
    }
    assert_eq!(tab_count(&workbench, cx), 3);
}

#[gpui::test]
fn closing_a_window_forgets_its_diffs(cx: &mut TestAppContext) {
    let dir = fixture();
    let other = fixture();
    let (workbench, mut first) = window(cx, dir.path(), Some("a.rs"));
    let (_, _second) = window(cx, other.path(), Some("a.rs"));
    ex("Gdiffsplit HEAD", &mut first);
    assert!(active_diff(&workbench, &mut first).is_some());
    assert_eq!(cx.update(|cx| super::state(cx).views.len()), 1);
    drop(workbench);
    first.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| super::state(cx).views.len()), 0);
}

#[gpui::test]
fn the_last_change_bar_click_wins_the_caret(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx).unwrap();
    cx.update(|_, cx| {
        tab.update(cx, |_, cx| {
            cx.emit(EditorViewEvent::ChangeBarClicked { rows: 9..12 });
            cx.emit(EditorViewEvent::ChangeBarClicked { rows: 15..16 });
        })
    });
    draw(cx);
    let [_, right] = sides(&diff(&workbench, cx), cx);
    assert_eq!(caret_row(&right, cx), 15);
    assert_eq!(tab_count(&workbench, cx), 2);
}
