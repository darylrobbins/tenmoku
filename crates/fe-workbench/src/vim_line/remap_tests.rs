//! `cnoremap` on the status bar's line: each character typed there is asked
//! of the `c` maps, a held prefix resolving after `CHORD_TIMEOUT`, on blur,
//! Enter and Esc; and a recording has the line as typed.

use fe_gpui::{
    EditorSettings, EditorTheme, EditorView, keymap::CHORD_TIMEOUT, set_vim_enabled, set_vim_remaps,
};
use fe_vim::{Remap, RemapMode};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench};

fn cnoremap(from: &str, to: &str) -> Remap {
    Remap {
        modes: vec![RemapMode::CmdLine],
        from: from.to_string(),
        to: to.to_string(),
        recursive: false,
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A workbench showing a file holding `aaa`, focused, with vim on and
/// `remaps` set.
fn opened(
    cx: &mut TestAppContext,
    remaps: Vec<Remap>,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "aaa").unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &remaps, ","));
    assert_eq!(issues, []);
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a tab is shown");
    (dir, workbench, editor, cx)
}

fn line(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(char, String)> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, cx| bar.line(cx))
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

fn time_out(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(CHORD_TIMEOUT * 2);
    draw(cx);
}

fn typed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    line(workbench, cx).expect("a line is open").1
}

#[gpui::test]
fn a_map_to_enter_submits_the_line(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("j");
    assert_eq!(typed(&workbench, cx), "s/a/b/", "j is held");
    cx.simulate_keystrokes("k");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "baa");
}

#[gpui::test]
fn a_key_that_breaks_the_prefix_types_both(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("j x");
    draw(cx);
    assert_eq!(typed(&workbench, cx), "s/a/b/jx");
}

#[gpui::test]
fn a_map_that_types_text_inserts_it(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "s/a/b/")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("j k");
    draw(cx);
    assert_eq!(typed(&workbench, cx), "s/a/b/");
}

#[gpui::test]
fn a_held_prefix_types_after_the_timeout(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("j");
    assert_eq!(typed(&workbench, cx), "");
    time_out(cx);
    assert_eq!(typed(&workbench, cx), "j");
}

#[gpui::test]
fn enter_types_a_held_prefix_first(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("gx", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("g enter");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(
        text(&editor, cx),
        "bbb",
        "the g was a flag of the submitted line"
    );
}

#[gpui::test]
fn escape_types_a_held_prefix_into_the_history(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("j escape");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    cx.simulate_keystrokes(": up");
    assert_eq!(typed(&workbench, cx), "j");
}

#[gpui::test]
fn blur_resolves_a_held_prefix_and_cancels(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("j");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    // Nothing is still held: the editor's `j` moves down, not on a map.
    cx.simulate_keystrokes("j");
    time_out(cx);
    assert_eq!(line(&workbench, cx), None);
}

#[gpui::test]
fn backspace_breaks_a_held_prefix_and_is_not_mapped(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("ab");
    cx.simulate_keystrokes("j backspace");
    draw(cx);
    assert_eq!(typed(&workbench, cx), "ab");
}

#[gpui::test]
fn a_line_without_maps_types_as_before(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("jk");
    assert_eq!(typed(&workbench, cx), "jk");
}

#[gpui::test]
fn a_recording_across_a_mapped_line_replays_identically(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes("q a :");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("j k q");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "baa");
    // The jk was recorded once, as typed.
    cx.simulate_keystrokes("@ a");
    draw(cx);
    assert_eq!(text(&editor, cx), "bba");
    assert_eq!(line(&workbench, cx), None);
}

#[gpui::test]
fn overlapping_prefixes_resolve_one_after_another(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) =
        opened(cx, vec![cnoremap("abc", "X"), cnoremap("bc", "Y")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("a b");
    time_out(cx);
    assert_eq!(typed(&workbench, cx), "ab");
}

#[gpui::test]
fn enter_resolves_overlapping_prefixes_first(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("sg", "X"), cnoremap("gg", "Y")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("s g enter");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    let _ = editor;
}

#[gpui::test]
fn a_mapped_line_records_a_backspace(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes("q a :");
    cx.simulate_input("s/a/b/x");
    cx.simulate_keystrokes("backspace enter q");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "baa");
    cx.simulate_keystrokes("@ a");
    draw(cx);
    assert_eq!(text(&editor, cx), "bba");
}

/// An edit the maps never saw, a `<BS>` before the map's keys, is recorded
/// before them: a replay takes the `x` out before `jk` submits.
#[gpui::test]
fn a_bypassed_edit_is_recorded_before_the_mapping_keys(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes("q a :");
    cx.simulate_input("s/a/b/x");
    cx.simulate_keystrokes("backspace j k q");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "baa");
    cx.simulate_keystrokes("@ a");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "bba");
}

#[gpui::test]
fn keys_after_a_map_s_enter_run_in_normal_mode(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>x")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("j k");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    // `x` deleted the character under the caret, in normal mode.
    assert_eq!(text(&editor, cx).len(), 2, "{}", text(&editor, cx));
}

#[gpui::test]
fn a_cmd_map_runs_its_command(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("zz", "<Cmd>s/a/b/<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("z z");
    draw(cx);
    assert_eq!(text(&editor, cx), "baa");
    // As in nvim, a `<Cmd>` leaves the command line open.
    assert_eq!(line(&workbench, cx), Some((':', String::new())));
}
