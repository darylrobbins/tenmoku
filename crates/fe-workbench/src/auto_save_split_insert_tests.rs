//! Focus and window auto-saves with one split of a buffer inserting.

use super::*;

#[gpui::test]
fn focus_or_window_save_mid_insert_in_another_split_runs_no_save_fixes(cx: &mut TestAppContext) {
    let (dir, a, _) = project();
    let (workbench, cx) = open(cx, dir.path(), settings(Setting::OnFocusChange));
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    open_file(&workbench, &a, cx);
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    let [left, right] = workbench
        .read_with(cx, |workbench, cx| {
            <[_; 2]>::try_from(workbench.open_editors(cx))
        })
        .expect("two editors");
    for editor in [&left, &right] {
        editor.update(cx, |editor, _| {
            editor.set_save_fixes(fe_gpui::SaveFixes {
                trim_trailing_whitespace: true,
                ..Default::default()
            })
        });
    }
    let mode = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.vim_mode())
    };

    // Focus change: the left split in normal mode, the right inserting.
    focus(&right, cx);
    keys("shift-a space space", cx);
    assert_eq!(mode(&right, cx), Some("insert"));
    focus(&left, cx);
    assert_eq!(mode(&right, cx), Some("insert"));
    assert_eq!(text(&right, cx), "one  \n");
    assert_eq!(on_disk(&a), "one  \n");

    // Window change: still inserting in the right split.
    focus(&right, cx);
    keys("space space", cx);
    cx.deactivate_window();
    draw(cx);
    assert_eq!(mode(&right, cx), Some("insert"));
    assert_eq!(text(&right, cx), "one    \n");
    assert_eq!(on_disk(&a), "one    \n");
}
