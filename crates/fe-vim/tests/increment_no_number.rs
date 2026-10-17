//! `<C-a>` and `<C-x>` that find no number are still the change `.` repeats,
//! and over a selection they do not fail, so a macro goes on. Each
//! expectation was probed with `nvim --clean --headless`.

use fe_vim::HeadlessVim;

fn run(buffer: &str, keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(buffer, (0, 0));
    vim.feed(keys).unwrap();
    vim
}

fn check(buffer: &str, keys: &str, text: &str) {
    assert_eq!(run(buffer, keys).text(), text, "{keys:?} on {buffer:?}");
}

#[test]
fn increment_that_finds_no_number_repeats_and_keeps_a_macro_going() {
    let rows = "ab\ncd\nef\ngh\nij";
    for keys in ["qav<C-a>xjq2@a", "qaVg<C-x>xjq2@a"] {
        let vim = run(rows, keys);
        assert_eq!(vim.text(), "b\nd\nf\ngh\nij", "{keys:?}");
        assert_eq!(vim.cursor(), (3, 0), "{keys:?}");
    }
    let nums = "ab\n5\n7\n9";
    check(nums, "v<C-a>j.", "ab\n6\n7\n9");
    check(nums, "vl3<C-x>j.", "ab\n2\n7\n9");
    check(nums, "3<C-x>j.", "ab\n2\n7\n9");
    check(nums, "<C-a>j.", "ab\n6\n7\n9");
    check(nums, "x<C-a>j.", "b\n6\n7\n9");
    assert!(!run("abc def", "v$<C-a>").state().failed());
    // Unchanged.
    assert!(run("abc def", "<C-a>").state().failed());
    check(nums, "Vj<C-a>jj.", "ab\n6\n8\n10");
}
