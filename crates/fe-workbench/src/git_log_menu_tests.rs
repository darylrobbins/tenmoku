//! The Graph's right-click menu and its commands, on repositories built
//! with the `git` CLI in temporary directories.

use std::fs;

use fe_command::{CommandId, Value};
use fe_gpui::{DiffView, EditorView};
use gpui::{Entity, Focusable as _, Modifiers, MouseButton, TestAppContext, VisualTestContext};
use kiln_shell::{
    Picker,
    command_palette::{COMMAND_PALETTE, CommandPalette},
};

use super::super::git_log_open_tests::{
    draw, fixture, focus_panel, lines, panel, press, row_center, row_of, selected, tab_count,
    title, window,
};
use super::{FILE_ITEMS, MenuItem};
use crate::Workbench;

/// The panel's commands, each a menu item's.
const COMMANDS: [&str; 5] = [
    "git_log.copy_commit_hash",
    "git_log.copy_subject",
    "git_log.open_changes",
    "git_log.open_file_at_revision",
    "git_log.compare_with_working_tree",
];

fn short(id: &str) -> &str {
    &id[..7]
}

fn id_of<'a>(ids: &'a [(&str, String)], summary: &str) -> &'a str {
    &ids.iter().find(|(name, _)| *name == summary).unwrap().1
}

/// Right-clicks row `ix` of the list.
fn right_click(workbench: &Entity<Workbench>, ix: usize, cx: &mut VisualTestContext) {
    let at = row_center(&panel(workbench, cx), ix, cx);
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    draw(cx);
}

/// The open menu's items, by title.
fn menu(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Vec<&'static str>> {
    let items = panel(workbench, cx).read_with(cx, |panel, _| panel.menu_items());
    items.map(|items| items.iter().map(|item| item.title()).collect())
}

/// Clicks the open menu's item titled `title`, as drawn.
fn choose(title: &str, cx: &mut VisualTestContext) {
    let bounds = (cx.debug_bounds(Box::leak(format!("menu-item:{title}").into_boxed_str())))
        .unwrap_or_else(|| panic!("{title} is drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard()?.text())
}

/// Expands the commit `summary` and gives the list's rows.
fn expand(workbench: &Entity<Workbench>, summary: &str, cx: &mut VisualTestContext) -> Vec<String> {
    let ix = row_of(&lines(workbench, cx), summary);
    let panel = panel(workbench, cx);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.open_row(ix, window, cx)));
    draw(cx);
    lines(workbench, cx)
}

/// The row of `file`, `M a.rs` say, under the commit `summary`.
fn file_row(rows: &[String], summary: &str, file: &str) -> usize {
    let commit = row_of(rows, summary);
    commit
        + (rows[commit..].iter())
            .position(|row| row == file)
            .unwrap_or_else(|| panic!("no {file} under {summary}: {rows:?}"))
}

/// Right-clicks `file` under the commit `summary` and chooses `item`.
fn file_item(
    workbench: &Entity<Workbench>,
    summary: &str,
    file: &str,
    item: MenuItem,
    cx: &mut VisualTestContext,
) {
    let row = file_row(&lines(workbench, cx), summary, file);
    right_click(workbench, row, cx);
    choose(item.title(), cx);
}

fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| {
            let pane = workbench.pane().read(cx);
            pane.active_item()?.downcast_ref::<EditorView>().cloned()
        })
        .expect("an editor is the active tab")
}

fn active_diff(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<DiffView> {
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
            (
                view.model().buffer().text().to_string(),
                view.is_read_only(),
            )
        })
    })
}

