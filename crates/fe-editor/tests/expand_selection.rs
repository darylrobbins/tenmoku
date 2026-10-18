//! `editor.expand_selection` and `editor.shrink_selection`: each selection
//! grows through a host's nested ranges and shrinking steps back.

use std::ops::{Range, RangeInclusive};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_command::{Effect, Invocation};
use fe_editor::{
    EditorModel,
    commands::{self, EditorHost, EditorTarget},
};

/// A host whose syntax is a fixed list of ranges: a range grows to the first
/// listed one that contains it and differs from it.
struct Nested(Vec<Range<usize>>);

impl EditorHost for Nested {
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

    fn larger_syntax_node(&self, range: Range<usize>) -> Option<Range<usize>> {
        self.0
            .iter()
            .find(|node| node.start <= range.start && range.end <= node.end && **node != range)
            .cloned()
    }
}

fn editor(carets: &[usize]) -> EditorModel {
    let mut editor = EditorModel::new(Buffer::new("abcdefghijkl"), None);
    editor.set_selections(carets.iter().map(|&at| selection(at..at)));
    editor
}

fn selection(range: Range<usize>) -> ResolvedSelection {
    ResolvedSelection {
        range,
        reversed: false,
    }
}

fn ranges(editor: &EditorModel) -> Vec<Range<usize>> {
    editor
        .selections()
        .into_iter()
        .map(|selection| selection.range)
        .collect()
}

fn run(editor: &mut EditorModel, host: &mut Nested, id: &str, args: serde_json::Value) -> Effect {
    let invocation = Invocation::of(id)
        .unwrap()
        .args(args)
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(editor, host), &invocation)
        .unwrap()
        .effect
}

fn expand(editor: &mut EditorModel, host: &mut Nested) -> Effect {
    run(
        editor,
        host,
        "editor.expand_selection",
        serde_json::Value::Null,
    )
}

fn shrink(editor: &mut EditorModel, host: &mut Nested) -> Effect {
    run(
        editor,
        host,
        "editor.shrink_selection",
        serde_json::Value::Null,
    )
}

#[test]
fn expand_grows_each_selection_and_shrink_steps_back() {
    let mut host = Nested(vec![2..3, 1..4, 0..5, 8..9, 7..10, 6..11]);
    let mut editor = editor(&[2, 8]);
    let mut seen = vec![ranges(&editor)];
    for _ in 0..3 {
        assert_eq!(expand(&mut editor, &mut host), Effect::Changed);
        seen.push(ranges(&editor));
    }
    assert_eq!(seen[1], [2..3, 8..9]);
    assert_eq!(seen[2], [1..4, 7..10]);
    assert_eq!(seen[3], [0..5, 6..11]);
    // Nothing is larger now.
    assert_eq!(expand(&mut editor, &mut host), Effect::Nothing);
    assert_eq!(ranges(&editor), seen[3]);

    for earlier in seen[..3].iter().rev() {
        assert_eq!(shrink(&mut editor, &mut host), Effect::Changed);
        assert_eq!(&ranges(&editor), earlier);
    }
    assert_eq!(shrink(&mut editor, &mut host), Effect::Nothing);
    assert_eq!(ranges(&editor), seen[0]);
}

#[test]
fn expanded_selections_that_meet_merge() {
    let mut host = Nested(vec![2..3, 4..5, 0..6]);
    let mut editor = editor(&[2, 4]);
    expand(&mut editor, &mut host);
    assert_eq!(ranges(&editor), [2..3, 4..5]);
    expand(&mut editor, &mut host);
    assert_eq!(ranges(&editor), one(0..6));
    // Shrinking restores both.
    shrink(&mut editor, &mut host);
    assert_eq!(ranges(&editor), [2..3, 4..5]);
}

#[test]
fn an_edit_or_a_click_forgets_the_history() {
    let mut host = Nested(vec![2..3, 1..4, 0..5]);

    let mut editor = editor(&[2]);
    expand(&mut editor, &mut host);
    expand(&mut editor, &mut host);
    editor.insert("x");
    let after_edit = ranges(&editor);
    assert_eq!(shrink(&mut editor, &mut host), Effect::Nothing);
    assert_eq!(ranges(&editor), after_edit);

    let mut editor = self::editor(&[2]);
    expand(&mut editor, &mut host);
    expand(&mut editor, &mut host);
    editor.set_cursor(7);
    assert_eq!(shrink(&mut editor, &mut host), Effect::Nothing);
    assert_eq!(ranges(&editor), one(7..7));
}

/// A list of the one range, which clippy would take for a range of items.
fn one(range: Range<usize>) -> Vec<Range<usize>> {
    vec![range]
}
