//! `vim.replace_with_register` reaching the editors' vim state.

use super::*;

#[gpui::test]
fn the_setting_gives_gr_to_vim(cx: &mut TestAppContext) {
    cx.update(init);
    let (first, cx) = open(cx, "one two\nthree");
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();

    cx.update(|_, cx| set_vim_replace_with_register(cx, true));
    cx.simulate_keystrokes("y i w j g r r");
    assert_eq!(text_of(&first, cx), "one two\none");
    cx.update(|_, cx| {
        let unnamed = vim_registers(cx).unwrap().get('"').unwrap();
        assert_eq!(unnamed.text, "one");
    });

    // An editor opened afterwards has it on too.
    let (second, cx) = open(cx, "four five\nsix");
    cx.simulate_keystrokes("y i w j g r r");
    assert_eq!(text_of(&second, cx), "four five\nfour");

    // Yank a different word, so an enabled `grw` would replace "four".
    cx.simulate_keystrokes("escape k w y i w");
    cx.update(|_, cx| set_vim_replace_with_register(cx, true));
    cx.simulate_keystrokes("j 0 g r w");
    assert_eq!(text_of(&second, cx), "four five\nfive");
    cx.simulate_keystrokes("u");
    assert_eq!(text_of(&second, cx), "four five\nfour");
    cx.update(|_, cx| set_vim_replace_with_register(cx, false));
    cx.simulate_keystrokes("g r w");
    assert_eq!(text_of(&second, cx), "four five\nfour");
}
