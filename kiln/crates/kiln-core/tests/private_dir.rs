// SPDX-License-Identifier: Apache-2.0

//! `create_private_dir` makes directories only their owner can write, which
//! `owned_and_unshared` trusts, along with what is made inside them; widened
//! to anyone else's writes (a mode on Unix, an access entry on Windows), it
//! trusts them no longer.

use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

use kiln_core::platform::{Owner, create_private_dir, open_dir, owned_and_unshared, private};

/// A directory of its own under cargo's temporary directory for tests, not yet made.
fn temp_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "kiln-core-private-dir-{}-{name}",
        std::process::id()
    ));
    fs::remove_dir_all(&dir).ok();
    dir
}

/// A new private file at `path`.
fn create(path: &Path) -> File {
    private(OpenOptions::new().write(true).create_new(true))
        .open(path)
        .unwrap()
}

/// Whose what we make in `dir` is, learnt as kiln-plugin's cache does: from a file made there.
fn owner_in(dir: &Path) -> Owner {
    let probe = dir.join(".probe");
    let owner = Owner::of(&create(&probe)).unwrap();
    fs::remove_file(probe).unwrap();
    owner
}

fn trusted(path: &Path, owner: &Owner) -> bool {
    let file = if path.is_dir() {
        open_dir(path).unwrap()
    } else {
        File::open(path).unwrap()
    };
    owned_and_unshared(&file, owner)
}

/// Lets anyone write `path`: on Windows an access entry for Everyone (`*S-1-1-0`, by SID, as
/// its name is translated), inherited by what is made inside if `inherited`.
#[cfg(windows)]
fn widen(path: &Path, inherited: bool) {
    let grant = if inherited {
        "*S-1-1-0:(OI)(CI)(W)"
    } else {
        "*S-1-1-0:(W)"
    };
    let status = std::process::Command::new("icacls")
        .arg(path)
        .args(["/grant", grant])
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "icacls /grant on {}", path.display());
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn private_dir_new_dir_is_owner_only() {
    let root = temp_dir("new");
    let dir = root.join("a").join("b");
    create_private_dir(&dir).unwrap();
    let owner = owner_in(&dir);
    assert!(trusted(&dir, &owner));
    // The missing ancestors it made are private too.
    assert!(trusted(&root.join("a"), &owner));
    // And what is made inside, which on Windows inherits the directory's entries.
    let file = dir.join("entry");
    create(&file);
    assert!(trusted(&file, &owner));
    #[cfg(unix)]
    {
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&root.join("a")), 0o700);
        assert_eq!(mode(&file), 0o600);
    }
    // Again, on the directory that now exists, it succeeds and changes nothing.
    create_private_dir(&dir).unwrap();
    assert!(trusted(&dir, &owner));
    fs::remove_dir_all(root).unwrap();
}

/// A parent anyone can write doesn't widen the private directory made in it: on Windows its
/// access list is protected, so the parent's inheritable entries don't reach it.
#[test]
fn private_dir_ignores_a_widened_parent() {
    let root = temp_dir("parent");
    fs::create_dir_all(&root).unwrap();
    #[cfg(unix)]
    chmod(&root, 0o777);
    #[cfg(windows)]
    widen(&root, true);
    let dir = root.join("private");
    create_private_dir(&dir).unwrap();
    let owner = owner_in(&dir);
    assert!(trusted(&dir, &owner));
    let file = dir.join("entry");
    create(&file);
    assert!(trusted(&file, &owner));
    #[cfg(unix)]
    chmod(&root, 0o700);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn private_dir_widened_dir_is_not_trusted() {
    let root = temp_dir("widened");
    let dir = root.join("private");
    create_private_dir(&dir).unwrap();
    let owner = owner_in(&dir);
    let file = dir.join("entry");
    create(&file);
    assert!(trusted(&dir, &owner));
    assert!(trusted(&file, &owner));
    #[cfg(unix)]
    {
        // Group write, then other write, on the directory and on a file in it.
        for widened in [0o720, 0o702] {
            chmod(&dir, widened);
            assert!(!trusted(&dir, &owner), "mode {widened:o}");
            chmod(&dir, 0o700);
            assert!(trusted(&dir, &owner));
        }
        for widened in [0o620, 0o602] {
            chmod(&file, widened);
            assert!(!trusted(&file, &owner), "mode {widened:o}");
            chmod(&file, 0o600);
            assert!(trusted(&file, &owner));
        }
        // Read and execute for others write nothing.
        chmod(&dir, 0o755);
        assert!(trusted(&dir, &owner));
        chmod(&dir, 0o700);
    }
    #[cfg(windows)]
    {
        widen(&file, false);
        assert!(!trusted(&file, &owner));
        // The directory itself; an entry not inherited, so a file made in it later still is.
        widen(&dir, false);
        assert!(!trusted(&dir, &owner));
        let later = dir.join("later");
        create(&later);
        assert!(trusted(&later, &owner));
    }
    fs::remove_dir_all(root).unwrap();
}

/// Someone else's file isn't the owner's: on Unix one owned by root, where a test can find
/// one.
#[cfg(unix)]
#[test]
fn private_dir_another_owner_is_not_trusted() {
    use std::os::unix::fs::MetadataExt;
    let root = temp_dir("other");
    create_private_dir(&root).unwrap();
    let owner = owner_in(&root);
    let theirs = Path::new("/");
    if fs::metadata(theirs).unwrap().uid() != fs::metadata(&root).unwrap().uid() {
        assert!(!owned_and_unshared(&open_dir(theirs).unwrap(), &owner));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn open_dir_refuses_a_file() {
    let root = temp_dir("file");
    create_private_dir(&root).unwrap();
    let file = root.join("entry");
    create(&file);
    assert!(open_dir(&file).is_err());
    fs::remove_dir_all(root).unwrap();
}
