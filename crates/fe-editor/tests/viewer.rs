//! `ViewerModel`: the editor's moves over a read-only `TextSource`, copy with
//! a cap, and every edit refused.

use std::{
    fs,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, ResolvedSelection, Text};
use fe_command::Invocation;
use fe_editor::{
    COPY_LIMIT, CopyError, EditorModel, Outcome, ViewerModel,
    commands::{self, EditorTarget, HeadlessHost},
};
use fe_fileview::{FileText, FileView, IndexState, LineIndex};
use fe_text::TextSource;
use rand::{Rng, SeedableRng, rngs::StdRng};
use serde_json::{Value, json};

/// The moves a viewer runs, by ID.
const MOVES: [&str; 24] = [
    "editor.move_left",
    "editor.move_word_left",
    "editor.move_word_right",
    "editor.move_right",
    "editor.move_up",
    "editor.move_down",
    "editor.move_line_start",
    "editor.move_line_end",
    "editor.move_document_start",
    "editor.move_document_end",
    "editor.page_up",
    "editor.page_down",
    "editor.select_left",
    "editor.select_word_left",
    "editor.select_word_right",
    "editor.select_right",
    "editor.select_up",
    "editor.select_down",
    "editor.select_line_start",
    "editor.select_line_end",
    "editor.select_document_start",
    "editor.select_document_end",
    "editor.select_page_up",
    "editor.select_page_down",
];

const MOTIONS: [&str; 10] = [
    "left",
    "right",
    "word_left",
    "word_right",
    "up",
    "down",
    "line_start",
    "line_end",
    "document_start",
    "document_end",
];

/// Up to 60 pieces: multi-byte chars (each its own grapheme, as a viewer
/// moves by char), tabs, indents and empty rows.
fn random_text(rng: &mut StdRng) -> String {
    const PIECES: [&str; 12] = [
        "a", "word ", "é", "日本", "😀", "\t", "  ", "\u{3000}", "\n", "\n\n", "x\ty", "ab\u{a0}",
    ];
    (0..rng.random_range(0..60))
        .map(|_| PIECES[rng.random_range(0..PIECES.len())])
        .collect()
}

fn complete(view: &Arc<FileView>) -> FileText {
    let index = Arc::new(LineIndex::build(Arc::clone(view)));
    let deadline = Instant::now() + Duration::from_secs(60);
    while index.progress().state == IndexState::Building {
        assert!(Instant::now() < deadline, "still building");
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(index.progress().state, IndexState::Complete);
    FileText::new(Arc::clone(view), index)
}

#[test]
fn moves_like_the_editor() {
    let registry = commands::registry();
    let mut rng = StdRng::seed_from_u64(273);
    for sequence in 0..500 {
        let source = random_text(&mut rng);
        let tab_size = rng.random_range(1..=8);
        let page_rows = rng.random_range(1..=4);
        let mut editor = EditorModel::new(Buffer::new(&source), None);
        editor.set_tab_size(tab_size);
        let mut host = HeadlessHost {
            clipboard: None,
            page_rows,
        };
        let text = Text::new(source.as_str());
        let mut viewer = ViewerModel::new(Arc::new(text.clone()), tab_size);
        viewer.set_page_rows(page_rows);

        let mut ran = Vec::new();
        for _ in 0..30 {
            let (id, args) = match rng.random_range(0..20) {
                0..14 => (MOVES[rng.random_range(0..MOVES.len())], Value::Null),
                14..18 => (
                    "editor.move",
                    json!({
                        "motion": MOTIONS[rng.random_range(0..MOTIONS.len())],
                        "extend": rng.random_bool(0.5),
                        "count": rng.random_range(1..4),
                    }),
                ),
                18 => ("editor.select_all", Value::Null),
                _ => {
                    // A click or drag.
                    let mut offset = || {
                        text.clip_offset(rng.random_range(0..=text.len()), fe_buffer::Bias::Left)
                    };
                    let (anchor, head) = (offset(), offset());
                    ran.push(format!("set_selection({anchor}, {head})"));
                    viewer.set_selection(anchor, head);
                    editor.set_selections([ResolvedSelection {
                        range: anchor.min(head)..anchor.max(head),
                        reversed: head < anchor,
                    }]);
                    continue;
                }
            };
            ran.push(format!("{id} {args}"));
            let outcome = viewer.run(id, &args);
            assert!(
                matches!(outcome, Outcome::Changed | Outcome::Nothing),
                "{id}: {outcome:?}"
            );
            let invocation = Invocation::of(id)
                .unwrap()
                .args(args)
                .context(commands::context());
            registry
                .dispatch(&mut EditorTarget::new(&mut editor, &mut host), &invocation)
                .unwrap();
            assert_eq!(
                viewer.resolved_selections(),
                editor.selections(),
                "sequence {sequence} over {source:?}, tab size {tab_size}, page rows {page_rows}, after {ran:#?}"
            );
            assert_eq!(viewer.head(), editor.primary_selection().head());
        }
    }
}

#[test]
fn copies_a_file_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crlf.txt");
    let bytes = b"caf\xc3\xa9\r\n\xff\xfeab\r\n\r\n\tx\xe2\x82\r\nlast \xc3";
    fs::write(&path, bytes).unwrap();
    let view = Arc::new(FileView::open(&path).unwrap());
    let mut viewer = ViewerModel::new(Arc::new(complete(&view)), 4);
    viewer.select_all();
    assert_eq!(viewer.selection(), 0..bytes.len());
    let expected = "café\n??ab\n\n\tx??\nlast ?";
    assert_eq!(viewer.copy_text().unwrap(), expected);
    assert_eq!(
        viewer.run("editor.copy", &Value::Null),
        Outcome::Copy(Ok(expected.to_string()))
    );
    // Part of the second row through part of the fourth.
    viewer.set_selection(8, 17);
    assert_eq!(viewer.copy_text().unwrap(), "?ab\n\n\tx");

    // Rows of zeros after the text take the file past the cap.
    let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(COPY_LIMIT as u64 + 1000).unwrap();
    drop(file);
    let view = Arc::new(FileView::open(&path).unwrap());
    let mut viewer = ViewerModel::new(Arc::new(complete(&view)), 4);
    viewer.select_all();
    let len = COPY_LIMIT + 1000;
    assert_eq!(viewer.selection(), 0..len);
    let cached = view.cached_bytes();
    assert_eq!(viewer.copy_text(), Err(CopyError::TooLarge { len }));
    assert_eq!(
        viewer.run("editor.copy", &Value::Null),
        Outcome::Copy(Err(CopyError::TooLarge { len }))
    );
    assert_eq!(view.cached_bytes(), cached);
    // One byte under the cap copies.
    viewer.set_selection(1000, len);
    assert_eq!(viewer.copy_text().map(|text| text.len()), Ok(COPY_LIMIT));
}

