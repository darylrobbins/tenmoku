//! A save keeps the file's creation date.
#![cfg(target_os = "macos")]

use std::time::Duration;

use fe_buffer::Origin;
use fe_editor::EditorModel;

#[test]
fn a_save_keeps_the_files_creation_date() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dated.txt");
    std::fs::write(&path, "old\n").unwrap();
    // Moving the mtime back moves APFS's birth time back with it.
    assert!(
        std::process::Command::new("/usr/bin/touch")
            .args(["-t", "202001020304"])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let created = std::fs::metadata(&path).unwrap().created().unwrap();
    assert!(created.elapsed().unwrap() > Duration::from_secs(24 * 3600));

    let mut model = EditorModel::open(&path).unwrap();
    let len = model.buffer().len();
    model.buffer_mut().edit([(0..len, "new\n")], Origin::User);
    model.save().unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
    assert_eq!(
        std::fs::metadata(&path).unwrap().created().unwrap(),
        created
    );
}
