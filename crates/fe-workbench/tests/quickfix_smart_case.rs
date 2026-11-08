//! `:vimgrep`'s smart case, as ripgrep's `--smart-case` has it: an uppercase
//! literal, including one a hex escape decodes to, makes the search case
//! sensitive; a class such as `\p{Lu}` or `[[:upper:]]` doesn't.

use std::{fs, path::Path};

use fe_gpui::{EditorSettings, EditorTheme, set_vim_enabled};
use fe_workbench::{Launch, Workbench};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn window(cx: &mut TestAppContext, root: &Path) -> (Entity<Workbench>, VisualTestContext) {
    fs::write(root.join("a.txt"), "1\na\nA\n").unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let cx = &mut vcx;
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = root.join("a.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    (workbench, vcx)
}

/// The row the caret lands on after `:vimgrep {pattern}` jumps to its first
/// match in `a.txt`, whose rows are `1`, `a` and `A`.
fn first_match_row(pattern: &str) -> u32 {
    let mut row = 0;
    let pattern = pattern.to_owned();
    // A fresh window per pattern: the caret starts on row 0 each time.
    let mut cx = TestAppContext::single();
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = window(&mut cx, dir.path());
    let cx = &mut vcx;
    cx.simulate_keystrokes(":");
    cx.simulate_input(&format!("vimgrep /{pattern}/ %"));
    cx.simulate_keystrokes("enter");
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a tab is shown");
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let head = model.primary_selection().head();
        row = model.snapshot().text().offset_to_point(head).row;
    });
    row
}

#[test]
fn a_hex_escape_to_an_uppercase_letter_is_case_sensitive() {
    for pattern in [r"\x41", r"\x{41}", "A"] {
        assert_eq!(first_match_row(pattern), 2, "{pattern}");
    }
}

#[test]
fn a_class_stays_case_insensitive() {
    for pattern in ["[[:upper:]]", r"\p{Lu}"] {
        assert_eq!(first_match_row(pattern), 1, "{pattern}");
    }
}
