//! The History panel, listing a file's commits from `git.file_history` and
//! `:0Gclog`, and opening a commit's change in a diff view, on
//! repositories built with the `git` CLI in temporary directories.

use std::{fs, path::Path, process::Command};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{DiffView, EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::{
    Picker,
    command_palette::{COMMAND_PALETTE, CommandPalette},
};

use super::HistoryPanel;
use crate::{DockPanel, Launch, Workbench};

/// Runs `git` in `dir`, with the developer's own config shut out, and
/// gives its output.
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
    String::from_utf8(output.stdout).unwrap()
}

const INIT: &str = "one\ntwo\nthree\n";
const EDITED_A: &str = "one\n2\nthree\n";
const EDITED_B: &str = "one\n2\nthree\nfour\n";

/// A repository where `a.rs` was committed, edited, renamed to `b.rs`,
/// then edited again: its commits' full ids, newest first.
fn fixture() -> (tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::write(root.join("a.rs"), INIT).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("a.rs"), EDITED_A).unwrap();
    git(root, &["commit", "-q", "-am", "edit a"]);
    git(root, &["mv", "a.rs", "b.rs"]);
    git(root, &["commit", "-q", "-m", "rename"]);
    fs::write(root.join("b.rs"), EDITED_B).unwrap();
    git(root, &["commit", "-q", "-am", "edit b"]);
    let ids = git(root, &["log", "--format=%H"]);
    let ids = ids.lines().map(str::to_owned).collect();
    (dir, ids)
}

/// A window of a workbench rooted at `root`, focused, with `file` open and
/// vim on.
fn window(
    cx: &mut TestAppContext,
    root: &Path,
    file: &str,
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
    let path = root.join(file);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    draw(cx);
    (workbench, vcx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs workbench command `id` as an unbound key would, leaving what it
/// starts in the background to run.
fn send(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
}

fn dispatch(id: &str, cx: &mut VisualTestContext) {
    send(id, cx);
    draw(cx);
}

/// Types `:{line}` and Enter.
fn ex(line: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor is active")
}

fn focus_on(focus: &gpui::FocusHandle, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(focus, cx));
    draw(cx);
}

fn focused(focus: &gpui::FocusHandle, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| focus.contains_focused(window, cx))
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

fn panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<HistoryPanel> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.dock().read(cx).history().cloned()).expect("the History panel was made")
    })
}

/// Whether the dock shows, the panel it shows and its title row.
fn dock(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (bool, DockPanel, String) {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        let title = dock.state().title(cx).unwrap_or_default().to_string();
        (dock.is_visible(), dock.active(), title)
    })
}

fn rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
    panel(workbench, cx).read_with(cx, |panel, _| panel.rows())
}

fn selected(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<usize> {
    panel(workbench, cx).read_with(cx, |panel, _| panel.selected())
}

fn panel_message(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    panel(workbench, cx).read_with(cx, |panel, _| panel.shown_message())
}

/// Focuses the History panel.
fn focus_panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let panel = panel(workbench, cx);
    let focus = panel.read_with(cx, |panel, cx| panel.focus_handle(cx));
    focus_on(&focus, cx);
}

fn panel_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let panel = panel(workbench, cx);
    let focus = panel.read_with(cx, |panel, cx| panel.focus_handle(cx));
    focused(&focus, cx)
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
    workbench
        .read_with(cx, |workbench, cx| {
            let pane = workbench.pane().read(cx);
            pane.active_item()?.downcast_ref::<DiffView>().cloned()
        })
        .expect("a diff view is the active tab")
}

