//! Text on the command line that no typed key made (a paste, a history
//! recall, a completion, an IME commit) is unmapped input: it goes in, in
//! order, between the typed keys, and no `cnoremap` touches it.

use gpui::{ClipboardItem, Entity, TestAppContext, VisualTestContext};

use super::{cnoremap, draw, line, opened, text, time_out, typed};
use crate::{Workbench, tests::find_bar::run};

fn paste(string: &str, cx: &mut VisualTestContext) {
    cx.write_to_clipboard(ClipboardItem::new_string(string.into()));
    run("text_input.paste", cx);
    draw(cx);
}

fn input(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<fe_ui::TextInput> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, _| bar.line_input().cloned().expect("an input"))
}

#[gpui::test]
fn pasted_text_is_not_mapped(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("a", "b")]);
    cx.simulate_keystrokes(":");
    paste("s/a/x/", cx);
    assert_eq!(typed(&workbench, cx), "s/a/x/");
}

#[gpui::test]
fn a_paste_after_a_held_prefix_follows_it(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("j");
    paste("a", cx);
    assert_eq!(typed(&workbench, cx), "ja");
}

#[gpui::test]
fn multi_char_platform_input_applies_before_a_held_prefix(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("j");
    let input = input(&workbench, cx);
    input.update_in(cx, |input, window, cx| {
        use gpui::EntityInputHandler as _;
        input.replace_text_in_range(None, "xy", window, cx)
    });
    time_out(cx);
    assert_eq!(typed(&workbench, cx), "jxy");
}

/// A recall while a cmap prefix is pending lands before the later mapped
/// `<CR>`: in the line, and in the recording a replay reads.
#[gpui::test]
fn a_recall_lands_before_a_later_mapped_enter(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("gx", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(text(&editor, cx), "baa");
    cx.simulate_keystrokes("q q : up g x q");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "bba");
    cx.simulate_keystrokes("@ q");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "bbb");
}

#[gpui::test]
fn a_held_prefix_goes_in_before_a_recall(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("sx", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("enter : s");
    assert_eq!(typed(&workbench, cx), "");
    cx.simulate_keystrokes("up");
    assert_eq!(typed(&workbench, cx), "s/a/b/");
}

fn compose(
    workbench: &Entity<Workbench>,
    marked: &str,
    committed: &str,
    cx: &mut VisualTestContext,
) {
    let input = input(workbench, cx);
    input.update_in(cx, |input, window, cx| {
        use gpui::EntityInputHandler as _;
        input.replace_and_mark_text_in_range(None, marked, None, window, cx);
        input.replace_text_in_range(None, committed, window, cx);
    });
    draw(cx);
}

#[gpui::test]
fn a_composition_goes_in_after_a_held_prefix_unmapped(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) =
        opened(cx, vec![cnoremap("jk", "<CR>"), cnoremap("a", "b")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("j");
    compose(&workbench, "a", "a", cx);
    time_out(cx);
    assert_eq!(typed(&workbench, cx), "ja");
}

#[gpui::test]
fn a_recorded_composition_replays_unmapped(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("a", "b")]);
    cx.simulate_keystrokes("q q :");
    compose(&workbench, "s/a/x/", "s/a/x/", cx);
    cx.simulate_keystrokes("enter q");
    draw(cx);
    assert_eq!(text(&editor, cx), "xaa");
    cx.simulate_keystrokes("@ q");
    draw(cx);
    assert_eq!(text(&editor, cx), "xxa");
}

/// A key the line ignores in a map's right side does the same on the live
/// line and in a replay.
#[gpui::test]
fn a_map_s_ignored_key_is_ignored_in_a_replay_too(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("zz", "<Left>s/a/b/<CR>")]);
    cx.simulate_keystrokes("q q : z z q");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "baa");
    cx.simulate_keystrokes("@ q");
    draw(cx);
    assert_eq!(text(&editor, cx), "bba");
}

