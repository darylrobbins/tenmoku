//! A save onto a file that didn't exist when it ran its check never
//! replaces one created meanwhile.

use fe_buffer::Buffer;

use super::*;

fn job(target: &Path, overwrite: bool) -> SaveJob {
    let model = EditorModel::new(Buffer::new("mine"), None);
    model.prepare_save_to(target.to_owned(), overwrite)
}

#[test]
fn a_file_created_before_the_rename_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new.txt");
    let result = job(&target, false).run_with(|| {}, || std::fs::write(&target, "theirs").unwrap());
    assert!(matches!(result, Err(SaveError::ChangedOnDisk)));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "theirs");
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "temp file gone"
    );
}

#[test]
fn an_existing_file_is_refused_and_a_forced_save_replaces_it() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("there.txt");
    std::fs::write(&target, "theirs").unwrap();
    assert!(matches!(
        job(&target, false).run(),
        Err(SaveError::ChangedOnDisk)
    ));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "theirs");

    // A forced save replaces one that appears after the job's own checks.
    let late = dir.path().join("late.txt");
    let forced = job(&late, true).run_with(|| {}, || std::fs::write(&late, "theirs").unwrap());
    forced.unwrap();
    assert_eq!(std::fs::read_to_string(&late).unwrap(), "mine");
    job(&target, true).run().unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "mine");
}

#[cfg(unix)]
#[test]
fn a_dangling_symlink_created_late_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new.txt");
    let referent = dir.path().join("referent.txt");
    let result = job(&target, false).run_with(
        || {},
        || std::os::unix::fs::symlink(&referent, &target).unwrap(),
    );
    assert!(matches!(result, Err(SaveError::ChangedOnDisk)));
    assert_eq!(std::fs::read_link(&target).unwrap(), referent);
    assert!(!referent.exists());
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "temp file gone"
    );
}

#[cfg(unix)]
#[test]
fn an_ordinary_save_through_a_dangling_symlink_creates_its_referent() {
    let dir = tempfile::tempdir().unwrap();
    let (link, referent) = (dir.path().join("link.txt"), dir.path().join("referent.txt"));
    std::os::unix::fs::symlink(&referent, &link).unwrap();
    let mut model = EditorModel::new(Buffer::new("mine"), Some(link.clone()));
    model.buffer_mut().set_disk_stamp(None);
    model.prepare_save(false).unwrap().run().unwrap();
    assert_eq!(std::fs::read_link(&link).unwrap(), referent);
    assert_eq!(std::fs::read_to_string(&referent).unwrap(), "mine");
}
