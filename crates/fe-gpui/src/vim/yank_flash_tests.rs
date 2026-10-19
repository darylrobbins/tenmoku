//! A yank flashes its text: a highlight decoration over it for a moment.

use std::time::Duration;

use fe_editor::decorations::DecorationKind;

use super::*;

fn highlights(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<std::ops::Range<usize>> {
    view.read_with(cx, |view, _| {
        let len = view.model().text().len();
        view.model()
            .decorations(0..len)
            .into_iter()
            .filter(|decoration| decoration.kind == DecorationKind::Highlight)
            .map(|decoration| decoration.range)
            .collect()
    })
}

#[gpui::test]
fn a_yank_flashes_its_text(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "one two\nthree");

    cx.simulate_keystrokes("w y i w");
    assert_eq!(highlights(&view, cx), vec![4..7]);
    cx.executor().advance_clock(Duration::from_millis(149));
    cx.run_until_parked();
    assert_eq!(highlights(&view, cx), vec![4..7]);
    cx.executor().advance_clock(Duration::from_millis(2));
    cx.run_until_parked();
    assert_eq!(highlights(&view, cx), Vec::<std::ops::Range<usize>>::new());

    // The next edit ends it.
    cx.simulate_keystrokes("y i w");
    assert_eq!(highlights(&view, cx), vec![4..7]);
    cx.simulate_keystrokes("x");
    assert_eq!(highlights(&view, cx), Vec::<std::ops::Range<usize>>::new());

    // A second yank replaces it.
    cx.simulate_keystrokes("0 y i w");
    assert_eq!(highlights(&view, cx), vec![0..3]);
    cx.simulate_keystrokes("j y y");
    let row = text(&view, cx).find("three").unwrap();
    assert_eq!(highlights(&view, cx), vec![row..row + 5]);

    // A delete shows none.
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    cx.simulate_keystrokes("d d");
    assert_eq!(highlights(&view, cx), Vec::<std::ops::Range<usize>>::new());
}