/// A diff view's sides: each one's text and whether it's read-only.
fn sides(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> [(String, bool); 2] {
    diff.read_with(cx, |diff, cx| {
        [diff.left(), diff.right()].map(|side| {
            let view = side.read(cx);
            let text = view.model().buffer().text().to_string();
            (text, view.is_read_only())
        })
    })
}

fn diff_focused(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> bool {
    let [left, right] = diff.read_with(cx, |diff, cx| {
        [diff.left(), diff.right()].map(|side| side.focus_handle(cx))
    });
    focused(&left, cx) || focused(&right, cx)
}

fn short(id: &str) -> &str {
    &id[..7]
}

/// The fixture's rows, newest first.
fn expected(ids: &[String]) -> Vec<Vec<String>> {
    let row = |ix: usize, summary: &str| {
        vec![
            format!("{}  {summary}", short(&ids[ix])),
            "t, just now".to_owned(),
        ]
    };
    let mut renamed = row(1, "rename");
    renamed.push("renamed from a.rs".to_owned());
    vec![row(0, "edit b"), renamed, row(2, "edit a"), row(3, "init")]
}

#[gpui::test]
fn lists(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), "b.rs");
    let cx = &mut cx;
    let editor = active(&workbench, cx);

    dispatch("git.file_history", cx);
    let shown = (true, DockPanel::History, "History: b.rs".to_owned());
    assert_eq!(dock(&workbench, cx), shown);
    assert_eq!(rows(&workbench, cx), expected(&ids));
    assert_eq!(panel_message(&workbench, cx), None);
    assert!(panel_focused(&workbench, cx));
    // On the rail once it has shown.
    let on_rail = workbench.read_with(cx, |workbench, cx| {
        let state = workbench.dock().read(cx).state();
        state.on_rail(DockPanel::History.id(), cx)
    });
    assert!(on_rail);

    for line in ["0Gclog", "Gclog %", "Gclog -- %"] {
        let editor_focus = cx.update(|_, cx| editor.focus_handle(cx));
        focus_on(&editor_focus, cx);
        dispatch("dock.toggle", cx);
        assert!(!dock(&workbench, cx).0, ":{line}");
        ex(line, cx);
        assert_eq!(dock(&workbench, cx), shown, ":{line}");
        assert_eq!(rows(&workbench, cx), expected(&ids), ":{line}");
        assert!(panel_focused(&workbench, cx), ":{line}");
        assert_eq!(message(&editor, cx), None, ":{line}");
    }

    let editor_focus = cx.update(|_, cx| editor.focus_handle(cx));
    // Bare, it's the repository's log.
    focus_on(&editor_focus, cx);
    ex("Gclog", cx);
    assert_eq!(message(&editor, cx), None);
    let log = (true, DockPanel::GitLog, "Git Log: main".to_owned());
    assert_eq!(dock(&workbench, cx), log);
    for (line, answer) in [
        ("3Gclog", "Not supported yet: :3Gclog"),
        ("Gclog main", "Not supported yet: :Gclog main"),
        ("0Gclog %", "Not supported yet: :0Gclog %"),
    ] {
        focus_on(&editor_focus, cx);
        ex(line, cx);
        assert_eq!(message(&editor, cx).as_deref(), Some(answer), ":{line}");
    }

    // The palette lists it.
    focus_on(&editor_focus, cx);
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    let picker = workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        assert_eq!(layer.modal(), Some(COMMAND_PALETTE));
        let view = layer.modal_view().unwrap().clone();
        view.downcast::<Picker<CommandPalette>>().unwrap()
    });
    let listed = picker.read_with(cx, |picker, _| {
        (picker.delegate.matches().iter()).any(|entry| {
            entry.id.as_str() == "git.file_history" && entry.title == "Git: File History"
        })
    });
    assert!(listed);
}

