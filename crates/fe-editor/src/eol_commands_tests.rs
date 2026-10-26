use fe_buffer::{Buffer, LineEnding};

use crate::{EditorModel, tests::parse};

fn saved_bytes(model: &EditorModel) -> Vec<u8> {
    let mut out = Vec::new();
    model.buffer().write_to(&mut out).unwrap();
    out
}

#[test]
fn to_crlf_marks_the_buffer_modified() {
    let mut model = parse("a[b]\nc|d\n");
    let (text, selections) = (model.text().to_string(), model.selections());
    assert!(!model.buffer().is_dirty());
    model.set_end_of_line_crlf();
    assert_eq!(model.buffer().line_ending(), LineEnding::CrLf);
    assert!(model.buffer().is_dirty());
    assert_eq!(saved_bytes(&model), b"ab\r\ncd\r\n");
    assert_eq!(model.text().to_string(), text);
    assert_eq!(model.selections(), selections);
}

#[test]
fn to_lf_and_noop() {
    let mut model = EditorModel::new(Buffer::from_file_contents("a\r\nb\r\n"), None);
    assert_eq!(model.buffer().line_ending(), LineEnding::CrLf);
    model.set_end_of_line_crlf();
    assert!(!model.buffer().is_dirty(), "already CRLF");
    assert_eq!(model.undo().unwrap(), None, "no step recorded");
    model.set_end_of_line_lf();
    assert!(model.buffer().is_dirty());
    assert_eq!(saved_bytes(&model), b"a\nb\n");
    let mut model = parse("a\nb");
    model.set_end_of_line_lf();
    assert!(!model.buffer().is_dirty(), "already LF");
}

#[test]
fn undo_restores() {
    let mut model = parse("a\nb");
    model.set_end_of_line_crlf();
    assert!(model.undo().unwrap().is_some());
    assert_eq!(model.buffer().line_ending(), LineEnding::Lf);
    assert!(!model.buffer().is_dirty());
    assert_eq!(model.undo().unwrap(), None, "one undo step");
    assert!(model.redo().unwrap().is_some());
    assert_eq!(model.buffer().line_ending(), LineEnding::CrLf);
    assert!(model.buffer().is_dirty());
    assert_eq!(saved_bytes(&model), b"a\r\nb");
}

/// Opens `contents` from a temp file, changes the ending with `change`,
/// saves through the save lifecycle and reopens it.
fn round_trip(contents: &[u8], change: fn(&mut EditorModel)) -> (EditorModel, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file.txt");
    std::fs::write(&path, contents).unwrap();
    let mut model = EditorModel::open(&path).unwrap();
    assert!(!model.buffer().is_dirty());
    change(&mut model);
    assert!(model.buffer().is_dirty());
    let saved = model.prepare_save(false).unwrap().run().unwrap();
    assert!(model.finish_save(saved));
    assert!(!model.buffer().is_dirty(), "clean once saved");
    let on_disk = std::fs::read(&path).unwrap();
    let reopened = EditorModel::open(&path).unwrap();
    assert_eq!(reopened.text().to_string(), model.text().to_string());
    assert_eq!(
        reopened.buffer().line_ending(),
        model.buffer().line_ending()
    );
    (model, on_disk)
}

#[test]
fn saving_to_crlf_writes_the_bytes_and_undo_is_dirty_against_it() {
    let (mut model, on_disk) = round_trip(b"a\nb\n", EditorModel::set_end_of_line_crlf);
    assert_eq!(on_disk, b"a\r\nb\r\n");
    assert!(model.undo().unwrap().is_some());
    assert_eq!(model.buffer().line_ending(), LineEnding::Lf);
    assert!(model.buffer().is_dirty(), "LF differs from the saved CRLF");
    assert!(model.redo().unwrap().is_some());
    assert_eq!(model.buffer().line_ending(), LineEnding::CrLf);
    assert!(!model.buffer().is_dirty(), "back to the saved ending");
}

#[test]
fn saving_to_lf_writes_the_bytes_and_undo_is_dirty_against_it() {
    let (mut model, on_disk) = round_trip(b"a\r\nb\r\n", EditorModel::set_end_of_line_lf);
    assert_eq!(on_disk, b"a\nb\n");
    assert!(model.undo().unwrap().is_some());
    assert_eq!(model.buffer().line_ending(), LineEnding::CrLf);
    assert!(model.buffer().is_dirty());
    assert!(model.redo().unwrap().is_some());
    assert!(!model.buffer().is_dirty());
}
