//! The editor tab strip, its pane controls and the breadcrumbs in B3
//! (`docs/design/design-system.md#workbench-anatomy`): 34 px tabs, the
//! accent line on the focused pane's active tab only, the unsaved dot, a
//! pinned tab as its icon; then the tab list, split right and "…", whose
//! menu lists the design's pane actions with the keys the keymap binds;
//! symbol crumbs in their kinds' syntax colours.

use std::{fs, path::Path};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand, set_vim_enabled};
use fe_syntax::Highlight;
use fe_ui::{ActiveTheme as _, CrumbKind, Icon, TAB_HEIGHT, Tab};
use gpui::{
    Bounds, Entity, Focusable as _, Hsla, Modifiers, Pixels, TestAppContext, VisualTestContext,
};
use kiln_shell::PaneView as _;

use super::open_root;
use crate::{Pane, Workbench};

const LIB_RS: &str = "\
mod outer {
    pub struct S;
    impl S {
        fn b() {
            let x = 1;
        }
    }
}
";

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), LIB_RS).unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(dir.path().join(name), "x\n").unwrap();
    }
    dir
}

/// A workbench rooted at `dir`, focused as the app focuses it, showing
/// `files` opened in turn in its pane, the last active.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
    files: &[&str],
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    for file in files {
        open_file(&workbench, &dir.path().join(file), cx);
    }
    (workbench, cx)
}

fn open_file(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = active(workbench, cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    settle(cx);
}

/// Runs and draws until the outline has landed and the crumbs show it.
fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().into_iter())
            .cloned()
            .collect()
    })
}

fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown")
}

fn run(command: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(command).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    settle(cx);
}

fn bounds(selector: String, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    // `debug_bounds` takes a `&'static str`.
    cx.debug_bounds(selector.leak())
}

fn tab(pane: &Entity<Pane>, ix: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    pane.read_with(cx, |pane, _| pane.tab_bounds(ix).expect("drawn"))
}

fn action(pane: &Entity<Pane>, ix: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    pane.read_with(cx, |pane, _| pane.action_bounds(ix).expect("drawn"))
}

fn click(at: Bounds<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_click(at.center(), Modifiers::none());
    settle(cx);
}

/// Each tab's accent line, by label, if it has one.
fn accents(labels: &[&str], cx: &mut VisualTestContext) -> Vec<(String, Option<Bounds<Pixels>>)> {
    (labels.iter())
        .map(|label| (label.to_string(), bounds(format!("tab-accent:{label}"), cx)))
        .collect()
}

