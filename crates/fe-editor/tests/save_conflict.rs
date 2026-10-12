//! A save refuses a file that changed on disk since it was read, and can
//! write on another thread while the user keeps typing.

use std::{
    io,
    path::Path,
    time::{Duration, SystemTime},
};

use fe_buffer::{Buffer, DiskStamp};
use fe_editor::{EditorModel, SaveError, SaveJob};

fn assert_conflict(error: io::Error) {
    assert!(
        error
            .to_string()
            .contains("changed on disk since it was read"),
        "{error}"
    );
    let inner = error.into_inner().expect("a conflict carries a SaveError");
    assert!(matches!(
        inner.downcast_ref::<SaveError>(),
        Some(SaveError::ChangedOnDisk)
    ));
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn refuses_a_changed_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, "one\n").unwrap();

    // Another writer, a different length.
    let mut model = EditorModel::open(&path).unwrap();
    assert_eq!(
        model.buffer().disk_stamp(),
        DiskStamp::of(&path).unwrap(),
        "open records the stamp"
    );
    model.insert("mine ");
    std::fs::write(&path, "theirs, longer\n").unwrap();
    assert_conflict(model.save().unwrap_err());
    assert_eq!(read(&path), "theirs, longer\n");
    assert!(model.is_dirty());
    assert!(matches!(
        model.prepare_save(false).unwrap().run(),
        Err(SaveError::ChangedOnDisk)
    ));
    model.save_overwriting().unwrap();
    assert_eq!(read(&path), "mine one\n");
    assert!(!model.is_dirty());

    // A second save after a save doesn't conflict, from either split.
    let mut split = model.split();
    model.insert("again ");
    model.save().unwrap();
    assert_eq!(read(&path), "mine again one\n");
    assert_eq!(split.buffer().disk_stamp(), model.buffer().disk_stamp());
    assert_eq!(model.buffer().disk_stamp(), DiskStamp::of(&path).unwrap());
    split.sync_display();
    split.set_cursor("mine again ".len());
    split.insert("split ");
    split.save().unwrap();
    assert_eq!(read(&path), "mine again split one\n");

    // The same length, written later.
    std::fs::write(&path, "mine again SPLIT one\n").unwrap();
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(10))
        .unwrap();
    drop(file);
    model.sync_display();
    model.set_cursor("mine again split ".len());
    model.insert("x");
    assert_conflict(model.save().unwrap_err());
    assert_conflict(split.save().unwrap_err());
    assert_eq!(read(&path), "mine again SPLIT one\n");

    // Deleted since it was read: saves.
    std::fs::remove_file(&path).unwrap();
    model.save().unwrap();
    assert_eq!(read(&path), "mine again split xone\n");
    assert!(!model.is_dirty());

    // Known missing when read, created since: a conflict.
    let new = dir.path().join("new.txt");
    let mut missing = Buffer::new("");
    missing.set_disk_stamp(None);
    let mut fresh = EditorModel::new(missing, Some(new.clone()));
    assert!(fresh.buffer().disk_stamp_known());
    assert_eq!(fresh.buffer().disk_stamp(), None);
    fresh.insert("hello");
    std::fs::write(&new, "someone else's").unwrap();
    assert_conflict(fresh.save().unwrap_err());
    assert_eq!(read(&new), "someone else's");
    fresh.save_overwriting().unwrap();
    assert_eq!(read(&new), "hello");

    // A buffer that doesn't know its file isn't checked, and is stamped by
    // its first save.
    let unknown = dir.path().join("unknown.txt");
    std::fs::write(&unknown, "on disk").unwrap();
    let mut model = EditorModel::new(Buffer::new("mine"), Some(unknown.clone()));
    assert!(!model.buffer().disk_stamp_known());
    model.save().unwrap();
    assert_eq!(read(&unknown), "mine");
    assert_eq!(
        model.buffer().disk_stamp(),
        DiskStamp::of(&unknown).unwrap()
    );
    std::fs::write(&unknown, "theirs").unwrap();
    model.insert("x");
    assert_conflict(model.save().unwrap_err());

    // A buffer read by from_file carries the stamp of what it read, so a
    // write before the model is made is still caught.
    let loaded = dir.path().join("loaded.txt");
    std::fs::write(&loaded, "read").unwrap();
    let buffer = Buffer::from_file(std::fs::File::open(&loaded).unwrap()).unwrap();
    std::fs::write(&loaded, "rewritten").unwrap();
    let mut model = EditorModel::new(buffer, Some(loaded.clone()));
    model.insert("x");
    assert_conflict(model.save().unwrap_err());
    assert_eq!(read(&loaded), "rewritten");

    // An untitled buffer has nowhere to save.
    let untitled = EditorModel::new(Buffer::new("x"), None);
    assert_eq!(
        untitled.prepare_save(false).err().map(|error| error.kind()),
        Some(io::ErrorKind::NotFound)
    );
}