#[gpui::test]
fn opens_change(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), "b.rs");
    let cx = &mut cx;
    dispatch("git.file_history", cx);
    assert_eq!(selected(&workbench, cx), Some(0));

    // A modifying commit: b.rs at its parent and at it.
    press("enter", cx);
    let first = diff(&workbench, cx);
    let s = short(&ids[0]);
    assert_eq!(title(&workbench, cx), format!("b.rs ({s}^ ↔ {s})"));
    let left = (EDITED_A.to_owned(), true);
    assert_eq!(sides(&first, cx), [left, (EDITED_B.to_owned(), true)]);
    assert!(diff_focused(&first, cx));
    assert_eq!(tab_count(&workbench, cx), 2);

    // The rename: a.rs at the parent.
    focus_panel(&workbench, cx);
    press("down enter", cx);
    let renamed = diff(&workbench, cx);
    let s = short(&ids[1]);
    assert_eq!(title(&workbench, cx), format!("b.rs ({s}^ ↔ {s})"));
    let texts = (EDITED_A.to_owned(), true);
    assert_eq!(sides(&renamed, cx), [texts.clone(), texts]);
    assert!(diff_focused(&renamed, cx));

    // The root commit: nothing on the left.
    focus_panel(&workbench, cx);
    press("down down enter", cx);
    let root = diff(&workbench, cx);
    let s = short(&ids[3]);
    assert_eq!(title(&workbench, cx), format!("a.rs ({s}^ ↔ {s})"));
    let right = (INIT.to_owned(), true);
    assert_eq!(sides(&root, cx), [(String::new(), true), right]);
    assert_eq!(tab_count(&workbench, cx), 4);

    // Again: the same tab, focused.
    focus_panel(&workbench, cx);
    press("up up up enter", cx);
    assert_eq!(diff(&workbench, cx), first);
    assert!(diff_focused(&first, cx));
    assert_eq!(tab_count(&workbench, cx), 4);
}

/// Opens row `ix`'s change, leaving its read in the background.
fn open_row(workbench: &Entity<Workbench>, ix: usize, cx: &mut VisualTestContext) {
    let panel = panel(workbench, cx);
    panel.update_in(cx, |panel, window, cx| panel.open(ix, window, cx));
}

/// The titles of `pane`'s tabs.
fn titles(pane: &Entity<crate::Pane>, cx: &mut VisualTestContext) -> Vec<String> {
    pane.read_with(cx, |pane, cx| {
        (pane.tabs().iter())
            .map(|tab| tab.item().tab_title(cx).to_string())
            .collect()
    })
}

#[gpui::test]
fn opens_late(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), "b.rs");
    let cx = &mut cx;
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let title_of = |ix: usize, name: &str| {
        let s = short(&ids[ix]);
        format!("{name} ({s}^ ↔ {s})")
    };
    dispatch("git.file_history", cx);
    press("enter", cx);
    let first = diff(&workbench, cx);

    // One still being read when an open one is shown again: it waits in
    // its tab.
    open_row(&workbench, 2, cx);
    open_row(&workbench, 0, cx);
    draw(cx);
    assert_eq!(diff(&workbench, cx), first);
    assert!(diff_focused(&first, cx));
    let tabs = ["b.rs".to_owned(), title_of(0, "b.rs"), title_of(2, "a.rs")];
    assert_eq!(titles(&pane, cx), tabs);

    // One still being read when another tab is shown.
    open_row(&workbench, 3, cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    draw(cx);
    assert_eq!(title(&workbench, cx), "b.rs");
    assert_eq!(titles(&pane, cx).last(), Some(&title_of(3, "a.rs")));

    // One still being read when another pane is made active: it opens in
    // the pane it was asked in.
    open_row(&workbench, 1, cx);
    send("pane.split_right", cx);
    draw(cx);
    let split = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_ne!(split, pane);
    assert_eq!(title(&workbench, cx), "b.rs");
    assert_eq!(titles(&split, cx), ["b.rs"]);
    assert_eq!(titles(&pane, cx).len(), 5);
    assert_eq!(titles(&pane, cx).last(), Some(&title_of(1, "b.rs")));
    let shown = pane.read_with(cx, |pane, cx| pane.active_item().unwrap().tab_title(cx));
    assert_eq!(shown.as_ref(), "b.rs");
}

