//! The "..." Toggle Search Details button and `search.toggle_details`: the
//! include and exclude fields are hidden until asked for, and hidden globs
//! still apply.

use std::fs;

use fe_gpui::{EditorSettings, EditorTheme};
use fe_project::SearchOptions;
use fe_ui::TextInput;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::ShownRow;
use crate::{Launch, SearchPanel, Workbench};

fn searching(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<SearchPanel>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.rs", "b.md"] {
        fs::write(dir.path().join(name), "a needle\n").unwrap();
    }
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let search = workbench.read_with(cx, |workbench: &Workbench, cx| {
        workbench.dock().read(cx).search().clone()
    });
    search.update(cx, |search, _| {
        search.set_options(SearchOptions {
            threads: 1,
            ..SearchOptions::default()
        })
    });
    cx.simulate_keystrokes("cmd-shift-f");
    cx.run_until_parked();
    (dir, search, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn click_toggle(cx: &mut VisualTestContext) {
    draw(cx);
    let bounds = cx.debug_bounds("search-details-toggle").expect("toggle");
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

fn focused(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn details_hidden_until_toggled(cx: &mut TestAppContext) {
    let (_dir, search, cx) = searching(cx);
    draw(cx);
    assert!(!search.read_with(cx, |s, _| s.details_shown()));
    assert!(cx.debug_bounds("search-details-toggle").is_some());
    let include = search.read_with(cx, |s, _| s.include_input().clone());
    let field_bounds = |cx: &mut VisualTestContext| {
        let _ = &include;
        search.read_with(cx, |s, _| s.details_shown())
    };
    assert!(!field_bounds(cx));
    click_toggle(cx);
    assert!(search.read_with(cx, |s, _| s.details_shown()));
    click_toggle(cx);
    assert!(!search.read_with(cx, |s, _| s.details_shown()));
}

#[gpui::test]
fn hidden_filters_still_apply(cx: &mut TestAppContext) {
    let (_dir, search, cx) = searching(cx);
    click_toggle(cx);
    let (query, include) =
        search.read_with(cx, |s, _| (s.input().clone(), s.include_input().clone()));
    query.update(cx, |input, cx| input.insert("needle", cx));
    search.update(cx, |s, cx| s.run(cx));
    cx.run_until_parked();
    include.update(cx, |input, cx| input.insert("*.rs", cx));
    cx.run_until_parked();
    click_toggle(cx);
    assert!(!search.read_with(cx, |s, _| s.details_shown()));
    search.update(cx, |s, cx| s.run(cx));
    cx.run_until_parked();
    let files: Vec<String> = search.read_with(cx, |s, _| {
        s.shown_rows()
            .into_iter()
            .filter_map(|row| match row {
                ShownRow::File(rel, _) => Some(rel),
                ShownRow::Folder(..) | ShownRow::Line(..) => None,
            })
            .collect()
    });
    assert_eq!(files, ["a.rs"]);
}

#[gpui::test]
fn hiding_details_refocuses_query(cx: &mut TestAppContext) {
    let (_dir, search, cx) = searching(cx);
    click_toggle(cx);
    let (query, include) =
        search.read_with(cx, |s, _| (s.input().clone(), s.include_input().clone()));
    cx.update(|window, cx| window.focus(&include.focus_handle(cx), cx));
    assert!(focused(&include, cx));
    cx.simulate_keystrokes("cmd-shift-j");
    draw(cx);
    assert!(!search.read_with(cx, |s, _| s.details_shown()));
    assert!(focused(&query, cx));
}

#[gpui::test]
fn cmd_shift_j_toggles_details(cx: &mut TestAppContext) {
    let (_dir, search, cx) = searching(cx);
    let query = search.read_with(cx, |s, _| s.input().clone());
    cx.update(|window, cx| window.focus(&query.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-shift-j");
    draw(cx);
    assert!(search.read_with(cx, |s, _| s.details_shown()));
    cx.simulate_keystrokes("cmd-shift-j");
    draw(cx);
    assert!(!search.read_with(cx, |s, _| s.details_shown()));
}
