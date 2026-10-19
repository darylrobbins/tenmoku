//! `ie ae` (the whole buffer) and `ii ai aI` (an indent level).

use fe_vim::HeadlessVim;

fn run(text: &str, cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    let _ = vim.feed(keys);
    vim
}

fn text_after(text: &str, cursor: (u32, usize), keys: &str) -> String {
    run(text, cursor, keys).text()
}

#[test]
fn entire_objects() {
    let text = "\n\na\n\nb\n\n";
    assert_eq!(text_after(text, (2, 0), "dae"), "");
    assert_eq!(text_after(text, (2, 0), "die"), "\n\n\n");
    assert_eq!(text_after(text, (4, 0), "cieX<Esc>"), "\n\nX\n\n");
    let vim = run(text, (2, 0), "yie");
    assert_eq!(vim.text(), text);
    assert_eq!(
        vim.registers().get('"').map(|r| r.text.clone()),
        Some("a\n\nb\n".to_string())
    );
    // An all-blank buffer: every row.
    assert_eq!(text_after("\n  \n", (0, 0), "die"), "");
    let vim = run(text, (2, 0), "vae");
    assert_eq!(
        (vim.mode(), vim.visual_anchor(), vim.cursor()),
        ("visual_line", Some((0, 0)), (6, 0))
    );
    let vim = run(text, (3, 0), "vie");
    assert_eq!(
        (vim.mode(), vim.visual_anchor(), vim.cursor()),
        ("visual_line", Some((2, 0)), (4, 0))
    );
}

#[test]
fn indent_objects() {
    let text = "a\n  b\n  c\n\n  d\n\n\ne\n  f\n";
    // The level around `b`: blank rows between rows, none after.
    assert_eq!(text_after(text, (1, 2), "dii"), "a\n\n\ne\n  f\n");
    assert_eq!(text_after(text, (1, 2), "dai"), "\n\ne\n  f\n");
    assert_eq!(text_after("a\n  b\nc\n", (1, 2), "daI"), "");
    assert_eq!(text_after("x\na\n  b\n\nc\n", (2, 2), "daI"), "x\nc\n");
    // A count takes levels out.
    let nested = "a\n  b\n    c\n    d\n  e\nf\n";
    assert_eq!(text_after(nested, (2, 4), "dii"), "a\n  b\n  e\nf\n");
    assert_eq!(text_after(nested, (2, 4), "2dii"), "a\nf\n");
    // On a blank row, the next non-blank row's level.
    assert_eq!(text_after(text, (3, 0), "dii"), "a\n\n\ne\n  f\n");
    // At the buffer's end.
    assert_eq!(
        text_after(text, (8, 2), "dii"),
        "a\n  b\n  c\n\n  d\n\n\ne\n"
    );
    // Visual: again grows to the next level.
    let vim = run(nested, (2, 4), "vii");
    assert_eq!(
        (vim.mode(), vim.visual_anchor(), vim.cursor()),
        ("visual_line", Some((2, 0)), (3, 0))
    );
    let vim = run(nested, (2, 4), "viiii");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((1, 0)), (4, 0)));
    // `.` repeats.
    assert_eq!(text_after("a\n  b\nc\n  d\n", (1, 2), "diij."), "a\nc\n");
    // The other objects on a blank row and at the buffer's end.
    assert_eq!(text_after(text, (3, 0), "dai"), "\n\ne\n  f\n");
    assert_eq!(text_after(text, (3, 0), "daI"), "\ne\n  f\n");
    assert_eq!(text_after(text, (8, 2), "dai"), "a\n  b\n  c\n\n  d\n\n\n");
    assert_eq!(text_after(text, (8, 2), "daI"), "a\n  b\n  c\n\n  d\n\n");
    assert_eq!(text_after(text, (3, 0), "2dii"), "");
    assert_eq!(text_after(text, (8, 2), "2dii"), "");
}

#[test]
fn indent_objects_grow_from_the_selected_level() {
    // From `c`, `ii` again grows, though the cursor sits on the deeper `e`.
    let text = "a\n b\n  c\n  d\n  e\nf\n";
    let vim = run(text, (2, 2), "viiii");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((1, 0)), (4, 0)));
    let vim = run(text, (2, 2), "vaiai");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((0, 0)), (4, 0)));
    let vim = run(text, (2, 2), "vaIaI");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((0, 0)), (5, 0)));
}

#[test]
fn indent_objects_measure_tab_stops() {
    // "\tb" and " \tc" are both eight columns in.
    assert_eq!(text_after("a\n\tb\n \tc\nd\n", (2, 2), "dii"), "a\nd\n");
}

#[test]
fn indent_objects_grow_past_blank_boundaries_and_by_a_count() {
    // `aI` with a blank footer, then again.
    let text = "a\nb\nc\nd\n e\n f\n";
    let vim = run(text, (5, 1), "vaIaI");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((0, 0)), (6, 0)));
    // A count takes that many levels out of the selected one.
    let deep = "a\n b\n  c\n   d\n  e\n b\nf\n";
    let vim = run(deep, (3, 3), "vii2ii");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((1, 0)), (5, 0)));
    let vim = run(deep, (3, 3), "vai2ai");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((0, 0)), (5, 0)));
    let vim = run(deep, (3, 3), "vaI2aI");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((0, 0)), (6, 0)));
    // An all-blank selection matches no level and still selects.
    let blanks = "\n".repeat(2000);
    let vim = run(&blanks, (0, 0), "Vjjii");
    assert_eq!(vim.mode(), "visual_line");
}
