//! Choices beyond the toast's two buttons go into a More menu: the first
//! button, then More, whose menu lists the rest in order; picking from it
//! replies with that choice, from the mouse or the keyboard.

use std::{cell::RefCell, fs, rc::Rc};

use fe_buffer::Buffer;
use gpui::{Modifiers, TestAppContext, VisualTestContext};

use super::open;
use crate::{
    Launch, Workbench,
    workbench::toast::{Toast, show},
};

fn opened(cx: &mut TestAppContext) -> (tempfile::TempDir, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, _| window.activate_window());
    draw(cx);
    (dir, cx)
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx)
        });
    }
}

type Answers = Rc<RefCell<Vec<Option<usize>>>>;

/// Shows a choice-only toast with `titles`, collecting its replies.
fn ask(titles: &[&str], cx: &mut VisualTestContext) -> Answers {
    let answers = Answers::default();
    let sink = answers.clone();
    let toast = Toast::info("Pick one").choices(
        titles.iter().map(|title| title.to_string()).collect(),
        move |answer| sink.borrow_mut().push(answer),
    );
    cx.update(|_, cx| show(cx, toast));
    draw(cx);
    answers
}

/// Presses and releases `keys`, a key at a time: a button clicks on the
/// release.
fn press(cx: &mut VisualTestContext, keys: &str) {
    for key in keys.split_whitespace() {
        cx.simulate_keystrokes(key);
        cx.simulate_event(gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse(key).unwrap(),
        });
        draw(cx);
    }
}

fn present(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.debug_bounds(selector).is_some()
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

fn button_labels(cx: &mut VisualTestContext) -> Vec<String> {
    let nodes = cx.update(|window, cx| fe_ui::nodes(window, cx));
    (nodes.iter())
        .filter(|node| node.role == fe_ui::Role::Button)
        .filter(|node| node.id.to_string().contains("toast-"))
        .map(|node| node.label.to_string())
        .collect()
}

#[gpui::test]
fn three_choices_show_one_button_and_more(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    ask(&["A", "B", "C"], cx);
    assert!(present(cx, "toast-choice-0-0"));
    assert!(!present(cx, "toast-choice-0-1"));
    assert!(!present(cx, "toast-choice-0-2"));
    assert!(present(cx, "toast-more-0"));
    assert!(!present(cx, "toast-more-menu-0"), "closed until asked");
    let labels = button_labels(cx);
    assert_eq!(
        labels.iter().filter(|l| *l == "More").count(),
        1,
        "{labels:?}"
    );
    assert_eq!(
        labels.iter().filter(|l| *l != "Dismiss").count(),
        2,
        "{labels:?}"
    );
}

#[gpui::test]
fn two_choices_need_no_more(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    ask(&["A", "B"], cx);
    assert!(present(cx, "toast-choice-0-0") && present(cx, "toast-choice-0-1"));
    assert!(!present(cx, "toast-more-0"));
}

#[gpui::test]
fn the_menu_lists_the_rest_in_order_and_a_pick_replies(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    let answers = ask(&["A", "B", "C", "D"], cx);
    click(cx, "toast-more-0");
    assert!(present(cx, "toast-more-menu-0"));
    assert!(!present(cx, "toast-more-item-0-0"), "A is a button");
    let (b, c, d) = (
        cx.debug_bounds("toast-more-item-0-1").unwrap(),
        cx.debug_bounds("toast-more-item-0-2").unwrap(),
        cx.debug_bounds("toast-more-item-0-3").unwrap(),
    );
    assert!(b.origin.y < c.origin.y && c.origin.y < d.origin.y);
    assert!(answers.borrow().is_empty());
    click(cx, "toast-more-item-0-2");
    assert_eq!(*answers.borrow(), [Some(2)]);
    assert!(!present(cx, "toast-0"), "the toast goes");
}

#[gpui::test]
fn the_first_button_still_replies(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    let answers = ask(&["A", "B", "C"], cx);
    click(cx, "toast-choice-0-0");
    assert_eq!(*answers.borrow(), [Some(0)]);
}

#[gpui::test]
fn more_is_in_the_tab_order_and_its_menu_opens_from_the_keyboard(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    let answers = ask(&["A", "B", "C"], cx);
    let toasts = cx.windows()[0]
        .downcast::<Workbench>()
        .unwrap()
        .entity(cx)
        .unwrap()
        .read_with(cx, |workbench, _| workbench.toasts().clone());
    cx.update(|window, cx| {
        toasts.update(cx, |layer, cx| layer.focus_newest(window, cx));
    });
    draw(cx);
    // A, then More.
    press(cx, "tab enter");
    assert!(present(cx, "toast-more-menu-0"), "Enter on More opens it");
    // The menu's B and C follow More in the tab order.
    press(cx, "tab tab enter");
    assert_eq!(*answers.borrow(), [Some(2)]);
}

#[gpui::test]
fn more_has_an_accessible_name(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    ask(&["A", "B", "C"], cx);
    let nodes = cx.update(|window, cx| fe_ui::nodes(window, cx));
    assert!((nodes.iter()).any(|node| node.role == fe_ui::Role::Button
        && node.id.to_string().contains("toast-more")
        && node.label.as_ref() == "More"));
}

fn layer(cx: &mut VisualTestContext) -> gpui::Entity<crate::workbench::toast::ToastLayer> {
    cx.windows()[0]
        .downcast::<Workbench>()
        .unwrap()
        .entity(cx)
        .unwrap()
        .read_with(cx, |workbench, _| workbench.toasts().clone())
}

fn focus_toast(cx: &mut VisualTestContext) {
    let layer = layer(cx);
    cx.update(|window, cx| layer.update(cx, |layer, cx| layer.focus_newest(window, cx)));
    draw(cx);
}

#[gpui::test]
fn opening_more_while_dismiss_is_focused_keeps_focus_on_dismiss(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    let answers = ask(&["A", "B", "C"], cx);
    focus_toast(cx);
    // A, More, Dismiss.
    press(cx, "tab tab");
    click(cx, "toast-more-0");
    assert!(present(cx, "toast-more-menu-0"));
    press(cx, "enter");
    assert_eq!(*answers.borrow(), [None], "Dismiss, not a choice");
    assert!(!present(cx, "toast-0"));
}

#[gpui::test]
fn closing_more_while_an_item_is_focused_moves_focus_to_more(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    let answers = ask(&["A", "B", "C"], cx);
    focus_toast(cx);
    press(cx, "tab enter tab tab");
    assert!(present(cx, "toast-more-menu-0"));
    // On C; the menu closes by a click on More.
    click(cx, "toast-more-0");
    assert!(!present(cx, "toast-more-menu-0"));
    press(cx, "enter");
    assert!(present(cx, "toast-more-menu-0"), "focus was on More");
    assert!(answers.borrow().is_empty());
}

#[gpui::test]
fn opening_another_toasts_menu_keeps_the_focused_button(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    let first = ask(&["A", "B", "C"], cx);
    let second = ask(&["X", "Y", "Z"], cx);
    focus_toast(cx);
    // The newest toast's A, More, Dismiss: Dismiss is focused.
    press(cx, "tab tab");
    click(cx, "toast-more-0");
    press(cx, "enter");
    assert_eq!(*second.borrow(), [None], "Dismiss on the focused toast");
    assert!(first.borrow().is_empty());
}
