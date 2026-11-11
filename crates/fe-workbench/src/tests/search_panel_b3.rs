//! The Search panel in B3 (`docs/design/design-system.md`, Workbench
//! anatomy and Surfaces): the fields as the controls' inputs with their
//! toggles inside, matches on the find fill, result lines ending in an
//! ellipsis, the toolbar's actions in the dock's header, and the centred
//! empty state when nothing matched.

use std::fs;

use fe_gpui::editor_palette;
use fe_ui::ActiveTheme as _;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{SearchPanel, Workbench, search_panel::match_style, tests::open_root};

/// A workbench on a project of `files`, search shown and `query` run.
fn searched<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, String)],
    query: &str,
) -> (
    Entity<Workbench>,
    Entity<SearchPanel>,
    &'a mut VisualTestContext,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        fs::write(dir.path().join(name), text).unwrap();
    }
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let search = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).search().clone()
    });
    search.update(cx, |search, _| {
        search.set_options(fe_project::SearchOptions {
            threads: 1,
            ..Default::default()
        })
    });
    cx.simulate_keystrokes("cmd-shift-f");
    cx.simulate_input(query);
    cx.simulate_keystrokes("enter");
    draw(cx);
    (workbench, search, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

#[gpui::test]
fn matches_are_drawn_with_the_other_match_fill(cx: &mut TestAppContext) {
    let (_workbench, _search, cx, _dir) = searched(cx, &[("a.txt", "a needle\n".into())], "needle");
    cx.update(|_, cx| {
        let style = match_style(&cx.theme().ui_theme(), cx);
        let fill = editor_palette(cx).editor.find_match_other;
        assert_eq!(
            style.background_color,
            Some(gpui::rgba(fill.to_rgba_u32()).into()),
            "a match has the find_match_other fill, bold alone isn't enough"
        );
    });
}

#[gpui::test]
fn long_lines_end_in_an_ellipsis_within_their_row(cx: &mut TestAppContext) {
    let long = format!("needle {}\n", "word ".repeat(200));
    let (_workbench, search, cx, _dir) = searched(cx, &[("a.txt", long)], "needle");
    // Rows: a.txt, then its line.
    let row = search.read_with(cx, |search, _| search.row_bounds(1).expect("drawn"));
    let text = cx
        .debug_bounds("search-line-text:1")
        .expect("the line's text is drawn");
    assert!(
        text.right() <= row.right() && text.left() >= row.left(),
        "{text:?} is within {row:?}: clipped, ending in an ellipsis"
    );
}

#[gpui::test]
fn the_toolbar_actions_are_in_the_dock_header(cx: &mut TestAppContext) {
    let (_workbench, _search, cx, _dir) = searched(cx, &[("a.txt", "needle\n".into())], "needle");
    let panel = cx.debug_bounds("sidebar-panel").expect("the panel");
    for id in [
        "search-refresh",
        "search-clear",
        "search-view-mode",
        "search-collapse",
    ] {
        let button = cx
            .debug_bounds(format!("icon-button:{id}").leak())
            .unwrap_or_else(|| panic!("{id} is in the header"));
        assert!(
            button.bottom() <= panel.top(),
            "{id} is above the panel, in the dock's header: {button:?} {panel:?}"
        );
    }
    for id in [
        "search-toolbar-refresh",
        "search-toolbar-clear",
        "search-toolbar-view-mode",
        "search-toolbar-collapse",
    ] {
        assert!(cx.debug_bounds(id).is_none(), "no toolbar row's {id}");
    }
}

#[gpui::test]
fn no_results_shows_the_centred_empty_state(cx: &mut TestAppContext) {
    let (_workbench, _search, cx, _dir) = searched(cx, &[("a.txt", "hay\n".into())], "needle");
    let panel = cx.debug_bounds("sidebar-panel").expect("the panel");
    let empty = cx.debug_bounds("search-empty").expect("the empty state");
    assert!(
        (empty.center().x - panel.center().x).abs() < gpui::px(1.),
        "centred across the panel: {empty:?} in {panel:?}"
    );
    assert!(empty.top() > panel.top() && empty.bottom() <= panel.bottom());
}

#[gpui::test]
fn the_fields_hold_their_toggles(cx: &mut TestAppContext) {
    let (_workbench, search, cx, _dir) = searched(cx, &[("a.txt", "needle\n".into())], "needle");
    let field = cx.debug_bounds("search-field").expect("the query's field");
    for ix in 0..3 {
        let toggle = search.read_with(cx, |search, _| search.toggle_bounds(ix).expect("drawn"));
        assert!(
            field.contains(&toggle.center()),
            "toggle {ix} {toggle:?} is inside the field {field:?}"
        );
    }
}
