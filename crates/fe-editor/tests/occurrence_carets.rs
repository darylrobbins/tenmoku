//! `editor.add_caret_at_next_occurrence` and
//! `editor.add_carets_at_all_occurrences`: carets, not selections, at the
//! whole-word, case-sensitive occurrences of the word at the primary caret,
//! as far into each as the primary caret is into its word.

use std::ops::{Range, RangeInclusive};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_command::Invocation;
use fe_editor::{
    EditorModel,
    commands::{self, EditorHost, EditorTarget},
};

struct Host;

impl EditorHost for Host {
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
}

/// An editor on `text` with a caret at each offset.
fn editor(text: &str, carets: &[usize]) -> EditorModel {
    let mut editor = EditorModel::new(Buffer::new(text), None);
    editor.set_selections(carets.iter().map(|&at| ResolvedSelection {
        range: at..at,
        reversed: false,
    }));
    editor
}

/// Each selection's range.
fn ranges(editor: &EditorModel) -> Vec<Range<usize>> {
    editor
        .selections()
        .into_iter()
        .map(|selection| selection.range)
        .collect()
}

/// Carets at `offsets`, as [`ranges`] gives them.
fn carets(offsets: &[usize]) -> Vec<Range<usize>> {
    offsets.iter().map(|&at| at..at).collect()
}

fn run(editor: &mut EditorModel, id: &str) {
    let invocation = Invocation::of(id).unwrap().context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(editor, &mut Host), &invocation)
        .unwrap();
}

fn next(editor: &mut EditorModel) {
    run(editor, "editor.add_caret_at_next_occurrence");
}

fn all(editor: &mut EditorModel) {
    run(editor, "editor.add_carets_at_all_occurrences");
}

#[test]
fn occurrence_carets_add_next_and_all() {
    // "foo foobar Foo foo_x foo foo": whole words only, case kept.
    let text = "foo foobar Foo foo_x foo foo";
    let (first, second, third) = (0, 21, 25);

    // From one caret, one into its word: the next occurrence, as far in.
    let mut model = editor(text, &[1]);
    next(&mut model);
    assert_eq!(ranges(&model), carets(&[first + 1, second + 1]));
    next(&mut model);
    assert_eq!(ranges(&model), carets(&[first + 1, second + 1, third + 1]));
    // Every occurrence holds a caret: nothing more.
    next(&mut model);
    assert_eq!(ranges(&model), carets(&[first + 1, second + 1, third + 1]));

    // From the middle occurrence: on to the last, then wrapping once to
    // the first, skipping the ones held.
    let mut model = editor(text, &[second + 2]);
    next(&mut model);
    assert_eq!(ranges(&model), carets(&[second + 2, third + 2]));
    next(&mut model);
    assert_eq!(ranges(&model), carets(&[first + 2, second + 2, third + 2]));
    next(&mut model);
    assert_eq!(ranges(&model), carets(&[first + 2, second + 2, third + 2]));

    // The search starts past the bottommost caret, wherever it is.
    let mut model = editor(text, &[0, 12]);
    // The primary (bottommost) caret is in `Foo`: there's no other.
    next(&mut model);
    assert_eq!(ranges(&model), carets(&[0, 12]));

    // A selection turns into a caret at its head.
    let mut model = editor(text, &[0]);
    model.select_range(first + 1..first + 2, false);
    next(&mut model);
    assert_eq!(ranges(&model), carets(&[first + 2, second + 2]));

    // A caret just past an occurrence, on the next char, isn't in it.
    let mut model = editor("foo.foo", &[3, 5]);
    next(&mut model);
    assert_eq!(ranges(&model), carets(&[1, 3, 5]));

    // On whitespace, nothing.
    let mut model = editor("foo  foo", &[4]);
    next(&mut model);
    assert_eq!(ranges(&model), carets(&[4]));
    all(&mut model);
    assert_eq!(ranges(&model), carets(&[4]));

    // A selection with no occurrence left, or on whitespace, still turns
    // into a caret.
    for (text, range, head) in [("foo", 0..1, 1), ("foo  foo", 0..4, 4)] {
        for command in [next, all] {
            let mut model = editor(text, &[0]);
            model.select_range(range.clone(), false);
            command(&mut model);
            assert_eq!(ranges(&model), carets(&[head]), "{text:?}");
        }
    }

    // At the end of the text after spaces or tabs, nothing; after a word,
    // that word, from its end.
    for text in [" foo  ", "foo\t\t", "foo \t"] {
        for command in [next, all] {
            let mut model = editor(text, &[text.len()]);
            command(&mut model);
            assert_eq!(ranges(&model), carets(&[text.len()]), "{text:?}");
        }
    }
    let mut model = editor("foo foo", &[7]);
    next(&mut model);
    assert_eq!(ranges(&model), carets(&[3, 7]));

    // All of them, from any one.
    let mut model = editor(text, &[second]);
    all(&mut model);
    assert_eq!(ranges(&model), carets(&[first, second, third]));
    // Again: nothing new.
    all(&mut model);
    assert_eq!(ranges(&model), carets(&[first, second, third]));

    // Other carets stay, and a word over several rows and multibyte
    // chars matches whole.
    let mut model = editor("été x\nété\nétés été", &[6, 10]);
    // The primary caret is at 10, two bytes into the second row's `été`.
    all(&mut model);
    assert_eq!(ranges(&model), carets(&[2, 6, 10, 23]));
    assert!(model.selections().iter().all(|s| s.range.is_empty()));
}
