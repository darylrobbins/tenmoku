//! A click on a Match Case, Whole Word or Regex toggle leaves focus where it
//! was.

use gpui::{Modifiers, TestAppContext, VisualTestContext, point, px};

use super::{draw, editor, editor_focused, input_focused, opened, pane, query, run, selection};

fn click_at(at: gpui::Point<gpui::Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

#[gpui::test]
fn clicking_the_regex_toggle_keeps_typing_in_the_input(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo one\nfoo two\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    super::select(&editor, 0..3, cx);
    run("find.show", cx);
    assert_eq!(query(&pane, cx), "foo");

    let bar = cx.debug_bounds("pane-find-bar").expect("drawn");
    click_at(point(bar.right() - px(12.), bar.center().y), cx);
    assert!(pane.read_with(cx, |pane, cx| { pane.find_bar().unwrap().query(cx).regex }));
    assert!(input_focused(&pane, cx));

    cx.simulate_input(" t");
    draw(cx);
    assert_eq!(query(&pane, cx), "foo t");
    run("find.close", cx);
    assert!(!super::open(&pane, cx));
}

#[gpui::test]
fn each_toggle_click_keeps_focus_where_it_was(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo one\nfoo two\nfoo one\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    super::select(&editor, 0..3, cx);
    run("find.show", cx);

    let flags = |cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, cx| {
            let q = pane.find_bar().unwrap().query(cx);
            [q.match_case, q.whole_word, q.regex]
        })
    };
    for ix in 0..3 {
        let toggle = cx
            .debug_bounds(format!("find-toggle:{ix}").leak())
            .expect("drawn");
        click_at(toggle.center(), cx);
        // Earlier toggles were flipped on and stay on.
        let expected = [true, ix >= 1, ix >= 2];
        assert_eq!(flags(cx), expected);
        assert!(input_focused(&pane, cx), "toggle {ix}");
        assert!(!editor_focused(&editor, cx));

        // Typing extends the query; then back to `foo`, which has three
        // matches, so find.next has somewhere to step.
        cx.simulate_input("o");
        draw(cx);
        assert_eq!(query(&pane, cx), "fooo", "toggle {ix}");
        run("text_input.backspace", cx);
        assert_eq!(query(&pane, cx), "foo");
        let before = selection(&editor, cx);
        run("find.next", cx);
        assert_ne!(selection(&editor, cx), before, "toggle {ix}");
    }

    // With the editor focused, a toggle click leaves it focused.
    super::focus(&editor, cx);
    let toggle = cx.debug_bounds("find-toggle:0").expect("drawn");
    click_at(toggle.center(), cx);
    assert!(!flags(cx)[0]);
    assert!(editor_focused(&editor, cx));
    assert!(!input_focused(&pane, cx));
}
