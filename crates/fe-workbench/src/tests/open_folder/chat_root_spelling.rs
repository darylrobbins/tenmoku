//! A folder opened by another spelling than its canonical one (T695): the
//! chat's context vetting checks a file an editor has open, by the
//! canonical path editors are keyed by, against the root as it was opened.
//! Through a link here, which Linux makes too; an 8.3 short name or another
//! letter case on Windows and macOS (`tests::chat_edit_root_spelling`).

#![cfg(unix)]

use std::fs;

use fe_buffer::DiskStamp;

use crate::chat_panel::chat_context;

#[test]
fn context_under_a_root_opened_through_a_link_is_inside_it() {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    fs::create_dir(&real).unwrap();
    let real = dunce::canonicalize(real).unwrap();
    let notes = real.join("notes.txt");
    fs::write(&notes, "one\n").unwrap();
    fs::write(real.join(".env"), "KEY=1\n").unwrap();
    let opened = temp.path().join("link");
    std::os::unix::fs::symlink(&real, &opened).unwrap();
    let stamp = DiskStamp::from_metadata(&fs::metadata(&notes).unwrap()).ok();

    // The active editor's path, canonical, as automatic context vets it.
    assert_eq!(
        chat_context::vet_text(&opened, &notes, stamp.as_ref(), || {}),
        Ok(())
    );
    // And joined to the root as it was opened.
    assert_eq!(
        chat_context::vet_text(&opened, &opened.join("notes.txt"), stamp.as_ref(), || {}),
        Ok(())
    );
    // Its secrets are still refused, by either spelling.
    for secret in [real.join(".env"), opened.join(".env")] {
        let vetted = chat_context::open_vetted(&opened, &secret).map(|(real, _)| real);
        assert!(vetted.is_err(), "{} refused", secret.display());
    }
}
