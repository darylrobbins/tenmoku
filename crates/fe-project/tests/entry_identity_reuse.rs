//! A file deleted and re-created in a folder can get the freed inode back on
//! ext4 and xfs. It is another entry all the same: what an operation took to
//! be "the file it was asked about" must not match it.

#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::MetadataExt;

use fe_project::fs_ops::create_recorded;

#[test]
fn a_recreated_file_with_a_reused_inode_is_not_the_same_entry() {
    // The repo's own disk (ext4), not /tmp, which is often tmpfs and never
    // reuses an inode.
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();

    let mut reused = None;
    for attempt in 0..50 {
        let name = format!("note-{attempt}.txt");
        let first = create_recorded(&root, &root, &name, false).unwrap();
        let inode = fs::symlink_metadata(&first.path).unwrap().ino();
        fs::remove_file(&first.path).unwrap();
        // Birth times come from the kernel's coarse clock, a few ms apart.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let second = create_recorded(&root, &root, &name, false).unwrap();
        if fs::symlink_metadata(&second.path).unwrap().ino() == inode {
            reused = Some((first, second));
            break;
        }
    }
    let Some((first, second)) = reused else {
        eprintln!("skipped: this file system doesn't reuse an inode at once");
        return;
    };

    assert_ne!(
        first.first.id, second.first.id,
        "the new file is the old one"
    );
}
