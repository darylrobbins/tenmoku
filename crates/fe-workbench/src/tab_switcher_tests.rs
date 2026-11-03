//! The most-recently-used tab switcher, driven by the default keys and the
//! modifier events a real press sends: ctrl down, ctrl-tab, ctrl up.

use std::fs;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::{
    Capslock, Entity, Modifiers, ModifiersChangedEvent, PlatformInput, TestAppContext,
    VisualTestContext,
};
use kiln_shell::PaneView as _;

use crate::{Pane, Workbench, tests::open_root};

/// A workbench with `names` opened in order in its pane, the last focused.
fn opened<'a>(
    names: &[&str],
    cx: &'a mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    for name in names {
        fs::write(dir.path().join(name), *name).unwrap();
    }
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, _| window.activate_window());
    let pane = pane(&workbench, cx);
    for name in names {
        let path = dir.path().join(name);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    cx.update(|window, cx| {
        let focus = pane.read(cx).focus_target(cx);
        window.focus(&focus, cx);
    });
    draw(cx);
    (dir, workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

/// The active pane's shown tab's title.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let pane = pane(workbench, cx);
    pane.read_with(cx, |pane, cx| {
        pane.active_item().unwrap().tab_title(cx).to_string()
    })
}

/// Whether the active pane's shown tab has focus.
fn shown_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let pane = pane(workbench, cx);
    cx.update(|window, cx| {
        let focus = pane.read(cx).focus_target(cx);
        focus.is_focused(window)
    })
}

/// The list showing: its rows' titles and the selected one's.
fn list(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<(Vec<String>, String)> {
    workbench.read_with(cx, |workbench, cx| {
        let switcher = workbench.tab_switcher(cx)?;
        let switcher = switcher.read(cx);
        Some((switcher.titles(cx), switcher.selected_title(cx)?))
    })
}

fn hold_ctrl(cx: &mut VisualTestContext) {
    cx.simulate_modifiers_change(Modifiers::control());
    draw(cx);
}

fn release_ctrl(cx: &mut VisualTestContext) {
    cx.simulate_modifiers_change(Modifiers::none());
    draw(cx);
}

/// Presses `keys`, each with ctrl already held.
fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

/// Shows each of `names` by clicking through its strip index, in order.
fn activate_in_order(names: &[&str], workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let pane = pane(workbench, cx);
    for name in names {
        cx.update(|window, cx| {
            pane.update(cx, |pane, cx| {
                let ix = (pane.pane().items().iter())
                    .position(|item| item.tab_title(cx) == *name)
                    .unwrap();
                pane.activate_at(ix, window, cx);
            })
        });
        draw(cx);
    }
}

/// Closes tab `name` of the active pane, whichever is active.
fn close(name: &'static str, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let pane = pane(workbench, cx);
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.close_matching(true, |item, cx| item.tab_title(cx) == name, window, cx);
        })
    });
    draw(cx);
}

/// Runs command `id` as the menu and the palette do, with no key involved.
fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

#[gpui::test]
fn tap_goes_to_previous_tab(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&["a.txt", "b.txt", "c.txt"], cx);
    // c.txt was used last, though a.txt is next to b.txt in the strip order
    // from its start.
    activate_in_order(&["a.txt", "c.txt", "b.txt"], &workbench, cx);
    assert_eq!(shown(&workbench, cx), "b.txt");

    // The press lists the tabs and holds off switching until the release.
    hold_ctrl(cx);
    press("ctrl-tab", cx);
    assert_eq!(shown(&workbench, cx), "b.txt");
    assert!(list(&workbench, cx).is_some());
    release_ctrl(cx);
    assert_eq!(shown(&workbench, cx), "c.txt");
    assert!(list(&workbench, cx).is_none());
    assert!(shown_focused(&workbench, cx), "focus is on the tab shown");

    // Tapping again comes back: the pair swaps, rather than walking on.
    hold_ctrl(cx);
    press("ctrl-tab", cx);
    release_ctrl(cx);
    assert_eq!(shown(&workbench, cx), "b.txt");
    assert!(shown_focused(&workbench, cx));

    // A release in the same update as the press, before any frame has drawn
    // the list, still commits it: the window draws before it dispatches.
    hold_ctrl(cx);
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.switch_tab(false, window, cx));
        assert!(workbench.read(cx).tab_switcher(cx).is_some());
        window.dispatch_event(
            PlatformInput::ModifiersChanged(ModifiersChangedEvent {
                modifiers: Modifiers::none(),
                capslock: Capslock { on: false },
            }),
            cx,
        );
    });
    draw(cx);
    assert_eq!(shown(&workbench, cx), "c.txt");
    assert!(list(&workbench, cx).is_none());

    // Letting go of another modifier isn't letting go of ctrl.
    hold_ctrl(cx);
    cx.simulate_modifiers_change(Modifiers::control_shift());
    press("ctrl-shift-tab", cx);
    cx.simulate_modifiers_change(Modifiers::control());
    draw(cx);
    assert!(list(&workbench, cx).is_some());
    release_ctrl(cx);
    assert!(list(&workbench, cx).is_none());
}

