//! A bracket object typed in charwise visual mode over a selection of more
//! than one position picks the pair from the selection's start, as nvim
//! does, whether or not it holds the selection. Each expectation was
//! probed with `nvim --clean --headless` (0.12.6, the oracle's settings).

use fe_vim::HeadlessVim;

type At = (u32, usize);

/// Feeds `keys` and checks the anchor, cursor, mode and failed flag.
fn check(buffer: &str, cursor: At, keys: &str, anchor: At, at: At, failed: bool) {
    let mut vim = HeadlessVim::new(buffer, cursor);
    vim.feed(keys).unwrap();
    assert_eq!(
        (
            vim.visual_anchor(),
            vim.cursor(),
            vim.mode(),
            vim.state().failed()
        ),
        (Some(anchor), at, "visual", failed),
        "{keys:?} on {buffer:?} at {cursor:?}"
    );
}

fn row(buffer: &str, col: usize, keys: &str, anchor: usize, at: usize, failed: bool) {
    check(buffer, (0, col), keys, (0, anchor), (0, at), failed);
}

#[test]
fn brackets_over_a_selection_match_nvim() {
    row("a (b c) d", 0, "vllli(", 3, 5, false);
    row("a (b c) d", 0, "vlllla(", 2, 6, false);
    row("a (b c) d", 6, "vli(", 3, 5, false);
    row("a (b c) d", 7, "vhhi(", 3, 5, false);
    row("a (b c) d", 3, "vlllla(", 2, 6, false);
    row("a (b c) d", 4, "vlli(", 3, 5, false);
    row("a (b c) d", 2, "vla(", 2, 3, true);
    row("a (b c) d", 3, "vha(", 3, 2, true);
    row("f(x (y z) w) q", 6, "va(i(", 2, 10, false);
    row("f(x (y z) w) q", 0, "va(a(", 4, 8, false);
    row("f(x (y z) w) q", 0, "vl2i(", 5, 7, false);
    row("(a (b) c)", 0, "vli(", 4, 4, false);
    row("a (b) (c)", 3, "vlllli(", 3, 3, false);
    row("a [b c] d", 0, "vlli[", 3, 5, false);
    row("a [b c] d", 2, "vla[", 2, 3, true);
    row("a {b c} d", 3, "vhhhi{", 3, 5, false);
    row("a <b c> d", 0, "vli>", 3, 5, false);
    check("xy\n(a b)", (0, 0), "vli(", (1, 1), (1, 3), false);
    check("(a\nb) (c d)", (0, 0), "jvli(", (0, 1), (1, 0), false);
}

#[test]
fn brackets_nvim_already_matched_stay() {
    row("a (b c) d", 3, "vlllli(", 3, 7, true);
    row("a (b c) d", 3, "vl2i(", 3, 4, true);
    row("a (b c) d", 4, "vlla(", 2, 6, false);
    row("(ab) (cd)", 0, "vli(", 0, 1, true);
    row("f(x (y z) w) q", 5, "vl2i(", 2, 10, false);
    row("f(x (y z) w) q", 6, "vi(i(", 2, 10, false);
    row("[[a]]", 2, "va[a[", 0, 4, false);
    row("x [ab cd] y", 4, "viwi[", 3, 7, false);
}
