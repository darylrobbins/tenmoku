//! 'incsearch' through the line: typing a `/` search shows the match it
//! would land on in its editor and scrolls to it, `escape` puts the view
//! and the earlier highlight back, and `enter` lands there.

use super::*;

/// `r0` to `r299`, one a row, with `world` on row 250.
fn rows() -> String {
    (0..300)
        .map(|row| match row {
            250 => "world\n".to_string(),
            row => format!("r{row}\n"),
        })
        .collect()
}

/// The editor's previewed match, its scroll, and the pattern its matches
/// paint by.
fn shown(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Option<std::ops::Range<usize>>, f32, Option<String>) {
    draw(cx);
    editor.read_with(cx, |editor, _| {
        (
            editor.search_preview(),
            editor.scroll_top(),
            editor
                .match_pattern()
                .map(|pattern| pattern.regex_str().to_string()),
        )
    })
}

#[gpui::test]
fn typing_a_search_reveals_and_escape_restores(cx: &mut TestAppContext) {
    let text = rows();
    let world = text.find("world").unwrap();
    let (_dir, workbench, editor, cx) = opened_files(cx, &[("a.txt", &text)]);
    // An earlier search to highlight, then back to the top.
    submit("/", "r1", cx);
    cx.simulate_keystrokes("g g");
    let (_, top, earlier) = shown(&editor, cx);
    assert_eq!(top, 0.);
    assert!(earlier.is_some());
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.shows_buffer_row(250)),
        Some(false)
    );

    // Each edit previews: the match below the view is the current one, in
    // view, the caret staying.
    cx.simulate_keystrokes("/");
    cx.simulate_input("wor");
    assert_eq!(line(&workbench, cx), Some(('/', "wor".into())));
    let (found, top, pattern) = shown(&editor, cx);
    assert_eq!(found, Some(world..world + 3));
    assert!(top > 0.);
    assert_ne!(pattern, earlier);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.shows_buffer_row(250)),
        Some(true)
    );
    assert_eq!(caret(&editor, cx), (0, 0));
    // An edit that finds nothing puts the view back; one that finds it
    // again shows it.
    cx.simulate_input("x");
    assert_eq!(shown(&editor, cx), (None, 0., earlier.clone()));
    cx.simulate_keystrokes("backspace");
    assert_eq!(shown(&editor, cx).0, Some(world..world + 3));

    // Escape: the view and the earlier highlight as they were.
    cx.simulate_keystrokes("escape");
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(shown(&editor, cx), (None, 0., earlier.clone()));
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.shows_buffer_row(250)),
        Some(false)
    );
    assert_eq!(caret(&editor, cx), (0, 0));

    // Up recalls a line, which previews too.
    cx.simulate_keystrokes("/ up");
    assert_eq!(line(&workbench, cx), Some(('/', "wor".into())));
    assert_eq!(shown(&editor, cx).0, Some(world..world + 3));
    cx.simulate_keystrokes("escape");

    // Enter lands on the match, still in view.
    cx.simulate_keystrokes("/");
    cx.simulate_input("wor");
    let (_, top, _) = shown(&editor, cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(caret(&editor, cx), (250, 0));
    let (found, landed, pattern) = shown(&editor, cx);
    assert_eq!((found, landed), (None, top));
    assert_ne!(pattern, earlier);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.shows_buffer_row(250)),
        Some(true)
    );
}

/// With the find bar left open, a typed search's matches paint in place of
/// its query's until the line closes.
#[gpui::test]
fn a_typed_search_paints_over_an_open_find_bar(cx: &mut TestAppContext) {
    let text = rows();
    let world = text.find("world").unwrap();
    let (_dir, workbench, editor, cx) = opened_files(cx, &[("a.txt", &text)]);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let bar_open = |cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, _| {
            pane.find_bar().is_some_and(|bar| bar.is_open())
        })
    };
    cx.simulate_keystrokes("cmd-f");
    cx.simulate_input("r1");
    let (_, _, find) = shown(&editor, cx);
    assert!(bar_open(cx));
    assert!(find.is_some());
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_keystrokes("g g");
    assert!(bar_open(cx));

    cx.simulate_keystrokes("/");
    cx.simulate_input("wor");
    let (found, _, pattern) = shown(&editor, cx);
    assert_eq!(found, Some(world..world + 3));
    assert_ne!(pattern, find);
    cx.simulate_keystrokes("escape");
    assert_eq!(shown(&editor, cx), (None, 0., find));
    assert!(bar_open(cx));
}
