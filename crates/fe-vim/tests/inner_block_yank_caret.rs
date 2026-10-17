//! A yank of a linewise `i(` `i{` `i[` or `i<` leaves the caret at column 0 of
//! the first inner row, as nvim's does.

use fe_vim::HeadlessVim;

const IF: &str = "  if (x) {\n    y = z;\n  }\n\nelse";
const MAIN: &str = "fn main() {\n    let v = vec![1, 2];\n}\n";

/// Buffer, caret, keys, caret after, register `"` (empty: unchecked), text after.
type Case = (
    &'static str,
    (u32, usize),
    &'static str,
    (u32, usize),
    &'static str,
    Option<&'static str>,
);

#[test]
fn yank_of_a_linewise_inner_block_leaves_the_caret_at_its_first_row_start() {
    let cases: &[Case] = &[
        (IF, (0, 2), "yiB", (1, 0), "    y = z;\n", None),
        (IF, (0, 2), "yi{", (1, 0), "    y = z;\n", None),
        (IF, (1, 6), "yiB", (1, 0), "    y = z;\n", None),
        (
            MAIN,
            (1, 21),
            "yiBw",
            (1, 4),
            "    let v = vec![1, 2];\n",
            None,
        ),
        (
            "f (\n  a,\n  b\n)",
            (2, 2),
            "yib",
            (1, 0),
            "  a,\n  b\n",
            None,
        ),
        (
            "f [\n  a,\n  b\n]",
            (2, 2),
            "yi[",
            (1, 0),
            "  a,\n  b\n",
            None,
        ),
        (
            "f <\n  a,\n  b\n>",
            (2, 2),
            "yi<",
            (1, 0),
            "  a,\n  b\n",
            None,
        ),
        (
            "f (\n  a,\n  b\n)",
            (0, 0),
            "yib",
            (1, 0),
            "  a,\n  b\n",
            None,
        ),
        // Unchanged.
        (
            IF,
            (1, 6),
            "diB",
            (1, 2),
            "    y = z;\n",
            Some("  if (x) {\n  }\n\nelse"),
        ),
        (
            IF,
            (1, 6),
            "gUiB",
            (1, 0),
            "",
            Some("  if (x) {\n    Y = Z;\n  }\n\nelse"),
        ),
        (
            IF,
            (1, 6),
            ">iB",
            (1, 6),
            "",
            Some("  if (x) {\n        y = z;\n  }\n\nelse"),
        ),
        (IF, (1, 6), "viBy", (1, 0), "    y = z;\n", None),
        (IF, (1, 6), "yaB", (0, 9), "{\n    y = z;\n  }", None),
        (
            IF,
            (1, 6),
            "yip",
            (0, 0),
            "  if (x) {\n    y = z;\n  }\n",
            None,
        ),
        (IF, (1, 6), "yj", (1, 6), "    y = z;\n  }\n", None),
    ];
    for &(buffer, caret, keys, after, register, text) in cases {
        let mut vim = HeadlessVim::new(buffer, caret);
        vim.feed(keys).unwrap();
        assert_eq!(
            vim.cursor(),
            after,
            "caret of {keys} on {buffer:?} at {caret:?}"
        );
        assert_eq!(vim.text(), text.unwrap_or(buffer), "text of {keys}");
        if !register.is_empty() {
            let got = vim.registers().get('"').unwrap().text.clone();
            assert_eq!(got, register, "register of {keys} on {buffer:?}");
        }
    }
}
