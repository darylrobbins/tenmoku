//! A map's `<BS>` on the command line takes the selection out wherever it
//! runs, every edit made while a `<Cmd>`'s keys wait for a command goes in
//! after them, and platform text goes in after all of a map's keys.

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::{cnoremap, draw, line, opened, text};
use crate::Workbench;

fn input(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<fe_ui::TextInput> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, _| bar.line_input().cloned().expect("an input"))
}

fn select_all(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let input = input(workbench, cx);
    input.update(cx, |input, cx| input.select_all(cx));
}

fn commit(workbench: &Entity<Workbench>, string: &str, cx: &mut VisualTestContext) {
    let input = input(workbench, cx);
    input.update_in(cx, |input, window, cx| {
        use gpui::EntityInputHandler as _;
        input.replace_text_in_range(None, string, window, cx)
    });
    draw(cx);
}

/// The selected `ab`, a held `j` whose map is `<BS><BS>x`, and a commit of
/// several characters: the first `<BS>` takes the selection and the second
/// cancels the line, the `x` the editor's.
#[gpui::test]
fn a_settled_backspace_takes_the_selection_out(cx: &mut TestAppContext) {
    let maps = vec![cnoremap("j", "<BS><BS>x"), cnoremap("jk", "Q")];
    let (_dir, workbench, editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(":");
    cx.simulate_input("ab");
    select_all(&workbench, cx);
    cx.simulate_keystrokes("j");
    commit(&workbench, "yz", cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "aa");
}

/// The same after a `<Cmd>`: its keys' `<BS>`s run on a selection too.
#[gpui::test]
fn a_cmd_s_backspace_takes_the_selection_out(cx: &mut TestAppContext) {
    let maps = vec![cnoremap("zz", "<Cmd>s/a/b/<CR><BS><BS>x")];
    let (_dir, workbench, editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(":");
    cx.simulate_input("ab");
    select_all(&workbench, cx);
    cx.simulate_keystrokes("z z");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "aa");
}

/// Platform text committed after a held `j` goes in after all of the
/// map's keys, the keys after its `<Cmd>` included.
#[gpui::test]
fn platform_text_goes_in_after_the_whole_map(cx: &mut TestAppContext) {
    let maps = vec![cnoremap("j", "<Cmd>s/a/b/<CR>x/"), cnoremap("jk", "Q")];
    let (_dir, workbench, editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/");
    cx.simulate_keystrokes("j");
    commit(&workbench, "yz", cx);
    assert_eq!(text(&editor, cx), "baa");
    assert_eq!(line(&workbench, cx), Some((':', "s/a/x/yz".to_owned())));
}

/// A map that closes the line: the platform text is the editor's after the
/// map's keys, not before them.
#[gpui::test]
fn platform_text_follows_a_map_that_closes_the_line(cx: &mut TestAppContext) {
    let maps = vec![cnoremap("j", "<CR>x"), cnoremap("jk", "Q")];
    let (_dir, workbench, editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("j");
    commit(&workbench, "ix", cx);
    assert_eq!(line(&workbench, cx), None);
    // `x` deletes "b", then `i` and `x` are typed: insert mode "x".
    assert_eq!(text(&editor, cx), "xaa");
}

/// Platform text and a menu's paste while a `<Cmd>` waits for the write it
/// ran go in after its keys, none lost.
#[gpui::test]
fn edits_made_while_a_cmd_waits_go_in_after_it(cx: &mut TestAppContext) {
    let (dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("zz", "<Cmd>w<CR>x/")]);
    cx.simulate_keystrokes("x :");
    cx.simulate_input("s/a/");
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("P".into()));
    // Without letting the executor run, so the write is still on its way.
    for key in ["z", "z"] {
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        cx.update(|window, cx| window.dispatch_keystroke(keystroke, cx));
    }
    let input = input(&workbench, cx);
    input.update_in(cx, |input, window, cx| {
        use gpui::EntityInputHandler as _;
        input.replace_text_in_range(None, "yz", window, cx)
    });
    crate::tests::find_bar::run("text_input.paste", cx);
    draw(cx);
    let saved = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
    assert_eq!(saved, "aa");
    assert_eq!(line(&workbench, cx), Some((':', "s/a/x/yzP".to_owned())));
}

/// A selection a `<Cmd>` continuation never consumed doesn't carry over to
/// the next one: its `<BS>` takes a grapheme out of the unselected line.
#[gpui::test]
fn a_continuation_s_selection_does_not_leak_into_the_next(cx: &mut TestAppContext) {
    let maps = vec![
        cnoremap("zz", "<Cmd>s/a/b/<CR>"),
        cnoremap("yy", "<Cmd>s/b/c/<CR><BS>x"),
    ];
    let (_dir, workbench, _editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(":");
    cx.simulate_input("ab");
    select_all(&workbench, cx);
    cx.simulate_keystrokes("z z");
    draw(cx);
    assert_eq!(line(&workbench, cx), Some((':', "ab".to_owned())));
    cx.simulate_keystrokes("y y");
    draw(cx);
    assert_eq!(line(&workbench, cx), Some((':', "ax".to_owned())));
}

/// Platform text with a private-use character, after a map that closes the
/// line, reaches the editor whole.
#[gpui::test]
fn platform_text_keeps_a_private_use_character(cx: &mut TestAppContext) {
    let maps = vec![cnoremap("j", "<CR>i"), cnoremap("jk", "Q")];
    let (_dir, workbench, editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("j");
    commit(&workbench, "\u{F8FF}z", cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "\u{F8FF}zbaa");
}

/// A paste held for a `<Cmd>` breaks a map prefix an earlier held character
/// started: the prefix resolves first, as an ordinary paste's does.
#[gpui::test]
fn a_held_paste_goes_in_after_an_earlier_held_prefix(cx: &mut TestAppContext) {
    let maps = vec![cnoremap("zz", "<Cmd>w<CR>x/"), cnoremap("jk", "Q")];
    let (_dir, workbench, _editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes("x :");
    cx.simulate_input("s/a/");
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("P".into()));
    for key in ["z", "z", "j"] {
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        cx.update(|window, cx| window.dispatch_keystroke(keystroke, cx));
    }
    crate::tests::find_bar::run("text_input.paste", cx);
    draw(cx);
    assert_eq!(line(&workbench, cx), Some((':', "s/a/x/jP".to_owned())));
}

/// Holds `zz` (a `<Cmd>` that waits for a write) and an ambiguous `j`, then
/// commits `commit` and pastes `pasted`, all while the write is on its way.
fn hold_prefix_text_and_paste(
    workbench: &Entity<Workbench>,
    commit_text: &str,
    pasted: &str,
    cx: &mut VisualTestContext,
) {
    cx.simulate_keystrokes("x :");
    cx.simulate_input("s/a/");
    cx.write_to_clipboard(gpui::ClipboardItem::new_string(pasted.into()));
    for key in ["z", "z", "j"] {
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        cx.update(|window, cx| window.dispatch_keystroke(keystroke, cx));
    }
    let input = input(workbench, cx);
    input.update_in(cx, |input, window, cx| {
        use gpui::EntityInputHandler as _;
        input.replace_text_in_range(None, commit_text, window, cx)
    });
    crate::tests::find_bar::run("text_input.paste", cx);
    draw(cx);
}

/// Platform text and a paste held behind an ambiguous prefix go in after
/// the prefix's own `<Cmd>`, in order.
#[gpui::test]
fn held_text_and_paste_wait_for_a_prefix_s_cmd(cx: &mut TestAppContext) {
    let maps = vec![
        cnoremap("zz", "<Cmd>w<CR>x/"),
        cnoremap("j", "<Cmd>w<CR>y/"),
        cnoremap("jk", "Q"),
    ];
    let (_dir, workbench, _editor, cx) = opened(cx, maps);
    hold_prefix_text_and_paste(&workbench, "PQ", "RS", cx);
    cx.executor().advance_clock(super::CHORD_TIMEOUT * 2);
    draw(cx);
    assert_eq!(line(&workbench, cx), Some((':', "s/a/x/y/PQRS".to_owned())));
}

/// A prefix that resolves to a map closing the line takes the held text and
/// paste to the editor, in order, none lost.
#[gpui::test]
fn held_text_and_paste_follow_a_prefix_that_closes_the_line(cx: &mut TestAppContext) {
    let maps = vec![
        cnoremap("zz", "<Cmd>w<CR>x/"),
        cnoremap("j", "<CR>"),
        cnoremap("jk", "Q"),
    ];
    let (_dir, workbench, editor, cx) = opened(cx, maps);
    hold_prefix_text_and_paste(&workbench, "xx", "x", cx);
    cx.executor().advance_clock(super::CHORD_TIMEOUT * 2);
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    // `s/a/x/` made `xaa`; the three `x`s took them all out, none lost.
    assert_eq!(text(&editor, cx), "");
}
