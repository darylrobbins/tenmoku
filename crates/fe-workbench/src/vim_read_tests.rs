//! `:r[ead] {file}` on the status bar's line puts the file's lines below
//! the line (`:0r` above the first), as nvim does.

use gpui::TestAppContext;

use super::{active, ex, message, window};

#[gpui::test]
fn read_puts_a_files_lines_below_the_line(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "a\nb\n").unwrap();
    std::fs::write(root.join("f.txt"), "one\ntwo\n").unwrap();
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let a = active(&workbench, cx);
    let state = |cx: &mut gpui::VisualTestContext| {
        a.read_with(cx, |a, _| {
            let model = a.model();
            let point = model
                .text()
                .offset_to_point(model.primary_selection().head());
            (model.text().to_string(), (point.row, point.column))
        })
    };

    ex("r f.txt", cx);
    assert_eq!(state(cx), ("a\none\ntwo\nb\n".to_string(), (1, 0)));
    assert_eq!(message(&workbench, cx).as_deref(), Some("\"f.txt\" 2L, 8B"));

    ex("0r f.txt", cx);
    assert_eq!(
        state(cx),
        ("one\ntwo\na\none\ntwo\nb\n".to_string(), (0, 0))
    );

    // One `u` undoes each.
    cx.simulate_keystrokes("u");
    assert_eq!(state(cx).0, "a\none\ntwo\nb\n");
    cx.simulate_keystrokes("u");
    assert_eq!(state(cx).0, "a\nb\n");

    ex("r nope.txt", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E484: Can't open file nope.txt")
    );
    assert_eq!(state(cx).0, "a\nb\n");
}
