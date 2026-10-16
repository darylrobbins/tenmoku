//! [`EditorModel::move_path`]: the stamp a buffer knows moves with its path
//! when its file is moved, so a save still finds a change made behind it.

use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime},
};

use super::*;

/// Writes `text` to `path` as another program would, its modified time
/// moved on so the change shows however coarse the file system's clock.
fn rewrite(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(10))
        .unwrap();
}

#[test]
fn a_change_behind_a_moved_buffer_is_still_a_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    fs::write(&a, "one\n").unwrap();
    let mut model = EditorModel::open(&a).unwrap();
    let split = model.split();
    let stamp = model.buffer().disk_stamp();

    // The file moves, then the buffer: nothing read, the stamp kept.
    fs::rename(&a, &b).unwrap();
    model.move_path(b.clone());
    assert_eq!(model.path(), Some(b.as_path()));
    assert_eq!(split.path(), Some(b.as_path()), "every split's path");
    assert_eq!(model.text().to_string(), "one\n");
    assert_eq!(model.buffer().disk_stamp(), stamp);
    assert!(!model.is_dirty());

    // Unchanged, b saves.
    model.insert("x");
    let saved = model.prepare_save(false).unwrap().run().unwrap();
    assert!(model.finish_save(saved));
    assert_eq!(fs::read_to_string(&b).unwrap(), "xone\n");

    // Changed behind the editor, b is refused, from either split, and
    // nothing reloads.
    rewrite(&b, "theirs\n");
    model.insert("y");
    for model in [&model, &split] {
        let refused = model.prepare_save(false).unwrap().run();
        assert!(matches!(refused, Err(SaveError::ChangedOnDisk)));
    }
    assert_eq!(fs::read_to_string(&b).unwrap(), "theirs\n");
    assert_eq!(model.text().to_string(), "xyone\n");
    assert!(!a.exists(), "nothing goes back to the old path");
}

#[test]
fn set_path_alone_would_not_check_the_moved_file() {
    // What `move_path` adds: a buffer only given the new path takes it as
    // a file it has no stamp of, and writes over a change made there.
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    fs::write(&a, "one\n").unwrap();
    let mut model = EditorModel::open(&a).unwrap();
    fs::rename(&a, &b).unwrap();
    model.set_path(Some(b.clone()));
    rewrite(&b, "theirs\n");
    assert!(model.prepare_save(false).unwrap().run().is_ok());

    let mut moved = EditorModel::open(&b).unwrap();
    let c = dir.path().join("c.txt");
    fs::rename(&b, &c).unwrap();
    moved.move_path(c.clone());
    rewrite(&c, "theirs again\n");
    assert!(moved.prepare_save(false).unwrap().run().is_err());
}

#[test]
fn an_untitled_buffer_given_a_path_keeps_no_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.txt");
    let mut model = EditorModel::new(fe_buffer::Buffer::new("new\n"), None);
    model.move_path(a.clone());
    assert_eq!(model.path(), Some(a.as_path()));
    // No stamp to check against: it saves as a first save does.
    let saved = model.prepare_save(false).unwrap().run().unwrap();
    assert!(model.finish_save(saved));
    assert_eq!(fs::read_to_string(&a).unwrap(), "new\n");
}