#[gpui::test]
fn a_recorded_paste_replays_unmapped(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("a", "b")]);
    cx.simulate_keystrokes("q q :");
    paste("s/a/x/", cx);
    cx.simulate_keystrokes("enter q");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "xaa");
    cx.simulate_keystrokes("@ q");
    draw(cx);
    assert_eq!(text(&editor, cx), "xxa");
}

fn replace(
    workbench: &Entity<Workbench>,
    range: Option<std::ops::Range<usize>>,
    string: &str,
    cx: &mut VisualTestContext,
) {
    let input = input(workbench, cx);
    input.update_in(cx, |input, window, cx| {
        use gpui::EntityInputHandler as _;
        input.replace_text_in_range(range, string, window, cx)
    });
    draw(cx);
}

fn bs_maps() -> Vec<fe_vim::Remap> {
    vec![cnoremap("j", "<BS>x"), cnoremap("jk", "<CR>")]
}

/// A held key whose map edits the line (`j` is `<BS>x`, `jk` is `<CR>`)
/// settles before a composition starts, so the composition isn't cleared.
#[gpui::test]
fn a_held_edit_settles_before_a_composition_starts(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, bs_maps());
    cx.simulate_keystrokes(":");
    cx.simulate_input("a");
    cx.simulate_keystrokes("j");
    compose(&workbench, "b", "b", cx);
    time_out(cx);
    assert_eq!(typed(&workbench, cx), "xb");
}

#[gpui::test]
fn a_held_edit_settles_before_a_multi_char_commit(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, bs_maps());
    cx.simulate_keystrokes(":");
    cx.simulate_input("a");
    cx.simulate_keystrokes("j");
    replace(&workbench, None, "yz", cx);
    time_out(cx);
    assert_eq!(typed(&workbench, cx), "xyz");
}

#[gpui::test]
fn explicit_range_edits_follow_a_held_prefix(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("j");
    replace(&workbench, Some(0..0), "xy", cx);
    time_out(cx);
    assert_eq!(typed(&workbench, cx), "jxy", "an insertion");
}

#[gpui::test]
fn an_explicit_range_replacement_follows_a_held_prefix(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("ab");
    cx.simulate_keystrokes("j");
    replace(&workbench, Some(0..1), "Z", cx);
    time_out(cx);
    assert_eq!(typed(&workbench, cx), "Zbj", "a replacement");
}

#[gpui::test]
fn an_explicit_range_deletion_follows_a_held_prefix(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("ab");
    cx.simulate_keystrokes("j");
    replace(&workbench, Some(1..2), "", cx);
    time_out(cx);
    assert_eq!(typed(&workbench, cx), "aj", "a deletion");
}

/// The recording's literal marker isn't `<C-v>`: a map whose left side
/// ends in `<C-v>` doesn't take it, so pasted text after a held `j` replays
/// as it was typed.
#[gpui::test]
fn the_literal_marker_does_not_match_a_map(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("j<C-v>", "X")]);
    cx.simulate_keystrokes("q q :");
    cx.simulate_keystrokes("j");
    paste("a", cx);
    cx.simulate_keystrokes("escape q");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    cx.simulate_keystrokes("@ q");
    draw(cx);
    cx.simulate_keystrokes(": up");
    assert_eq!(typed(&workbench, cx), "ja");
}

/// A `<C-v>` in a map's right side is ignored by the live line, so a replay
/// ignores it too.
#[gpui::test]
fn a_map_s_ctrl_v_is_ignored_live_and_in_a_replay(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("zz", "<C-v>a")]);
    cx.simulate_keystrokes("q q : z z escape q");
    draw(cx);
    cx.simulate_keystrokes(": up");
    assert_eq!(typed(&workbench, cx), "a");
    cx.simulate_keystrokes("escape @ q");
    draw(cx);
    cx.simulate_keystrokes(": up");
    assert_eq!(typed(&workbench, cx), "a");
}
