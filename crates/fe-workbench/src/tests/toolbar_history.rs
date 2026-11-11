//! The title bar's back and forward buttons follow the navigation history,
//! and breadcrumb picks are jumps.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_command::Source;
use fe_gpui::EditorView;
use fe_ui::CrumbKind;
use gpui::{Bounds, Entity, Focusable as _, Modifiers, Pixels, TestAppContext, VisualTestContext};

use super::open_root;

mod goto;
use crate::{Pane, Workbench, pane_toolbar::NavButtons};

/// Where the title bar's back and forward buttons are drawn.
fn button(forward: bool, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let id = if forward {
        "title-forward"
    } else {
        "title-back"
    };
    cx.debug_bounds(format!("icon-button:{id}").leak())
        .expect("drawn")
}

const MAIN: &str = "\
mod util;

struct S;

impl S {
    fn a() {}
    fn b() {
        util::helper();
    }
}

fn main() {}
";
const UTIL: &str = "pub fn helper() {}\n";

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), MAIN).unwrap();
    fs::write(root.join("src/util.rs"), UTIL).unwrap();
    (dir, root)
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

/// A workbench showing `src/main.rs`, focused.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, root);
    workbench.update(cx, |workbench, cx| workbench.set_title_bar(true, cx));
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let pane = active(&workbench, cx);
    let path = root.join("src/main.rs");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    settle(cx);
    (workbench, cx)
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| {
        workbench
            .pane_group(cx)
            .panes()
            .into_iter()
            .cloned()
            .collect()
    })
}

fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown")
}

/// Puts the active caret at the start of `needle`, plus `skip`; its offset.
fn caret_at(
    workbench: &Entity<Workbench>,
    needle: &str,
    skip: usize,
    cx: &mut VisualTestContext,
) -> usize {
    let editor = editor(workbench, cx);
    let offset = editor.update(cx, |editor, cx| {
        let offset = editor.model().text().to_string().find(needle).unwrap() + skip;
        editor.model_mut().set_cursor(offset);
        cx.notify();
        offset
    });
    settle(cx);
    offset
}

/// The active editor's file and caret.
fn at(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, usize) {
    let editor = editor(workbench, cx);
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        (
            model.path().unwrap().to_owned(),
            model.primary_selection().head(),
        )
    })
}

fn nav(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (bool, bool) {
    workbench.read_with(cx, |workbench, _| {
        let NavButtons { back, forward } = workbench.nav_buttons();
        (back, forward)
    })
}

fn pushes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, _| workbench.nav_pushes)
}

fn mouse_runs(workbench: &Entity<Workbench>, command: &str, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, _| {
        let runs = &workbench.commands_run;
        let of = runs.iter().filter(|run| run.command.as_str() == command);
        of.filter(|run| run.source == Source::Mouse).count()
    })
}

fn click_button(forward: bool, cx: &mut VisualTestContext) {
    let at = button(forward, cx).center();
    cx.simulate_click(at, Modifiers::none());
    settle(cx);
}

fn click_crumb(pane: &Entity<Pane>, kind: CrumbKind, label: &str, cx: &mut VisualTestContext) {
    let ix = pane.read_with(cx, |pane, _| {
        pane.drawn_crumbs()
            .iter()
            .position(|(text, crumb_kind, _)| text == label && *crumb_kind == kind)
            .unwrap_or_else(|| panic!("no crumb {label}"))
    });
    let crumb = cx.debug_bounds(format!("crumb:{ix}:{label}").leak());
    cx.simulate_click(crumb.expect("drawn").center(), Modifiers::none());
    settle(cx);
}

#[gpui::test]
fn buttons_follow_the_history(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = opened(cx, &root);
    let main = root.join("src/main.rs");
    let util = root.join("src/util.rs");
    assert_eq!(nav(&workbench, cx), (false, false));

    let origin = caret_at(&workbench, "helper", 1, cx);
    cx.simulate_keystrokes("f12");
    settle(cx);
    let target = at(&workbench, cx);
    assert_eq!(target.0, util);
    assert_eq!(nav(&workbench, cx), (true, false));

    // As ctrl-minus returns.
    click_button(false, cx);
    assert_eq!(at(&workbench, cx), (main.clone(), origin));
    assert_eq!(nav(&workbench, cx), (false, true));

    click_button(true, cx);
    assert_eq!(at(&workbench, cx), target);
    assert_eq!(nav(&workbench, cx), (true, false));

    // A new jump clears the way forward.
    click_button(false, cx);
    assert_eq!(nav(&workbench, cx), (false, true));
    cx.simulate_keystrokes("f12");
    settle(cx);
    assert_eq!(nav(&workbench, cx), (true, false));
}

