//! Saving over a path that has become a directory or other non-regular file
//! reports the I/O error, not a change on disk.

use std::io;

use fe_editor::{EditorModel, SaveError};

fn assert_not_conflict(error: io::Error) -> io::Error {
    let conflict = error
        .get_ref()
        .is_some_and(|inner| inner.downcast_ref::<SaveError>().is_some());
    assert!(!conflict, "{error}");
    error
}

#[test]
fn saving_over_a_path_that_became_a_directory_reports_is_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, "one\n").unwrap();
    let mut model = EditorModel::open(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("inner"), "x").unwrap();
    model.insert("new ");

    for overwrite in [false, true] {
        let result = if overwrite {
            model.save_overwriting()
        } else {
            model.save()
        };
        let error = assert_not_conflict(result.unwrap_err());
        assert_eq!(error.kind(), io::ErrorKind::IsADirectory, "{error}");
        assert!(error.to_string().contains("Is a directory"), "{error}");
        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[cfg(unix)]
#[test]
fn saving_over_a_fifo_is_an_io_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, "one\n").unwrap();
    let mut model = EditorModel::open(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let made = std::process::Command::new("mkfifo").arg(&path).status();
    assert!(
        made.is_ok_and(|status| status.success()),
        "mkfifo is needed to make a FIFO"
    );
    model.insert("new ");

    for overwrite in [false, true] {
        let result = if overwrite {
            model.save_overwriting()
        } else {
            model.save()
        };
        assert_not_conflict(result.unwrap_err());
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
