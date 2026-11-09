//! Column selection counts visible columns: a wide char takes two.

use fe_buffer::{Buffer, ResolvedSelection};
use fe_command::Invocation;
use fe_editor::{
    EditorModel,
    commands::{self, EditorTarget, HeadlessHost},
};

fn run(model: &mut EditorModel, id: &str) {
    let mut host = HeadlessHost {
        clipboard: None,
        page_rows: 10,
    };
    let invocation = Invocation::of(id).unwrap().context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(model, &mut host), &invocation)
        .unwrap();
}

fn ranges(model: &EditorModel) -> Vec<std::ops::Range<usize>> {
    model.selections().iter().map(|s| s.range.clone()).collect()
}

#[test]
fn keyboard_column_select_counts_visible_columns() {
    let mut model = EditorModel::new(Buffer::new("ab一二cd\nabcdefgh"), None);
    model.set_selections([ResolvedSelection {
        range: 1..1,
        reversed: false,
    }]);
    run(&mut model, "editor.column_select_right");
    run(&mut model, "editor.column_select_right");
    run(&mut model, "editor.column_select_down");
    assert_eq!(ranges(&model), [1..5, 12..15]);
}

#[test]
fn select_box_takes_visible_columns() {
    let mut model = EditorModel::new(Buffer::new("abcdefgh\nab一二cd"), None);
    model.select_box((0, 1), (1, 4));
    assert_eq!(ranges(&model), [1..4, 10..14]);
}
