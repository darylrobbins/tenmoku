//! A Save As onto a symlink to nothing is refused as a change on disk, as
//! any existing destination is, rather than creating the file the link
//! names (docs/decisions/h6ngbbm2jd-fe-host.md).

#![cfg(unix)]

use fe_buffer::Buffer;
use fe_editor::{EditorModel, SaveError};

#[test]
fn a_save_as_onto_a_dangling_link_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (link, referent) = (dir.path().join("link.txt"), dir.path().join("referent.txt"));
    std::os::unix::fs::symlink(&referent, &link).unwrap();
    let model = EditorModel::new(Buffer::new("mine"), None);
    let result = model.prepare_save_as(link.clone(), None, false).run();
    assert!(matches!(result, Err(SaveError::ChangedOnDisk)));
    assert_eq!(std::fs::read_link(&link).unwrap(), referent, "link kept");
    assert!(!referent.exists());
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "nothing written"
    );

    // Overwriting writes through it.
    let model = EditorModel::new(Buffer::new("mine"), None);
    model
        .prepare_save_as(link.clone(), None, true)
        .run()
        .unwrap();
    assert_eq!(std::fs::read_link(&link).unwrap(), referent, "link kept");
    assert_eq!(std::fs::read_to_string(&referent).unwrap(), "mine");
}
