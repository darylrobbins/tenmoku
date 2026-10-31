//! Selecting a Git Log commit with a click to show its changed files, and a
//! click on a file focusing its diff in the pane's one preview tab, on
//! repositories built with the `git` CLI in temporary directories.

use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext,
};

use super::git_log_open_tests::{
    draw, fixture, lines, panel, press, row_center, row_of, selected, tab_count, title, window,
};
use crate::Workbench;

/// Clicks row `ix` of the list `clicks` times: a click, a double click.
fn click(
    workbench: &gpui::Entity<Workbench>,
    ix: usize,
    clicks: usize,
    cx: &mut VisualTestContext,
) {
    let at = row_center(&panel(workbench, cx), ix, cx);
    for click_count in 1..=clicks {
        cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
        });
    }
    draw(cx);
}

/// The tabs' titles of the active pane.
fn tabs(workbench: &gpui::Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.tabs()
            .iter()
            .map(|tab| tab.item().tab_title(cx).to_string())
            .collect()
    })
}

#[gpui::test]
fn click_expands(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let all = lines(&workbench, cx);
    let edit = row_of(&all, "edit");

    click(&workbench, edit, 1, cx);
    assert_eq!(selected(&workbench, cx), Some(edit));
    let listed = lines(&workbench, cx);
    assert!(listed.len() > all.len(), "expanded: {listed:?}");

    // Another click on the expanded commit collapses it.
    click(&workbench, edit, 1, cx);
    assert_eq!(lines(&workbench, cx), all);
    assert_eq!(selected(&workbench, cx), Some(edit));

    // Enter and o still toggle.
    press("enter", cx);
    assert!(lines(&workbench, cx).len() > all.len());
    press("enter", cx);
    assert_eq!(lines(&workbench, cx), all);
    press("o", cx);
    assert!(lines(&workbench, cx).len() > all.len());
}

#[gpui::test]
fn header(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let edit = row_of(&lines(&workbench, cx), "edit");
    click(&workbench, edit, 1, cx);
    let listed = lines(&workbench, cx);
    // a.rs +1 -1, n.rs +1, d.rs -1.
    assert_eq!(listed[edit + 1], "3 files changed, +2 -2");
    assert_eq!(listed[edit + 2], "M a.rs");
}

#[gpui::test]
fn file_focus(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let edit = row_of(&lines(&workbench, cx), "edit");
    click(&workbench, edit, 1, cx);
    let before = tab_count(&workbench, cx);

    // File rows follow the header: a.rs, d.rs, n.rs.
    click(&workbench, edit + 2, 1, cx);
    assert_eq!(tab_count(&workbench, cx), before + 1);
    let first = title(&workbench, cx);

    click(&workbench, edit + 3, 1, cx);
    assert_eq!(
        tab_count(&workbench, cx),
        before + 1,
        "{:?}",
        tabs(&workbench, cx)
    );
    assert_ne!(title(&workbench, cx), first);

    // A double click keeps the tab: the next click adds another.
    click(&workbench, edit + 4, 2, cx);
    assert_eq!(tab_count(&workbench, cx), before + 1);
    click(&workbench, edit + 2, 1, cx);
    assert_eq!(tab_count(&workbench, cx), before + 2);

    // Enter keeps it too.
    click(&workbench, edit + 3, 1, cx);
    assert_eq!(tab_count(&workbench, cx), before + 2);
    super::git_log_open_tests::focus_panel(&workbench, cx);
    press("enter", cx);
    click(&workbench, edit + 2, 1, cx);
    assert_eq!(tab_count(&workbench, cx), before + 3);
}

#[gpui::test]
fn double_click_takes_the_previews_place(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let edit = row_of(&lines(&workbench, cx), "edit");
    click(&workbench, edit, 1, cx);
    let before = tab_count(&workbench, cx);
    click(&workbench, edit + 2, 1, cx);
    // Double clicking another file replaces the preview and stays.
    click(&workbench, edit + 3, 2, cx);
    assert_eq!(
        tab_count(&workbench, cx),
        before + 1,
        "{:?}",
        tabs(&workbench, cx)
    );
    assert!(title(&workbench, cx).starts_with("d.rs"));
    click(&workbench, edit + 4, 1, cx);
    assert_eq!(tab_count(&workbench, cx), before + 2);
}