#[test]
fn refuses_edits() {
    let text = Text::new("first row\n\tsecond é\nthird");
    let mut viewer = ViewerModel::new(Arc::new(text.clone()), 4);
    viewer.set_selection(3, 14);
    let snapshot = |viewer: &ViewerModel| {
        (
            viewer.resolved_selections(),
            viewer.text().slice_to_string(0..viewer.text().len()),
        )
    };
    let before = snapshot(&viewer);

    let registry = commands::registry();
    let mutating: Vec<_> = registry
        .commands()
        .filter(|meta| meta.mutates_buffer)
        .map(|meta| meta.id.as_str().to_owned())
        .collect();
    assert!(mutating.len() >= 10, "{mutating:?}");
    assert!(
        !registry
            .commands()
            .any(|meta| meta.mutates_buffer && meta.id.as_str() == "editor.save")
    );
    for id in mutating.iter().map(String::as_str).chain(["editor.save"]) {
        for args in [Value::Null, json!({ "text": "x" })] {
            assert_eq!(viewer.run(id, &args), Outcome::ReadOnly, "{id}");
            assert_eq!(snapshot(&viewer), before, "{id}");
        }
    }

    // Everything else the registry has either runs or is unsupported.
    let unsupported = [
        "editor.fold",
        "editor.unfold",
        "editor.fold_all",
        "editor.unfold_all",
        "editor.toggle_soft_wrap",
        "editor.add_cursor_above",
        "editor.add_cursor_below",
        "editor.select_next_occurrence",
        "editor.select_all_occurrences",
        "editor.split_selection_into_lines",
        "editor.single_selection",
        "editor.nonexistent",
        "find.next",
    ];
    for id in unsupported {
        assert_eq!(viewer.run(id, &Value::Null), Outcome::Unsupported, "{id}");
        assert_eq!(snapshot(&viewer), before, "{id}");
    }
    assert!(matches!(
        viewer.run("editor.move", &Value::Null),
        Outcome::BadArgs(_)
    ));
    assert_eq!(snapshot(&viewer), before);
    for meta in registry.commands() {
        let id = meta.id.as_str();
        if mutating.iter().any(|m| m == id) || id == "editor.save" || unsupported.contains(&id) {
            continue;
        }
        assert!(
            MOVES.contains(&id)
                || ["editor.move", "editor.select_all", "editor.copy"].contains(&id),
            "{id} is neither refused, run nor unsupported"
        );
    }

    // A longer snapshot of a file still being indexed keeps the selection.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("growing.txt");
    let rows: String = (0..60_000).map(|row| format!("row {row}\r\n")).collect();
    fs::write(&path, &rows).unwrap();
    let view = Arc::new(FileView::open(&path).unwrap());
    let (index, mut steps) = LineIndex::build_stepped(Arc::clone(&view));
    let index = Arc::new(index);
    steps.next().expect("a first chunk");
    let first = FileText::new(Arc::clone(&view), Arc::clone(&index));
    assert!(first.len() < rows.len() && first.row_count() > 10);
    let mut viewer = ViewerModel::new(Arc::new(first.clone()), 4);
    viewer.set_selection(first.len() - 3, 5);
    let before = viewer.resolved_selections();
    steps.for_each(drop);
    let second = FileText::new(Arc::clone(&view), Arc::clone(&index));
    assert_eq!(second.len(), rows.len());
    viewer.refresh(Arc::new(second));
    assert_eq!(viewer.resolved_selections(), before);
    assert_eq!(viewer.text().len(), rows.len());
    assert_eq!(viewer.display().row_count(), 60_001);
}