/// The toolbar picker's rows, if it shows: title, key and command.
fn menu(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Vec<(String, Option<String>, &'static str)>> {
    workbench.read_with(cx, |workbench, cx| {
        let picker = &workbench.toolbar_picker()?.read(cx).delegate;
        let rows = picker.labels().into_iter().zip(picker.shortcuts());
        let rows = rows.zip(picker.commands());
        Some(rows.map(|((title, key), id)| (title, key, id)).collect())
    })
}

/// The label the platform prints `keys` with.
fn keys(keys: &str) -> Option<String> {
    Some(fe_keymap::shortcut_label(
        &fe_keymap::KeySequence::parse(keys).unwrap(),
    ))
}

#[gpui::test]
fn tabs_mark_the_focused_pane_and_end_in_its_controls(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir, &["a.txt", "b.txt"]);
    let left = active(&workbench, cx);

    // 34 px, the strip and each tab, from the pane's top.
    let pane_top = left.read_with(cx, |pane, _| pane.bounds().top());
    let strip = left.read_with(cx, |pane, _| pane.pane().tab_bar_bounds());
    assert_eq!(TAB_HEIGHT, gpui::px(34.));
    assert_eq!((strip.top(), strip.size.height), (pane_top, TAB_HEIGHT));
    for ix in 0..2 {
        let tab = tab(&left, ix, cx);
        assert_eq!(tab.top(), pane_top, "tab {ix}");
        assert!(tab.size.height >= TAB_HEIGHT - gpui::px(1.), "tab {ix}");
    }

    // One pane, focused: its active tab alone has the 2 px line, along its
    // top, as wide as it.
    let drawn = accents(&["a.txt", "b.txt"], cx);
    assert_eq!(drawn[0].1, None);
    let line = drawn[1].1.expect("the active tab's");
    let active_tab = tab(&left, 1, cx);
    assert_eq!(line.origin, active_tab.origin);
    // Inside its right border.
    let short = active_tab.size.width - line.size.width;
    assert!(
        short >= gpui::px(0.) && short <= gpui::px(1.),
        "{line:?} in {active_tab:?}"
    );
    assert_eq!(line.size.height, gpui::px(2.));

    // Split, with another file in the new pane: only the focused pane's
    // active tab has it.
    run("pane.split_right", cx);
    let right = active(&workbench, cx);
    assert_ne!(right, left);
    open_file(&workbench, &dir.path().join("c.txt"), cx);
    let drawn = accents(&["a.txt", "b.txt", "c.txt"], cx);
    let marked: Vec<_> = (drawn.iter())
        .filter_map(|(label, line)| line.map(|_| label.as_str()))
        .collect();
    assert_eq!(marked, ["c.txt"]);
    let c = right.read_with(cx, |pane, _| pane.pane().active()).unwrap();
    assert_eq!(drawn[2].1.unwrap().origin, tab(&right, c, cx).origin);

    // Focus back in the left pane moves it there.
    let left_editor = left.read_with(cx, |pane, cx| pane.focus_target(cx));
    cx.update(|window, cx| window.focus(&left_editor, cx));
    settle(cx);
    let drawn = accents(&["a.txt", "b.txt", "c.txt"], cx);
    let marked: Vec<_> = (drawn.iter())
        .filter_map(|(label, line)| line.map(|_| label.as_str()))
        .collect();
    assert_eq!(marked, ["b.txt"]);

    // After the tabs, at the strip's end: the tab list, split right, "…".
    let ids: Vec<_> = left.read_with(cx, |pane, _| {
        (pane.drawn_actions().iter())
            .map(|(id, icon)| (*id, *icon))
            .collect()
    });
    assert_eq!(
        ids,
        [
            ("tab-list", Icon::ChevronDown),
            ("split-right", Icon::Columns2),
            ("pane-menu", Icon::Ellipsis),
        ]
    );
    let buttons: Vec<_> = (0..3).map(|ix| action(&left, ix, cx)).collect();
    let strip = left.read_with(cx, |pane, _| pane.pane().tab_bar_bounds());
    assert!(
        strip.right() <= buttons[0].left(),
        "after the tabs: {strip:?} {buttons:?}"
    );
    for pair in buttons.windows(2) {
        assert!(pair[0].right() <= pair[1].left(), "{buttons:?}");
    }
    for button in &buttons {
        assert!(button.top() >= strip.top() && button.bottom() <= strip.bottom());
    }
}

#[gpui::test]
fn a_dirty_tab_shows_its_dot_and_a_pinned_tab_its_icon(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir, &["a.txt", "b.txt"]);
    let pane = active(&workbench, cx);

    // Clean, the active tab has its close mark, the inactive one none
    // until hovered; edited, the dot in the mark's place.
    assert!(bounds("tab-close:b.txt".into(), cx).is_some());
    assert_eq!(bounds("tab-dirty:b.txt".into(), cx), None);
    let edited = editor(&workbench, cx);
    edited.update(cx, |editor, cx| {
        editor.model_mut().insert("y");
        cx.notify();
    });
    settle(cx);
    let dot = bounds("tab-dirty:b.txt".into(), cx).expect("the dot");
    assert!(tab(&pane, 1, cx).contains(&dot.center()));
    assert_eq!(bounds("tab-close:b.txt".into(), cx), None);
    assert_eq!(bounds("tab-dirty:a.txt".into(), cx), None);

    // Pinned, a tab is its icon alone: compact, no label, no close mark.
    let unpinned = tab(&pane, 0, cx).size.width;
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    run("workbench.pin_tab", cx);
    let tabs = pane.read_with(cx, |pane, cx| pane.pane().tabs(cx));
    assert!(tabs[0].is_pinned());
    let pinned = tab(&pane, 0, cx);
    assert_eq!(pinned.size.width, Tab::PINNED_WIDTH);
    assert!(pinned.size.width < unpinned);
    let icon = tabs[0].icon_ref().expect("a file icon").name();
    let drawn = bounds(format!("tab-icon:a.txt:{icon}"), cx).expect("its icon");
    assert!(pinned.contains(&drawn.center()));
    assert_eq!(bounds("tab-close:a.txt".into(), cx), None);
    assert_eq!(bounds("tab-pin:a.txt".into(), cx), None, "no pin glyph");
    assert_eq!(
        tabs[0].tooltip_text().as_deref(),
        Some("a.txt"),
        "its name in its tooltip"
    );
    // Its accent line, as the focused pane's active tab.
    let line = bounds("tab-accent:a.txt".into(), cx).expect("marked");
    assert_eq!(line.origin, pinned.origin);
    assert!(line.size.width >= Tab::PINNED_WIDTH - gpui::px(1.));
}