#[gpui::test]
fn mru_order_follows_activation(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&["a.txt", "b.txt", "c.txt", "d.txt"], cx);
    let by_use = ["d.txt", "c.txt", "b.txt", "a.txt"]
        .map(String::from)
        .to_vec();
    activate_in_order(&["a.txt", "b.txt", "c.txt", "d.txt"], &workbench, cx);

    let selected = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        let (titles, selected) = list(workbench, cx).unwrap();
        assert_eq!(titles, by_use);
        selected
    };

    // Forward from the second, wrapping past the last to the active tab.
    hold_ctrl(cx);
    press("ctrl-tab", cx);
    assert_eq!(selected(&workbench, cx), "c.txt");
    for expected in ["b.txt", "a.txt", "d.txt", "c.txt", "b.txt"] {
        press("ctrl-tab", cx);
        assert_eq!(selected(&workbench, cx), expected);
    }
    release_ctrl(cx);
    assert_eq!(shown(&workbench, cx), "b.txt");

    // Committing used the tab: it heads the order now.
    activate_in_order(&["a.txt", "b.txt", "c.txt", "d.txt"], &workbench, cx);

    // Backward from the last, wrapping past the active tab.
    hold_ctrl(cx);
    press("ctrl-shift-tab", cx);
    assert_eq!(selected(&workbench, cx), "a.txt");
    for expected in ["b.txt", "c.txt", "d.txt", "a.txt", "b.txt"] {
        press("ctrl-shift-tab", cx);
        assert_eq!(selected(&workbench, cx), expected);
    }
    release_ctrl(cx);
    assert_eq!(shown(&workbench, cx), "b.txt");

    // Selecting the active tab itself commits to staying on it.
    let by_use = ["b.txt", "d.txt", "c.txt", "a.txt"].map(String::from);
    hold_ctrl(cx);
    press("ctrl-tab", cx);
    assert_eq!(list(&workbench, cx).unwrap().0, by_use);
    press("ctrl-shift-tab", cx);
    release_ctrl(cx);
    assert_eq!(shown(&workbench, cx), "b.txt");
    assert!(shown_focused(&workbench, cx));
}

#[gpui::test]
fn escape_and_close_leave_active_tab(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(&["a.txt", "b.txt", "c.txt", "d.txt"], cx);
    activate_in_order(&["a.txt", "b.txt", "c.txt", "d.txt"], &workbench, cx);

    // Escape is ctrl-escape while ctrl is held, as it must be for the list
    // to be up, and plain escape too. Neither switches, nor does the release
    // that follows, and focus goes back.
    for keys in ["ctrl-escape", "escape"] {
        hold_ctrl(cx);
        press("ctrl-tab", cx);
        press("ctrl-tab", cx);
        assert_eq!(list(&workbench, cx).unwrap().1, "b.txt");
        press(keys, cx);
        assert!(list(&workbench, cx).is_none(), "{keys}");
        assert_eq!(shown(&workbench, cx), "d.txt", "{keys}");
        assert!(shown_focused(&workbench, cx), "{keys}");
        release_ctrl(cx);
        assert_eq!(shown(&workbench, cx), "d.txt", "{keys}");
    }

    // A tab closed while the list is up leaves it, the selection staying on
    // its tab, or at its place with that gone.
    hold_ctrl(cx);
    press("ctrl-tab", cx);
    assert_eq!(list(&workbench, cx).unwrap().1, "c.txt");
    close("a.txt", &workbench, cx);
    assert_eq!(
        list(&workbench, cx).unwrap(),
        (
            ["d.txt", "c.txt", "b.txt"].map(String::from).to_vec(),
            "c.txt".to_owned()
        )
    );
    close("c.txt", &workbench, cx);
    assert_eq!(
        list(&workbench, cx).unwrap(),
        (
            ["d.txt", "b.txt"].map(String::from).to_vec(),
            "b.txt".to_owned()
        )
    );
    press("escape", cx);
    release_ctrl(cx);
    assert_eq!(shown(&workbench, cx), "d.txt");

    // A tab moved to another pane leaves this pane's list and heads that
    // pane's, which only lists its own.
    let first = pane(&workbench, cx);
    run("pane.move_tab_right", cx);
    assert_ne!(pane(&workbench, cx), first);
    assert_eq!(shown(&workbench, cx), "d.txt");
    run("pane.focus_left", cx);
    assert_eq!(pane(&workbench, cx), first);
    assert_eq!(shown(&workbench, cx), "b.txt");
    let tabs = first.read_with(cx, |pane, _| pane.tabs_by_recency());
    assert_eq!(tabs.len(), 1);

    // With one tab left there's no list to choose from.
    hold_ctrl(cx);
    press("ctrl-tab", cx);
    assert!(list(&workbench, cx).is_none());
    assert_eq!(shown(&workbench, cx), "b.txt");
    release_ctrl(cx);

    // A pane's own tabs only: nothing lists d.txt, now in the other pane.
    let path = dir.path().join("e.txt");
    fs::write(&path, "e").unwrap();
    cx.update(|window, cx| first.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|window, cx| {
        let focus = first.read(cx).focus_target(cx);
        window.focus(&focus, cx);
    });
    draw(cx);
    hold_ctrl(cx);
    press("ctrl-tab", cx);
    assert_eq!(
        list(&workbench, cx).unwrap().0,
        ["e.txt", "b.txt"].map(String::from)
    );
    press("escape", cx);
    release_ctrl(cx);
}