#[gpui::test]
fn keys(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), "b.rs");
    let cx = &mut cx;
    let editor = active(&workbench, cx);
    let editor_focus = cx.update(|_, cx| editor.focus_handle(cx));
    dispatch("git.file_history", cx);
    assert!(panel_focused(&workbench, cx));

    // With vim on.
    for (keystrokes, row) in [
        ("j", 1),
        ("j j j", 3),
        ("k", 2),
        ("g g", 0),
        ("shift-g", 3),
        ("g j", 3),
        // A second g only right after the first.
        ("g k", 2),
        ("g k g", 1),
        ("g g", 0),
    ] {
        press(keystrokes, cx);
        assert_eq!(selected(&workbench, cx), Some(row), "{keystrokes}");
    }
    press("q", cx);
    assert!(!dock(&workbench, cx).0);
    assert!(focused(&editor_focus, cx));
    assert_eq!(tab_count(&workbench, cx), 1);

    dispatch("git.file_history", cx);
    press("j o", cx);
    let s = short(&ids[1]);
    assert_eq!(title(&workbench, cx), format!("b.rs ({s}^ ↔ {s})"));
    assert_eq!(tab_count(&workbench, cx), 2);

    // With vim off, the letters do nothing.
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, false));
    focus_on(&editor_focus, cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    draw(cx);
    dispatch("git.file_history", cx);
    assert!(panel_focused(&workbench, cx));
    press("j shift-g o q", cx);
    assert_eq!(selected(&workbench, cx), Some(0));
    assert!(dock(&workbench, cx).0);
    assert!(panel_focused(&workbench, cx));
    assert_eq!(tab_count(&workbench, cx), 2);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.model().text().to_string()),
        EDITED_B
    );

    press("down down", cx);
    assert_eq!(selected(&workbench, cx), Some(2));
    press("up", cx);
    assert_eq!(selected(&workbench, cx), Some(1));
    // The rename's change, opened before: its tab.
    press("enter", cx);
    assert_eq!(title(&workbench, cx), format!("b.rs ({s}^ ↔ {s})"));
    assert_eq!(tab_count(&workbench, cx), 2);
    focus_panel(&workbench, cx);
    press("up enter", cx);
    let s = short(&ids[0]);
    assert_eq!(title(&workbench, cx), format!("b.rs ({s}^ ↔ {s})"));
    assert_eq!(tab_count(&workbench, cx), 3);
}

#[gpui::test]
fn edges(cx: &mut TestAppContext) {
    // Outside a repository: answered on the vim line, or in a toast.
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("c.rs"), "c\n").unwrap();
    let (workbench, mut cx) = window(cx, dir.path(), "c.rs");
    let cx = &mut cx;
    let editor = active(&workbench, cx);
    ex("0Gclog", cx);
    assert_eq!(
        message(&editor, cx).as_deref(),
        Some("Not in a git repository")
    );
    let not_repo = Some("Not in a git repository".to_owned());
    assert_eq!(panel_message(&workbench, cx), not_repo);
    dispatch("git.file_history", cx);
    assert_eq!(toasts(&workbench, cx), ["Not in a git repository"]);

    // An untracked file has no commits; the list is read in the background.
    let (repo, _) = fixture();
    fs::write(repo.path().join("c.rs"), "c\n").unwrap();
    let path = repo.path().join("c.rs");
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    send("git.file_history", cx);
    assert_eq!(panel_message(&workbench, cx).as_deref(), Some("Loading…"));
    assert_eq!(dock(&workbench, cx).2, "History: c.rs");
    draw(cx);
    let none = Some("No commits for c.rs".to_owned());
    assert_eq!(panel_message(&workbench, cx), none);
    assert_eq!(rows(&workbench, cx), Vec::<Vec<String>>::new());

    // A tracked file's commits, read in the background too.
    let path = repo.path().join("b.rs");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    send("git.file_history", cx);
    assert_eq!(panel_message(&workbench, cx).as_deref(), Some("Loading…"));
    assert!(rows(&workbench, cx).is_empty());
    draw(cx);
    assert_eq!(panel_message(&workbench, cx), None);
    assert_eq!(rows(&workbench, cx).len(), 4);
}
