use std::ops::{Range, RangeInclusive};

use fe_buffer::BufferSnapshot;
use fe_command::Invocation;

use crate::{
    EditorModel,
    auto_indent::IndentSuggestion,
    commands::{self, EditorHost, EditorTarget},
    tests::{parse, render},
};

/// A host with no syntax but an indent suggestion of a level in from the
/// row before.
struct Indenting;

impl EditorHost for Indenting {
    fn page_rows(&self) -> u32 {
        20
    }
    fn read_clipboard(&mut self) -> Option<String> {
        None
    }
    fn write_clipboard(&mut self, _: String) {}
    fn fold_range_at(&self, _: u32) -> Option<RangeInclusive<u32>> {
        None
    }
    fn fold_ranges(&self, _: Range<u32>) -> Vec<RangeInclusive<u32>> {
        Vec::new()
    }
    fn suggest_indent(&self, _: &BufferSnapshot, row: u32) -> Option<IndentSuggestion> {
        Some(IndentSuggestion::Relative {
            basis_row: row - 1,
            delta: 1,
        })
    }
}

fn run(model: &mut EditorModel, host: &mut dyn EditorHost, command: &str) {
    let invocation = Invocation::of(command)
        .unwrap()
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(model, host), &invocation)
        .unwrap();
}

/// `marked` after `command`, which must be one undo step.
fn one_step(marked: &str, command: &str, host: &mut dyn EditorHost) -> String {
    let mut model = parse(marked);
    run(&mut model, host, command);
    let after = render(&model);
    model.undo().unwrap();
    assert_eq!(model.text().to_string(), parse(marked).text().to_string());
    assert_eq!(model.undo(), Ok(None), "{marked:?} is one step");
    after
}

fn plain(marked: &str, command: &str) -> String {
    one_step(marked, command, &mut commands::HeadlessHost::new())
}

#[test]
fn below_opens_an_indented_row() {
    assert_eq!(
        plain("    fo|o(a, b)", "editor.insert_line_below"),
        "    foo(a, b)\n    |"
    );
    assert_eq!(plain("a|\nb", "editor.insert_line_below"), "a\n|\nb");
    // After an opener the host's indentation applies.
    assert_eq!(
        one_step("  f {|", "editor.insert_line_below", &mut Indenting),
        "  f {\n      |"
    );
    assert_eq!(
        one_step("  f| {", "editor.insert_line_below", &mut Indenting),
        "  f {\n      |"
    );
}

#[test]
fn above_and_many_carets() {
    assert_eq!(plain("  fo|o", "editor.insert_line_above"), "  |\n  foo");
    assert_eq!(plain("|a", "editor.insert_line_above"), "|\na");
    assert_eq!(plain("a|b|\nc|", "editor.insert_line_below"), "ab\n|\nc\n|");
    assert_eq!(plain("a|b|\nc|", "editor.insert_line_above"), "|\nab\n|\nc");
    // A selection spanning rows: its last row for below, first for above.
    assert_eq!(
        plain("a[b\nc\nd]e\nf", "editor.insert_line_below"),
        "ab\nc\nde\n|\nf"
    );
    assert_eq!(
        plain("a[b\nc\nd]e\nf", "editor.insert_line_above"),
        "|\nab\nc\nde\nf"
    );
}

#[test]
fn copy_up_keeps_selection_on_the_upper_copy() {
    assert_eq!(plain("a|b\ncd", "editor.copy_rows_up"), "a|b\nab\ncd");
    assert_eq!(
        plain("a\nb[c\nd]e\nf", "editor.copy_rows_up"),
        "a\nb[c\nd]e\nbc\nde\nf"
    );
    // The final row has no newline.
    assert_eq!(plain("a\nb|c", "editor.copy_rows_up"), "a\nb|c\nbc");
    assert_eq!(plain("a|\nb\nc|", "editor.copy_rows_up"), "a|\na\nb\nc|\nc");
}