#[gpui::test]
fn commit_menu(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let all = lines(&workbench, cx);
    let edit = row_of(&all, "edit");

    // Copy Commit Hash and Copy Subject; nothing that rewrites history.
    right_click(&workbench, edit, cx);
    assert_eq!(
        menu(&workbench, cx).unwrap(),
        ["Copy Commit Hash", "Copy Subject"]
    );
    assert_eq!(selected(&workbench, cx), Some(edit));
    assert_eq!(lines(&workbench, cx), all, "a right click expands nothing");
    choose("Copy Commit Hash", cx);
    assert_eq!(clipboard(cx).as_deref(), Some(id_of(&ids, "edit")));
    assert_eq!(menu(&workbench, cx), None, "choosing closes the menu");

    let rename = row_of(&all, "rename");
    right_click(&workbench, rename, cx);
    choose("Copy Subject", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("rename"));
    assert_eq!(selected(&workbench, cx), Some(rename));

    // A merge's subject, its message's first line.
    right_click(&workbench, row_of(&all, "merge"), cx);
    choose("Copy Commit Hash", cx);
    assert_eq!(clipboard(cx).as_deref(), Some(id_of(&ids, "merge")));

    // Escape closes it, and so does a click elsewhere.
    right_click(&workbench, edit, cx);
    assert!(menu(&workbench, cx).is_some());
    press("escape", cx);
    assert_eq!(menu(&workbench, cx), None);
    right_click(&workbench, edit, cx);
    let elsewhere = row_center(&panel(&workbench, cx), rename, cx);
    cx.simulate_mouse_down(elsewhere, MouseButton::Left, Modifiers::none());
    draw(cx);
    assert_eq!(menu(&workbench, cx), None);
    assert_eq!(tab_count(&workbench, cx), 0, "nothing opened");
}

#[gpui::test]
fn file_menu(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let root = &dir.path().canonicalize().unwrap();
    fs::write(root.join("n.rs"), "newer\n").unwrap();
    let (workbench, mut cx) = window(cx, root);
    let cx = &mut cx;
    let edit = short(id_of(&ids, "edit")).to_owned();
    let rows = expand(&workbench, "edit", cx);
    let a = file_row(&rows, "edit", "M a.rs");

    right_click(&workbench, a, cx);
    let expected: Vec<_> = FILE_ITEMS.iter().map(|item| item.title()).collect();
    assert_eq!(
        expected,
        [
            "Open Changes",
            "Open File at This Revision",
            "Compare with Working Tree"
        ]
    );
    assert_eq!(menu(&workbench, cx).unwrap(), expected);
    assert_eq!(selected(&workbench, cx), Some(a));

    // Open Changes: the change at that commit, as a click opens it.
    choose("Open Changes", cx);
    assert_eq!(title(&workbench, cx), format!("a.rs ({edit}^ ↔ {edit})"));
    let change = active_diff(&workbench, cx);
    assert_eq!(
        sides(&change, cx),
        [("one\n".into(), true), ("two\n".into(), true)]
    );

    // Open File at This Revision: a read-only editor on its text there.
    file_item(
        &workbench,
        "edit",
        "M a.rs",
        MenuItem::OpenFileAtRevision,
        cx,
    );
    assert_eq!(title(&workbench, cx), format!("a.rs @ {edit}"));
    let editor = active_editor(&workbench, cx);
    let (text, read_only, path, language) = editor.read_with(cx, |editor, _| {
        (
            editor.model().buffer().text().to_string(),
            editor.is_read_only(),
            editor.model().path().map(ToOwned::to_owned),
            editor.syntax().map(|syntax| syntax.language().name()),
        )
    });
    assert_eq!((text.as_str(), read_only, path), ("two\n", true, None));
    assert_eq!(language, Some("rust"));
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    let tabs = tab_count(&workbench, cx);

    // Asked again, the same tab shows.
    file_item(&workbench, "edit", "M a.rs", MenuItem::OpenChanges, cx);
    file_item(
        &workbench,
        "edit",
        "M a.rs",
        MenuItem::OpenFileAtRevision,
        cx,
    );
    assert_eq!(tab_count(&workbench, cx), tabs);
    assert_eq!(active_editor(&workbench, cx), editor);

    // Compare with Working Tree: its text there against the file's
    // working side, which edits the file.
    file_item(
        &workbench,
        "edit",
        "A n.rs",
        MenuItem::CompareWithWorkingTree,
        cx,
    );
    assert_eq!(
        title(&workbench, cx),
        format!("n.rs ({edit} ↔ Working Tree)")
    );
    let compare = active_diff(&workbench, cx);
    assert_eq!(
        sides(&compare, cx),
        [("new\n".into(), true), ("newer\n".into(), false)]
    );
    let right = compare.read_with(cx, |diff, _| diff.right().clone());
    let path = right.read_with(cx, |right, _| right.model().path().map(ToOwned::to_owned));
    assert_eq!(path, Some(root.join("n.rs")));
    let tabs = tab_count(&workbench, cx);
    file_item(
        &workbench,
        "edit",
        "A n.rs",
        MenuItem::CompareWithWorkingTree,
        cx,
    );
    assert_eq!(tab_count(&workbench, cx), tabs, "asked again, the same tab");
    assert_eq!(active_diff(&workbench, cx), compare);
}

