//! The pickers' vim switch: once [`fe_workbench::init`] has run,
//! kiln-shell's pickers take vim keys while fastedit's vim is on.

use gpui::TestAppContext;

#[gpui::test]
fn the_pickers_follow_the_vim_switch(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
        assert!(!kiln_shell::picker::vim_keys(cx), "vim starts off");
        fe_gpui::set_vim_enabled(cx, true);
        assert!(kiln_shell::picker::vim_keys(cx));
        fe_gpui::set_vim_enabled(cx, false);
        assert!(!kiln_shell::picker::vim_keys(cx));
    });
}
