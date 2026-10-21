//! `:normal` inside itself stops at its depth limit on a small stack, however
//! large this platform's frames are.

use fe_editor::commands::HeadlessHost;
use fe_vim::{HeadlessVim, Register, Write};

#[test]
fn nested_normal_fits_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let mut vim = HeadlessVim::new("abc", (0, 0));
            let register = Register::new(":normal @a\n", false);
            vim.registers_mut()
                .write(Some('a'), Write::Yank, register, &mut HeadlessHost::new())
                .unwrap();
            vim.feed("@a").unwrap();
            assert_eq!(vim.text(), "abc");
            assert_eq!(
                vim.state().message(),
                Some("E192: Recursive use of :normal too deep")
            );
        })
        .unwrap()
        .join()
        .expect("no overflow");
}