#[gpui::test]
fn edges(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let root = dir.path();
    // d.rs, deleted by `edit`, is back in the working tree; a.rs, renamed
    // by `rename`, isn't.
    fs::write(root.join("d.rs"), "back\n").unwrap();
    let (workbench, mut cx) = window(cx, root);
    let cx = &mut cx;
    let (edit, init) = (short(id_of(&ids, "edit")), short(id_of(&ids, "init")));
    expand(&workbench, "edit", cx);
    expand(&workbench, "init", cx);

    // A deleted file compares nothing with the working tree.
    file_item(
        &workbench,
        "edit",
        "D d.rs",
        MenuItem::CompareWithWorkingTree,
        cx,
    );
    assert_eq!(
        title(&workbench, cx),
        format!("d.rs ({edit} ↔ Working Tree)")
    );
    let compare = active_diff(&workbench, cx);
    assert_eq!(
        sides(&compare, cx),
        [(String::new(), true), ("back\n".into(), false)]
    );
    // And has no text there to open.
    let tabs = tab_count(&workbench, cx);
    file_item(
        &workbench,
        "edit",
        "D d.rs",
        MenuItem::OpenFileAtRevision,
        cx,
    );
    assert_eq!(tab_count(&workbench, cx), tabs);

    // A file gone from the working tree: the right side is empty.
    file_item(
        &workbench,
        "init",
        "A a.rs",
        MenuItem::CompareWithWorkingTree,
        cx,
    );
    assert_eq!(
        title(&workbench, cx),
        format!("a.rs ({init} ↔ Working Tree)")
    );
    let compare = active_diff(&workbench, cx);
    assert_eq!(
        sides(&compare, cx),
        [("one\n".into(), true), (String::new(), true)]
    );

    // The menu's items are commands of the catalog.
    let typed = kiln::recorded_catalog("fe_workbench");
    let catalog = crate::command_catalog();
    for id in COMMANDS {
        let id = CommandId::parse(id).unwrap();
        assert_eq!(typed.check(&id, &Value::Null), Ok(()), "{id}");
        assert_eq!(catalog.check(&id, &Value::Null), Ok(()), "{id}");
    }

    // The palette lists them while the panel has focus, and runs them on
    // the selected row.
    focus_panel(&workbench, cx);
    let listed = palette(&workbench, cx);
    for id in COMMANDS {
        assert!(listed.iter().any(|listed| listed == id), "{id}: {listed:?}");
    }
    let rows = lines(&workbench, cx);
    let rename = row_of(&rows, "rename");
    panel(&workbench, cx).update(cx, |panel, cx| panel.select(rename, cx));
    focus_panel(&workbench, cx);
    cx.simulate_keystrokes("cmd-shift-p");
    cx.simulate_input("Copy Commit Subject");
    draw(cx);
    press("enter", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("rename"));
    run("git_log.copy_commit_hash", cx);
    assert_eq!(clipboard(cx).as_deref(), Some(id_of(&ids, "rename")));
    // A file command on a commit row does nothing.
    let tabs = tab_count(&workbench, cx);
    run("git_log.open_file_at_revision", cx);
    assert_eq!(tab_count(&workbench, cx), tabs);
    let d = file_row(&rows, "init", "A d.rs");
    panel(&workbench, cx).update(cx, |panel, cx| panel.select(d, cx));
    run("git_log.open_file_at_revision", cx);
    assert_eq!(title(&workbench, cx), format!("d.rs @ {init}"));
}

/// Runs typed command `id` where focus is, as a key bound to it does.
fn run(id: &str, cx: &mut VisualTestContext) {
    let id = CommandId::parse(id).unwrap();
    let command = cx.read(|cx| kiln::platform::command_actions::typed_command(&id, cx));
    let action = command
        .expect("a typed command")
        .build(&Value::Null)
        .unwrap();
    cx.update(|window, cx| window.dispatch_action(action, cx));
    draw(cx);
}