#[gpui::test]
fn the_tab_list_lists_the_pane_s_tabs(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir, &["a.txt", "b.txt", "c.txt"]);
    let pane = active(&workbench, cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(1, window, cx)));
    settle(cx);

    click(action(&pane, 0, cx), cx);
    let rows = workbench.read_with(cx, |workbench, cx| {
        let picker = workbench.toolbar_picker().expect("shown").read(cx);
        (picker.delegate.labels(), picker.selected())
    });
    assert_eq!(
        rows,
        (vec!["a.txt".into(), "b.txt".into(), "c.txt".into()], 1)
    );

    // A pick shows the tab, focused.
    cx.simulate_input("c");
    settle(cx);
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert!(workbench.read_with(cx, |workbench, _| workbench.toolbar_picker().is_none()));
    assert_eq!(pane.read_with(cx, |pane, _| pane.pane().active()), Some(2));
    let shown = editor(&workbench, cx);
    let path = shown.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned));
    assert_eq!(path, Some(dir.path().join("c.txt")));
    assert!(cx.update(|window, cx| shown.focus_handle(cx).is_focused(window)));
}

#[gpui::test]
fn pane_menu_lists_the_design_actions_with_their_keys(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir, &["a.txt", "b.txt"]);
    let pane = active(&workbench, cx);

    // The design's actions, in its order, then the rest; each with the key
    // the keymap binds where the editor has focus, chords too, and none
    // when nothing binds it. The keymap is macOS's.
    click(action(&pane, 2, cx), cx);
    let design = [
        ("Split Right", keys("cmd-\\"), "pane.split_right"),
        ("Split Down", keys("cmd-k cmd-\\"), "pane.split_down"),
        ("Pin Tab", None, "workbench.pin_tab"),
        ("Close Other Tabs", None, "workbench.close_other_tabs"),
        (
            "Reopen Closed Tab",
            keys("cmd-shift-t"),
            "workbench.reopen_closed_editor",
        ),
    ];
    let rows = menu(&workbench, cx).expect("shown");
    let leading: Vec<_> = (rows.iter().take(design.len()))
        .map(|(title, key, id)| (title.as_str(), key.clone(), *id))
        .collect();
    assert_eq!(leading, design);
    let rest: Vec<_> = rows[design.len()..]
        .iter()
        .map(|row| row.0.as_str())
        .collect();
    assert_eq!(
        rest,
        [
            "Close Tab",
            "Close Pane",
            "Move Tab Left",
            "Move Tab Right",
            "Move Tab Up",
            "Move Tab Down",
            "Toggle Breadcrumbs",
        ]
    );
    cx.simulate_keystrokes("escape");
    settle(cx);

    // With vim on, in normal mode, the window keys bind them: ⌃W V, the
    // fewest keys held, over its ⌃W ⌃V.
    cx.update(|_, cx| set_vim_enabled(cx, true));
    settle(cx);
    click(action(&pane, 2, cx), cx);
    let rows = menu(&workbench, cx).expect("shown");
    let key = |id: &str| rows.iter().find(|row| row.2 == id).unwrap().1.clone();
    assert_eq!(key("pane.split_right"), keys("ctrl-w v"));
    assert_eq!(key("pane.split_down"), keys("ctrl-w s"));
    assert_eq!(key("workbench.pin_tab"), None);
    cx.simulate_keystrokes("escape");
    settle(cx);
    cx.update(|_, cx| set_vim_enabled(cx, false));
    settle(cx);

    // A pinned active tab lists Unpin Tab in Pin Tab's place, which runs.
    run("workbench.pin_tab", cx);
    click(action(&pane, 2, cx), cx);
    let rows = menu(&workbench, cx).expect("shown");
    assert_eq!(
        (rows[2].0.as_str(), rows[2].2),
        ("Unpin Tab", "workbench.unpin_tab")
    );
    assert!(!rows.iter().any(|row| row.2 == "workbench.pin_tab"));
    cx.simulate_input("unpin");
    settle(cx);
    cx.simulate_keystrokes("enter");
    settle(cx);
    let pinned = pane.read_with(cx, |pane, _| {
        let state = pane.pane();
        state.active().is_some_and(|ix| state.is_pinned(ix))
    });
    assert!(!pinned);
    assert_eq!(panes(&workbench, cx), [pane]);
}