#[test]
fn job_off_thread() {
    fn assert_send<T: Send>() {}
    assert_send::<SaveJob>();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, "one\r\ntwo\r\n").unwrap();
    let mut model = EditorModel::open(&path).unwrap();
    model.set_cursor(3);
    model.insert("!");

    // Typing straight on: preparing the save ends the run of typing.
    let job = model.prepare_save(false).unwrap();
    assert_eq!(job.path(), path);
    let running = std::thread::spawn(move || job.run());
    model.insert("?");
    model.insert("?");
    let saved = running.join().unwrap().unwrap();
    assert_eq!(saved.stamp(), DiskStamp::of(&path).unwrap());
    assert!(model.finish_save(saved));

    assert_eq!(read(&path), "one!\r\ntwo\r\n");
    assert_eq!(model.text().to_string(), "one!??\ntwo\n");
    assert!(model.is_dirty(), "typed after the snapshot");
    assert_eq!(model.buffer().disk_stamp(), DiskStamp::of(&path).unwrap());

    // Undoing the typing since gets back to what's on disk.
    model.undo().unwrap().unwrap();
    assert_eq!(model.text().to_string(), "one!\ntwo\n");
    assert!(!model.is_dirty());

    // And the next save doesn't conflict with the one that ran off thread.
    model.insert("#");
    model.save().unwrap();
    assert_eq!(read(&path), "one!#\r\ntwo\r\n");
}

#[test]
fn saves_finishing_out_of_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, "base").unwrap();
    let mut model = EditorModel::open(&path).unwrap();
    model.set_cursor(4);

    // A, then B, prepared; B written first, then A over it.
    model.insert(" a");
    let a = model.prepare_save(true).unwrap();
    model.insert(" b");
    let b = model.prepare_save(true).unwrap();
    let b = b.run().unwrap();
    let a = a.run().unwrap();
    assert_eq!(read(&path), "base a");
    assert!(model.finish_save(a));
    assert!(!model.finish_save(b), "B was overwritten by A");
    assert!(model.is_dirty());
    assert_eq!(model.buffer().disk_stamp(), DiskStamp::of(&path).unwrap());
    model.undo().unwrap().unwrap();
    assert_eq!(model.text().to_string(), "base a");
    assert!(!model.is_dirty(), "clean against A, which is on disk");
    model.redo().unwrap().unwrap();

    // Finished the other way round, B is still the one dropped.
    let a = model.prepare_save(true).unwrap();
    model.insert("!");
    let b = model.prepare_save(true).unwrap();
    let b = b.run().unwrap();
    let a = a.run().unwrap();
    assert!(!model.finish_save(b));
    assert!(model.finish_save(a));
    assert_eq!(read(&path), "base a b");
    assert!(model.is_dirty());

    // A reload of another writer's file before a save finishes: the save
    // isn't recorded over it.
    let job = model.prepare_save(true).unwrap();
    let saved = job.run().unwrap();
    std::fs::write(&path, "theirs").unwrap();
    let (file, stamp) = Buffer::read_file(std::fs::File::open(&path).unwrap()).unwrap();
    model.reload(file, Some(stamp));
    assert!(!model.finish_save(saved));
    assert_eq!(model.text().to_string(), "theirs");
    assert!(!model.is_dirty());
    assert_eq!(model.buffer().disk_stamp(), Some(stamp));

    // A reload of the file the save wrote: already recorded.
    model.set_cursor(0);
    model.insert(">");
    let saved = model.prepare_save(false).unwrap().run().unwrap();
    let (file, stamp) = Buffer::read_file(std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(Some(stamp), saved.stamp());
    assert_eq!(model.reload(file, Some(stamp)), None);
    assert!(!model.finish_save(saved));
    assert!(!model.is_dirty());
}

#[test]
fn reload_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, "first\nmiddle\nlast\n").unwrap();
    let mut model = EditorModel::open(&path).unwrap();
    let mut split = model.split();
    model.set_cursor(2);
    let last = "first\nmiddle\nla".len();
    split.set_cursor(last);

    std::fs::write(&path, "first\nthe new middle\nlast\n").unwrap();
    let (file, stamp) = Buffer::read_file(std::fs::File::open(&path).unwrap()).unwrap();
    assert!(model.reload(file, Some(stamp)).is_some());
    assert_eq!(model.text().to_string(), "first\nthe new middle\nlast\n");
    assert!(!model.is_dirty());
    assert_eq!(model.primary_selection().head(), 2);
    split.sync_display();
    assert_eq!(split.primary_selection().head(), last + "the new ".len());
    // Saving after the reload doesn't conflict.
    model.insert("x");
    model.save().unwrap();
    model.undo().unwrap().unwrap();

    model.undo().unwrap().unwrap();
    assert_eq!(model.text().to_string(), "first\nmiddle\nlast\n");
    assert!(model.is_dirty());
}