/// The command IDs the palette lists, opened where focus is now, then
/// closed.
fn palette(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    let picker: Entity<Picker<CommandPalette>> = workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        assert_eq!(layer.modal(), Some(COMMAND_PALETTE));
        layer.modal_view().unwrap().clone().downcast().unwrap()
    });
    let ids = picker.read_with(cx, |picker, _| {
        (picker.delegate.matches().iter())
            .map(|entry| entry.id.to_string())
            .collect()
    });
    cx.simulate_keystrokes("escape");
    draw(cx);
    ids
}

/// The text of `editor`'s buffer and whether it's read-only.
fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (String, bool) {
    editor.read_with(cx, |editor, _| {
        (
            editor.model().buffer().text().to_string(),
            editor.is_read_only(),
        )
    })
}

// A split of a revision's tab is read-only too, with its title and
// language: typing in it changes neither.
#[gpui::test]
fn a_split_revision_tab_stays_read_only(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let edit = short(id_of(&ids, "edit")).to_owned();
    expand(&workbench, "edit", cx);
    file_item(
        &workbench,
        "edit",
        "M a.rs",
        MenuItem::OpenFileAtRevision,
        cx,
    );
    let editor = active_editor(&workbench, cx);
    run("pane.split_right", cx);
    let split = active_editor(&workbench, cx);
    assert_ne!(split, editor);
    assert_eq!(title(&workbench, cx), format!("a.rs @ {edit}"));
    let language = split.read_with(cx, |split, _| {
        split.syntax().map(|syntax| syntax.language().name())
    });
    assert_eq!(language, Some("rust"));
    assert!(cx.update(|window, cx| split.focus_handle(cx).is_focused(window)));
    press("x i", cx);
    cx.simulate_input("typed");
    press("escape", cx);
    assert_eq!(text(&split, cx), ("two\n".into(), true));
    assert_eq!(text(&editor, cx), ("two\n".into(), true));
}

/// Opens a.rs then n.rs as `edit` has them, in a pane with b.rs open if
/// `populated`, their reads landing in `order` (the indices of the held
/// reads left, released in turn): the later one asked for is shown and
/// focused, whichever lands first; the earlier waits in its tab.
fn land_in(order: [usize; 2], populated: bool, cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root);
    let cx = &mut cx;
    let edit = short(id_of(&ids, "edit")).to_owned();
    if populated {
        let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
        let path = root.join("b.rs");
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    let tabs = tab_count(&workbench, cx);
    expand(&workbench, "edit", cx);
    cx.update(|_, cx| Workbench::hold_git_work(cx));
    file_item(
        &workbench,
        "edit",
        "M a.rs",
        MenuItem::OpenFileAtRevision,
        cx,
    );
    file_item(
        &workbench,
        "edit",
        "A n.rs",
        MenuItem::OpenFileAtRevision,
        cx,
    );
    assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 2);
    assert_eq!(tab_count(&workbench, cx), tabs);
    for ix in order {
        cx.update(|_, cx| Workbench::release_git_work(ix, cx));
        draw(cx);
    }
    assert_eq!(tab_count(&workbench, cx), tabs + 2, "a.rs waits in its tab");
    assert_eq!(title(&workbench, cx), format!("n.rs @ {edit}"));
    let shown = active_editor(&workbench, cx);
    assert!(cx.update(|window, cx| shown.focus_handle(cx).is_focused(window)));
}

#[gpui::test]
fn revision_opens_landing_in_order_show_the_last(cx: &mut TestAppContext) {
    land_in([0, 0], false, cx);
}

#[gpui::test]
fn revision_opens_landing_in_reverse_show_the_last(cx: &mut TestAppContext) {
    land_in([1, 0], false, cx);
}

#[gpui::test]
fn revision_opens_landing_in_order_in_a_pane_with_tabs_show_the_last(cx: &mut TestAppContext) {
    land_in([0, 0], true, cx);
}

#[gpui::test]
fn revision_opens_landing_in_reverse_in_a_pane_with_tabs_show_the_last(cx: &mut TestAppContext) {
    land_in([1, 0], true, cx);
}