#[gpui::test]
fn rapid_clicks_leave_one_preview(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let edit = row_of(&lines(&workbench, cx), "edit");
    click(&workbench, edit, 1, cx);
    let before = tab_count(&workbench, cx);
    // Two files clicked before either read lands: only the last opens.
    let panel = panel(&workbench, cx);
    for file in [edit + 2, edit + 3] {
        let line = panel.read_with(cx, |panel, _| panel.lines[file]);
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.click(line, 1, window, cx)));
    }
    draw(cx);
    assert_eq!(
        tab_count(&workbench, cx),
        before + 1,
        "{:?}",
        tabs(&workbench, cx)
    );
    assert!(title(&workbench, cx).starts_with("d.rs"));
}

/// Titles of the diff tabs of each pane of the workbench.
fn pane_tabs(workbench: &gpui::Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.panes(cx).iter())
            .map(|pane| {
                (pane.read(cx).tabs().iter())
                    .map(|tab| tab.item().tab_title(cx).to_string())
                    .filter(|title| title.contains('↔'))
                    .collect()
            })
            .collect()
    })
}

/// Clicks file row `ix` `clicks` times, or presses Enter on it, without
/// letting the reads land.
fn ask(
    workbench: &gpui::Entity<Workbench>,
    ix: usize,
    clicks: Option<usize>,
    cx: &mut VisualTestContext,
) {
    let panel = panel(workbench, cx);
    let line = panel.read_with(cx, |panel, _| panel.lines[ix]);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| match clicks {
            Some(clicks) => panel.click(line, clicks, window, cx),
            None => {
                panel.select(ix, cx);
                panel.open_row(ix, window, cx)
            }
        })
    });
}

#[gpui::test]
fn repeated_keeps_share_one_read_and_the_previews_place(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let edit = row_of(&lines(&workbench, cx), "edit");
    click(&workbench, edit, 1, cx);
    click(&workbench, edit + 2, 1, cx);
    assert_eq!(pane_tabs(&workbench, cx)[0].len(), 1);
    // A triple click and an Enter on an unread file, none of the reads
    // landed yet.
    for clicks in [Some(1), Some(2), Some(3), None] {
        ask(&workbench, edit + 3, clicks, cx);
    }
    draw(cx);
    let tabs = pane_tabs(&workbench, cx);
    assert_eq!(tabs[0].len(), 1, "{tabs:?}");
    assert!(tabs[0][0].starts_with("d.rs"));
    // Kept: the next preview opens beside it.
    click(&workbench, edit + 4, 1, cx);
    assert_eq!(pane_tabs(&workbench, cx)[0].len(), 2);
}

#[gpui::test]
fn a_keep_after_changing_pane_leaves_that_panes_preview(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let edit = row_of(&lines(&workbench, cx), "edit");
    click(&workbench, edit, 1, cx);
    // A preview in a second pane.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.split(crate::pane_group::Direction::Right, window, cx)
    });
    draw(cx);
    click(&workbench, edit + 2, 1, cx);
    let panes = workbench.read_with(cx, |workbench, cx| workbench.panes(cx));
    // Preview d.rs in the first pane, then keep it from the second.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.activate(panes[0].clone(), window, cx)
    });
    ask(&workbench, edit + 3, Some(1), cx);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.activate(panes[1].clone(), window, cx)
    });
    ask(&workbench, edit + 3, None, cx);
    draw(cx);
    let tabs = pane_tabs(&workbench, cx);
    let in_second = &tabs[1];
    assert_eq!(in_second.len(), 2, "{tabs:?}");
    assert!(in_second.iter().any(|title| title.starts_with("a.rs")));
    assert!(in_second.iter().any(|title| title.starts_with("d.rs")));
}

#[gpui::test]
fn a_request_in_another_window_leaves_this_previews_alone(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (other_dir, _) = fixture();
    let (workbench, mut first) = window(cx, dir.path());
    let (other, mut second) = window(cx, other_dir.path());
    let first = &mut first;
    let second = &mut second;
    let edit = row_of(&lines(&workbench, first), "edit");
    let other_edit = row_of(&lines(&other, second), "edit");
    // Both commits expanded before either diff is asked for.
    click(&workbench, edit, 1, first);
    click(&other, other_edit, 1, second);
    ask(&workbench, edit + 2, Some(1), first);
    ask(&other, other_edit + 2, Some(1), second);
    // Neither read has landed when the second window asked.
    assert!(pane_tabs(&workbench, first)[0].is_empty());
    assert!(pane_tabs(&other, second)[0].is_empty());
    draw(second);
    draw(first);
    assert_eq!(pane_tabs(&workbench, first)[0].len(), 1);
    assert_eq!(pane_tabs(&other, second)[0].len(), 1);
}