#[gpui::test]
fn the_title_bar_goes_back_in_the_active_pane(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = opened(cx, &root);
    let main = root.join("src/main.rs");

    cx.simulate_keystrokes("cmd-\\");
    settle(cx);
    let [left, right] = panes(&workbench, cx).try_into().unwrap();
    assert_eq!(active(&workbench, cx), right);
    let origin = caret_at(&workbench, "helper", 1, cx);
    cx.simulate_keystrokes("f12");
    settle(cx);
    assert_eq!(nav(&workbench, cx), (true, false));

    // Back, from the title bar, goes back in the active pane.
    assert_eq!(active(&workbench, cx), right);
    click_button(false, cx);
    assert_eq!(active(&workbench, cx), right);
    assert_eq!(at(&workbench, cx), (main.clone(), origin));
    assert_eq!(nav(&workbench, cx), (false, true));
    assert_eq!(mouse_runs(&workbench, "nav.back", cx), 1);

    // With the left pane active, forward opens the target there.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.activate(left.clone(), window, cx)
    });
    settle(cx);
    assert_eq!(active(&workbench, cx), left);
    click_button(true, cx);
    assert_eq!(active(&workbench, cx), left);
    assert_eq!(at(&workbench, cx).0, root.join("src/util.rs"));
    assert_eq!(mouse_runs(&workbench, "nav.forward", cx), 1);
}

#[gpui::test]
fn crumb_picks_push_history(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = opened(cx, &root);
    let pane = active(&workbench, cx);
    let main = root.join("src/main.rs");

    // A symbol from the quick outline the symbol crumb opens: its name
    // selected.
    let in_b = caret_at(&workbench, "util::helper", 2, cx);
    assert_eq!(nav(&workbench, cx), (false, false));
    click_crumb(&pane, CrumbKind::Symbol, "b", cx);
    cx.simulate_keystrokes("a enter");
    settle(cx);
    let (_, in_a) = at(&workbench, cx);
    assert_eq!(in_a, MAIN.find("a()").unwrap() + "a".len());
    assert_eq!(nav(&workbench, cx), (true, false));
    click_button(false, cx);
    assert_eq!(at(&workbench, cx), (main.clone(), in_b));

    // Another file from the entry crumb.
    click_crumb(&pane, CrumbKind::File, "main.rs", cx);
    cx.simulate_keystrokes("u t i l enter");
    settle(cx);
    assert_eq!(at(&workbench, cx).0, root.join("src/util.rs"));
    assert_eq!(nav(&workbench, cx), (true, false));
    click_button(false, cx);
    assert_eq!(at(&workbench, cx), (main, in_b));
}

#[gpui::test]
fn button_state_is_pushed_on_change_only(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = opened(cx, &root);
    let before = pushes(&workbench, cx);
    for _ in 0..5 {
        settle(cx);
    }
    caret_at(&workbench, "fn main", 3, cx);
    assert_eq!(pushes(&workbench, cx), before, "no history change");
    caret_at(&workbench, "helper", 1, cx);
    cx.simulate_keystrokes("f12");
    settle(cx);
    assert_eq!(pushes(&workbench, cx), before + 1);
    for _ in 0..5 {
        settle(cx);
    }
    assert_eq!(pushes(&workbench, cx), before + 1);
}

#[gpui::test]
fn an_editor_jump_pushes_history(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = opened(cx, &root);
    let main = root.join("src/main.rs");

    // A click on a sticky scroll row: the caret is at the header, and the
    // editor says where it jumped from.
    let in_b = caret_at(&workbench, "util::helper", 2, cx);
    let header = caret_at(&workbench, "impl S", 0, cx);
    let editor = editor(&workbench, cx);
    editor.update(cx, |_, cx| {
        cx.emit(fe_gpui::commands::EditorViewEvent::Jumped { from: in_b })
    });
    settle(cx);
    assert_eq!(nav(&workbench, cx), (true, false));
    assert_eq!(at(&workbench, cx), (main.clone(), header));
    click_button(false, cx);
    assert_eq!(at(&workbench, cx), (main, in_b));
}

#[gpui::test]
fn go_to_bracket_is_a_jump(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = opened(cx, &root);
    let main = root.join("src/main.rs");

    // Just before the call's `(`: the bracket's partner is the `)`.
    let from = caret_at(&workbench, "util::helper", "util::helper".len(), cx);
    let to = MAIN.find("();").unwrap() + 2;
    cx.simulate_keystrokes("cmd-shift-\\");
    settle(cx);
    assert_eq!(at(&workbench, cx), (main.clone(), to));
    assert_eq!(nav(&workbench, cx), (true, false));

    click_button(false, cx);
    assert_eq!(at(&workbench, cx), (main.clone(), from));
    assert_eq!(nav(&workbench, cx), (false, true));
    click_button(true, cx);
    assert_eq!(at(&workbench, cx), (main, to));
}