// A comparison whose file was gone, asked for again once the file is
// back, compares with the file: a new diff in its tab.
#[gpui::test]
fn a_restored_file_rebuilds_its_empty_comparison(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root);
    let cx = &mut cx;
    let init = short(id_of(&ids, "init")).to_owned();
    expand(&workbench, "init", cx);
    file_item(
        &workbench,
        "init",
        "A a.rs",
        MenuItem::CompareWithWorkingTree,
        cx,
    );
    let gone = active_diff(&workbench, cx);
    assert_eq!(sides(&gone, cx)[1], (String::new(), true));

    fs::write(root.join("a.rs"), "restored\n").unwrap();
    file_item(
        &workbench,
        "init",
        "A a.rs",
        MenuItem::CompareWithWorkingTree,
        cx,
    );
    let back = active_diff(&workbench, cx);
    assert_ne!(back, gone);
    assert_eq!(tab_count(&workbench, cx), 1, "in the same tab");
    assert_eq!(
        title(&workbench, cx),
        format!("a.rs ({init} ↔ Working Tree)")
    );
    assert_eq!(
        sides(&back, cx),
        [("one\n".into(), true), ("restored\n".into(), false)]
    );

    // Asked again, that one shows.
    file_item(
        &workbench,
        "init",
        "A a.rs",
        MenuItem::CompareWithWorkingTree,
        cx,
    );
    assert_eq!(active_diff(&workbench, cx), back);
}

// Two commits whose ids start alike are told apart by their full ids.
#[gpui::test]
fn revisions_are_told_apart_by_full_id(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root);
    let cx = &mut cx;
    let repo = std::sync::Arc::new(fe_git::Repo::discover(root).unwrap().unwrap());
    let path = root.join("n.rs");
    let alike = "1234567".to_owned();
    for (summary, text) in [("main work", "new\n"), ("edit", "new\n"), ("init", "")] {
        let id = id_of(&ids, summary).to_owned();
        let (repo, path, alike) = (repo.clone(), path.clone(), alike.clone());
        cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                let op = super::clicked();
                workbench.compare_revision(repo, (id, alike), path, op, window, cx)
            })
        });
        draw(cx);
        let compare = active_diff(&workbench, cx);
        assert_eq!(sides(&compare, cx)[0].0, text, "{summary}");
    }
    assert_eq!(tab_count(&workbench, cx), 3);
}

// An open buffer of the file is its working side even when the file can't
// be read from disk.
#[cfg(unix)]
#[gpui::test]
fn an_open_buffer_is_compared_when_its_file_cant_be_read(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;

    let (dir, ids) = fixture();
    let root = dir.path();
    let path = root.join("n.rs");
    fs::write(&path, "newer\n").unwrap();
    let (workbench, mut cx) = window(cx, root);
    let cx = &mut cx;
    let edit = short(id_of(&ids, "edit")).to_owned();
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let open = path.clone();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(open, window, cx)));
    draw(cx);
    assert_eq!(text(&active_editor(&workbench, cx), cx).0, "newer\n");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();

    expand(&workbench, "edit", cx);
    file_item(
        &workbench,
        "edit",
        "A n.rs",
        MenuItem::CompareWithWorkingTree,
        cx,
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        title(&workbench, cx),
        format!("n.rs ({edit} ↔ Working Tree)")
    );
    let compare = active_diff(&workbench, cx);
    assert_eq!(
        sides(&compare, cx),
        [("new\n".into(), true), ("newer\n".into(), false)]
    );
}

// Two paths of a commit that are one file in the working tree now, one a
// symlink to the other, compare apart: each with its own text there.
#[cfg(unix)]
#[gpui::test]
fn committed_paths_that_are_one_file_now_compare_apart(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let root = dir.path();
    fs::write(root.join("a.rs"), "now\n").unwrap();
    std::os::unix::fs::symlink(root.join("a.rs"), root.join("d.rs")).unwrap();
    let (workbench, mut cx) = window(cx, root);
    let cx = &mut cx;
    let init = short(id_of(&ids, "init")).to_owned();
    expand(&workbench, "init", cx);
    file_item(
        &workbench,
        "init",
        "A a.rs",
        MenuItem::CompareWithWorkingTree,
        cx,
    );
    let a = active_diff(&workbench, cx);
    assert_eq!(sides(&a, cx)[0].0, "one\n");
    file_item(
        &workbench,
        "init",
        "A d.rs",
        MenuItem::CompareWithWorkingTree,
        cx,
    );
    let d = active_diff(&workbench, cx);
    assert_ne!(d, a);
    assert_eq!(
        title(&workbench, cx),
        format!("d.rs ({init} ↔ Working Tree)")
    );
    assert_eq!(
        sides(&d, cx),
        [("gone soon\n".into(), true), ("now\n".into(), false)]
    );
    assert_eq!(tab_count(&workbench, cx), 2);
}
