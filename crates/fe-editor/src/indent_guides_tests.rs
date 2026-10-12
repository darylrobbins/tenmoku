use super::*;

fn text(s: &str) -> Text {
    Text::from_file_contents(s).0
}

fn g(column: u32, rows: Range<u32>) -> IndentGuide {
    IndentGuide { column, rows }
}

const NESTED: &str = "\
fn main() {
    if x {
        a();
        b();
    }
    c();
}
";

#[test]
fn nested_rust_gives_guides_over_deeper_rows() {
    let t = text(NESTED);
    assert_eq!(indent_guides(&t, 0..8, 4, 4), vec![g(0, 1..6), g(4, 2..4)]);
    // Column 8 needs a row indented past 8.
    let deeper = text("a {\n    b {\n        c {\n            d\n        }\n    }\n}\n");
    assert_eq!(
        indent_guides(&deeper, 0..7, 4, 4),
        vec![g(0, 1..6), g(4, 2..5), g(8, 3..4)]
    );
}

#[test]
fn tabs_advance_to_tab_stops() {
    let t = text("a\n\tb\n\t\tc\n \td\n  \t  e\n");
    let (raw, _) = indents(&t, 0..5, 4);
    assert_eq!(raw, vec![Some(0), Some(4), Some(8), Some(4), Some(6)]);
    let (raw, _) = indents(&t, 0..5, 2);
    assert_eq!(raw[1..5], [Some(2), Some(4), Some(2), Some(6)]);
    assert_eq!(
        indent_guides(&t, 0..5, 4, 4),
        vec![g(0, 1..5), g(4, 2..3), g(4, 4..5)]
    );
}

#[test]
fn blank_rows_keep_guides_and_take_the_greater_neighbour() {
    let t = text("a {\n    b\n\n    c\n}\n");
    assert_eq!(indent_guides(&t, 0..5, 4, 4), vec![g(0, 1..4)]);
    // Blank before a dedent takes the deeper (above) indent; whitespace-only too.
    let t = text("a {\n    b\n   \n}\n");
    assert_eq!(indent_guides(&t, 0..4, 4, 4), vec![g(0, 1..3)]);
    // Blank before an indent takes the deeper (below) indent.
    let t = text("a\n\n    b\n");
    assert_eq!(indent_guides(&t, 0..3, 4, 4), vec![g(0, 1..3)]);
}

#[test]
fn guides_are_clipped_and_sorted() {
    let t = text(NESTED);
    assert_eq!(indent_guides(&t, 3..5, 4, 4), vec![g(0, 3..5), g(4, 3..4)]);
    assert_eq!(indent_guides(&t, 0..0, 4, 4), vec![]);
    assert_eq!(indent_guides(&t, 50..60, 4, 4), vec![]);
    let t = text("a\n    b\n        c\nd\n    e\n        f\n");
    assert_eq!(
        indent_guides(&t, 0..6, 4, 4),
        vec![g(0, 1..3), g(0, 4..6), g(4, 2..3), g(4, 5..6)]
    );
}

#[test]
fn active_guide_on_opening_row_and_inside() {
    let t = text(NESTED);
    assert_eq!(active_indent_guide(&t, 0, 4, 4), Some(g(0, 1..6)));
    assert_eq!(active_indent_guide(&t, 1, 4, 4), Some(g(4, 2..4)));
    assert_eq!(active_indent_guide(&t, 2, 4, 4), Some(g(4, 2..4)));
    assert_eq!(active_indent_guide(&t, 3, 4, 4), Some(g(4, 2..4)));
    assert_eq!(active_indent_guide(&t, 4, 4, 4), Some(g(0, 1..6)));
    assert_eq!(active_indent_guide(&t, 6, 4, 4), None);
    assert_eq!(active_indent_guide(&t, 99, 4, 4), None);
}

#[test]
fn active_guide_with_blank_row_after_opening_row() {
    let t = text("a {\n\n    b\n}\n");
    assert_eq!(active_indent_guide(&t, 0, 4, 4), Some(g(0, 1..3)));
}

#[test]
fn long_indents_count_as_400_columns() {
    let t = text(&format!("{}x\n{}\n", " ".repeat(500), " ".repeat(450)));
    let (raw, _) = indents(&t, 0..2, 4);
    assert_eq!(raw, vec![Some(400), Some(400)]);
    let t = text(&format!("{}x\n", "\t".repeat(300)));
    let (raw, _) = indents(&t, 0..1, 8);
    assert_eq!(raw, vec![Some(400)]);
}

#[test]
fn blank_rows_look_around_at_most_1000_rows() {
    let mut s = String::from("    a\n");
    s.push_str(&"\n".repeat(1000));
    s.push_str("    b\n");
    // Blank rows 1..=1000: nearest non-blank is within 1000 on some side.
    let t = text(&s);
    let (_, eff) = indents(&t, 0..1002, 4);
    assert!(eff.iter().all(|&i| i == 4));

    let mut s = String::from("    a\n");
    s.push_str(&"\n".repeat(2001));
    s.push_str("    b\n");
    let t = text(&s);
    let (_, eff) = indents(&t, 0..2003, 4);
    assert_eq!(eff[1000], 4);
    assert_eq!(eff[1001], 0); // 1,001 rows from a non-blank row on both sides
}

#[test]
fn zero_widths_fall_back() {
    let t = text("a\n  b\n    c\n");
    assert_eq!(indent_guides(&t, 0..3, 0, 2), indent_guides(&t, 0..3, 2, 2));
    assert_eq!(indent_guides(&t, 0..3, 0, 0), indent_guides(&t, 0..3, 4, 4));
    assert_eq!(widths(0, 0), (4, 4));
    assert_eq!(widths(0, 8), (8, 8));
}

#[test]
fn a_multibyte_char_at_the_400_byte_cutoff_is_safe() {
    let t = text(&format!("a\n{}é\n  b\n", "x".repeat(399)));
    assert_eq!(indent_guides(&t, 0..3, 4, 4), vec![g(0, 2..3)]);
    assert_eq!(active_indent_guide(&t, 1, 4, 4), Some(g(0, 2..4)));
}

#[test]
fn unicode_blank_rows_are_blank_and_only_spaces_and_tabs_indent() {
    let t = text("a\n\u{2003}\n    b\n");
    assert_eq!(indent_guides(&t, 0..3, 4, 4), vec![g(0, 1..3)]);
    let t = text("a\n \u{c}   b\n");
    let (raw, _) = indents(&t, 0..2, 4);
    assert_eq!(raw, vec![Some(0), Some(1)]);
}

#[test]
fn a_huge_tab_width_caps_instead_of_overflowing() {
    let t = text("a\n\t\tb\n");
    let (raw, _) = indents(&t, 0..2, 1 << 31);
    assert_eq!(raw[1], Some(400));
    assert_eq!(indent_guides(&t, 0..2, 4, 1 << 31).len(), 100);
}
