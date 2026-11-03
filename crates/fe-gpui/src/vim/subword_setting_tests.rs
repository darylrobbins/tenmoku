//! `vim.subword_motions` reaching the editors' vim state.

use super::jump_setting_tests::{open, text_of};
use super::*;
use crate::init;
use gpui::TestAppContext;

#[gpui::test]
fn the_setting_reaches_every_editor(cx: &mut TestAppContext) {
    cx.update(init);
    let (first, cx) = open(cx, "fooBar baz");
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();

    // `w` on fooBar goes to the next word until the setting is on.
    cx.simulate_keystrokes("w i X escape");
    assert_eq!(text_of(&first, cx), "fooBar Xbaz");
    cx.simulate_keystrokes("u 0");

    cx.update(|_, cx| set_vim_subword_motions(cx, true));
    cx.simulate_keystrokes("w i X escape");
    assert_eq!(text_of(&first, cx), "fooXBar baz");

    // An editor opened afterwards has it on too.
    let (second, cx) = open(cx, "fooBar baz");
    cx.simulate_keystrokes("w i X escape");
    assert_eq!(text_of(&second, cx), "fooXBar baz");

    cx.update(|_, cx| set_vim_subword_motions(cx, false));
    cx.simulate_keystrokes("u 0 w i X escape");
    assert_eq!(text_of(&second, cx), "fooBar Xbaz");
}
