//! Tab on the vim line's `:` completes the command's name and cycles
//! through the others, shift-Tab back.

use super::{line, opened, steps};

#[gpui::test]
fn tab_completes_and_cycles_the_command_name(cx: &mut gpui::TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx);
    cx.simulate_keystrokes(":");
    cx.simulate_input("noh");
    steps(&workbench, cx, ':', &[("tab", "nohlsearch")]);
    cx.simulate_keystrokes("escape");

    // Several completions: in order, then what was typed, and back.
    cx.simulate_keystrokes(":");
    cx.simulate_input("s");
    cx.simulate_keystrokes("tab");
    let first = line(&workbench, cx).unwrap().1;
    cx.simulate_keystrokes("tab");
    let second = line(&workbench, cx).unwrap().1;
    assert!(first.starts_with('s') && first.len() > 1, "{first}");
    assert!(second.starts_with('s') && second != first, "{second}");
    let mut texts = vec![first.clone(), second];
    loop {
        cx.simulate_keystrokes("tab");
        let text = line(&workbench, cx).unwrap().1;
        if text == "s" {
            break;
        }
        assert!(texts.len() < 20, "never returned to the typed text");
        texts.push(text);
    }
    // Wraps to the first again, and shift-tab walks back.
    steps(&workbench, cx, ':', &[("tab", &first), ("shift-tab", "s")]);
    for text in texts.iter().rev() {
        cx.simulate_keystrokes("shift-tab");
        assert_eq!(line(&workbench, cx), Some((':', text.clone())));
    }
    cx.simulate_keystrokes("escape");

    // A range is kept.
    cx.simulate_keystrokes(":");
    cx.simulate_input("'<,'>noh");
    steps(&workbench, cx, ':', &[("tab", "'<,'>nohlsearch")]);
}

#[gpui::test]
fn typing_ends_the_cycle_and_search_lines_keep_tab(cx: &mut gpui::TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx);
    cx.simulate_keystrokes(":");
    cx.simulate_input("noh");
    steps(&workbench, cx, ':', &[("tab", "nohlsearch")]);
    // Backspace then tab completes the shortened text afresh.
    cx.simulate_keystrokes("backspace backspace");
    assert_eq!(line(&workbench, cx), Some((':', "nohlsear".into())));
    steps(
        &workbench,
        cx,
        ':',
        &[("tab", "nohlsearch"), ("tab", "nohlsear")],
    );
    cx.simulate_keystrokes("escape");

    cx.simulate_keystrokes("/");
    cx.simulate_input("noh");
    // As before: the input takes the tab as text.
    cx.simulate_keystrokes("tab");
    assert_eq!(line(&workbench, cx), Some(('/', "noh\t".into())));
}