#[gpui::test]
fn focus_leaving_closes_the_list_without_switching(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&["a.txt", "b.txt"], cx);
    hold_ctrl(cx);
    press("ctrl-tab", cx);
    assert_eq!(list(&workbench, cx).unwrap().1, "a.txt");

    // Say the user switched to another app, ctrl still down: the release
    // happens where the window can't hear it.
    cx.deactivate_window();
    draw(cx);
    assert!(list(&workbench, cx).is_none());
    assert_eq!(shown(&workbench, cx), "b.txt");
}

/// The menu, the palette or a key bound without ctrl has no release to wait
/// for, so it goes straight to the tab the list would select.
#[gpui::test]
fn without_ctrl_held_goes_straight_there(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&["a.txt", "b.txt", "c.txt"], cx);
    run("workbench.tab_switcher_next", cx);
    assert!(list(&workbench, cx).is_none());
    assert_eq!(shown(&workbench, cx), "b.txt");
    run("workbench.tab_switcher_next", cx);
    assert_eq!(shown(&workbench, cx), "c.txt");
    run("workbench.tab_switcher_previous", cx);
    assert!(list(&workbench, cx).is_none());
    assert_eq!(shown(&workbench, cx), "a.txt");
    assert!(shown_focused(&workbench, cx));
}

/// A file landing behind the tab the user moved on to was never seen, so a
/// tap goes to the tab used before, not to it.
#[gpui::test]
fn a_tab_landing_behind_is_not_the_previous_tab(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(&["a.txt", "b.txt", "c.txt"], cx);
    activate_in_order(&["a.txt", "c.txt", "b.txt"], &workbench, cx);
    let late = dir.path().join("late.txt");
    fs::write(&late, "late").unwrap();

    // The read is asked for, then the user shows a tab before it lands.
    let pane = pane(&workbench, cx);
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.open_as_text(late, window, cx);
            pane.cycle(false, window, cx);
            pane.cycle(true, window, cx);
        })
    });
    draw(cx);
    assert_eq!(shown(&workbench, cx), "b.txt");
    assert_eq!(pane.read_with(cx, |pane, _| pane.tabs().len()), 4);

    hold_ctrl(cx);
    press("ctrl-tab", cx);
    let by_use = ["b.txt", "c.txt", "a.txt", "late.txt"].map(String::from);
    assert_eq!(list(&workbench, cx).unwrap().0, by_use);
    release_ctrl(cx);
    assert_eq!(shown(&workbench, cx), "c.txt");
}

/// Removing a row above the selection, with the list scrolled, moves the
/// selection up past the viewport's top: the list follows it.
#[gpui::test]
fn the_selection_stays_in_view_as_rows_go(cx: &mut TestAppContext) {
    let names: Vec<String> = (0..20).map(|ix| format!("t{ix:02}.txt")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let (_dir, workbench, cx) = opened(&names, cx);

    // From the last row, t00, back up to t15, the viewport's top by then,
    // with rows below it to keep the list from settling back at its end.
    hold_ctrl(cx);
    for _ in 0..16 {
        press("ctrl-shift-tab", cx);
    }
    assert_eq!(list(&workbench, cx).unwrap().1, "t15.txt");
    assert!(cx.debug_bounds("tab-switcher-row:t15.txt").is_some());

    close("t18.txt", &workbench, cx);
    assert_eq!(list(&workbench, cx).unwrap().1, "t15.txt");
    assert!(cx.debug_bounds("tab-switcher-row:t15.txt").is_some());
    press("escape", cx);
    release_ctrl(cx);
}
