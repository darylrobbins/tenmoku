//! A save keeps the file's extended attributes.
#![cfg(target_os = "macos")]

use fe_buffer::Origin;
use fe_editor::EditorModel;

#[test]
fn a_save_keeps_the_files_extended_attributes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tagged.txt");
    std::fs::write(&path, "old\n").unwrap();
    let xattr = |args: &[&str]| {
        std::process::Command::new("/usr/bin/xattr")
            .args(args)
            .arg(&path)
            .output()
            .unwrap()
    };
    assert!(
        xattr(&["-w", "com.example.fastedit-test", "kept"])
            .status
            .success()
    );

    let mut model = EditorModel::open(&path).unwrap();
    let len = model.buffer().len();
    model.buffer_mut().edit([(0..len, "new\n")], Origin::User);
    model.save().unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
    let kept = xattr(&["-p", "com.example.fastedit-test"]);
    assert_eq!(String::from_utf8_lossy(&kept.stdout).trim(), "kept");
}