#[gpui::test]
fn symbol_crumbs_are_syntax_coloured(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir, &["src/lib.rs"]);
    let pane = active(&workbench, cx);
    let editor = editor(&workbench, cx);
    editor.update(cx, |editor, cx| {
        let offset = LIB_RS.find("let x").unwrap();
        editor.model_mut().set_cursor(offset);
        cx.notify();
    });
    settle(cx);

    let drawn = pane.read_with(cx, |pane, _| {
        let crumbs = pane.drawn_crumbs().iter();
        let labels = crumbs.map(|(label, kind, _)| (label.clone(), *kind));
        labels
            .zip(pane.crumb_colors().iter().copied())
            .collect::<Vec<_>>()
    });
    let labels: Vec<_> = drawn
        .iter()
        .map(|((label, kind), _)| (label.as_str(), *kind))
        .collect();
    assert_eq!(
        labels,
        [
            ("src", CrumbKind::Path),
            ("lib.rs", CrumbKind::File),
            ("outer", CrumbKind::Symbol),
            ("impl S", CrumbKind::Symbol),
            ("b", CrumbKind::Symbol),
        ]
    );
    let text = cx.update(|_, cx| cx.theme().colors.text);
    // A highlight the theme leaves uncoloured is drawn as plain text.
    let syntax = |capture: &str, cx: &mut VisualTestContext| -> Option<Hsla> {
        editor.read_with(cx, |editor, _| {
            let highlight = Highlight::from_capture(capture).unwrap();
            editor.theme().syntax_color(highlight)
        })
    };
    let ty = syntax("type", cx).expect("types are coloured");
    let method = syntax("function.method", cx).expect("methods are coloured");
    let module = syntax("module", cx).unwrap_or(text.primary);
    let colors: Vec<_> = drawn.iter().map(|(_, color)| *color).collect();
    assert_eq!(colors, [text.muted, text.primary, module, ty, method]);
    // Kinds apart, so the colours say which is which.
    assert_ne!(ty, method);
}

/// The quick outline (Go to Symbol in File), if it shows.
fn quick_outline(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<crate::Picker<crate::symbol_picker::SymbolPicker>>> {
    workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        (layer.modal() == Some(crate::workbench::Palette::DocumentSymbols.id()))
            .then(|| layer.modal_view().unwrap().clone().downcast().unwrap())
    })
}

#[gpui::test]
fn clicking_a_symbol_crumb_opens_the_quick_outline(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir, &["src/lib.rs"]);
    let pane = active(&workbench, cx);
    let editor = editor(&workbench, cx);
    editor.update(cx, |editor, cx| {
        editor.model_mut().set_cursor(LIB_RS.find("let x").unwrap());
        cx.notify();
    });
    settle(cx);
    let crumbs = pane.read_with(cx, |pane, _| pane.drawn_crumbs().len());
    assert_eq!(crumbs, 5, "src, lib.rs, outer, impl S, b");

    click(bounds("crumb:4:b".into(), cx).expect("drawn"), cx);
    let outline = quick_outline(&workbench, cx).expect("the quick outline shows");
    let rows = outline.read_with(cx, |picker, _| picker.delegate.row_names());
    for symbol in ["outer", "S", "impl S", "b"] {
        assert!(rows.iter().any(|row| row == symbol), "{symbol} in {rows:?}");
    }
    assert!(
        workbench.read_with(cx, |workbench, _| workbench.toolbar_picker().is_none()),
        "not the sibling list"
    );
    let run = workbench.read_with(cx, |workbench, _| workbench.commands_run.clone());
    let last = run.last().expect("a command ran");
    assert_eq!(
        (last.command.as_str(), last.source),
        ("nav.document_symbols", fe_command::Source::Mouse)
    );

    // Dismissed, focus is back in the editor.
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(quick_outline(&workbench, cx).is_none());
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
}
