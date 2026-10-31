//! File operations under a project root: `fe_project::fs_ops`.

use std::{
    fs,
    io::ErrorKind,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};

use fe_project::{
    NameError, create_dir, create_file, duplicate, fs_ops::testing, relative_path, rename, trash,
    validate_name,
};
use tempfile::TempDir;

/// A temporary directory and its canonical path (on macOS `/var` is a link to
/// `/private/var`, and the operations want a root reached without one).
fn temp() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(dir.path()).unwrap();
    (dir, path)
}

/// Every path under `root`, relative and sorted, folders with a trailing `/`,
/// symlinks with their target, and files with their contents.
fn snapshot(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let rel = relative_path(root, &path);
            let kind = fs::symlink_metadata(&path).unwrap().file_type();
            if kind.is_symlink() {
                out.push(format!(
                    "{rel} -> {}",
                    fs::read_link(&path).unwrap().display()
                ));
            } else if kind.is_dir() {
                out.push(format!("{rel}/"));
                walk(root, &path, out);
            } else {
                out.push(format!(
                    "{rel} = {}",
                    fs::read_to_string(&path).unwrap_or_default()
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn write(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

// C1: creating.

#[test]
fn create_file_makes_the_folders_named_above_it() {
    let (_dir, root) = temp();
    let made = create_file(&root, &root, "a/b/c.rs").unwrap();
    assert_eq!(made, root.join("a/b/c.rs"));
    assert!(made.is_file());
    assert_eq!(snapshot(&root), ["a/", "a/b/", "a/b/c.rs = "]);

    // In a folder given relative to the root, or absolute under it, through
    // folders that exist and some that don't.
    let made = create_file(&root, Path::new("a"), "b/d/e.rs").unwrap();
    assert_eq!(made, root.join("a/b/d/e.rs"));
    let made = create_file(&root, &root.join("a/b"), "f.rs").unwrap();
    assert_eq!(made, root.join("a/b/f.rs"));
    assert_eq!(
        snapshot(&root),
        [
            "a/",
            "a/b/",
            "a/b/c.rs = ",
            "a/b/d/",
            "a/b/d/e.rs = ",
            "a/b/f.rs = "
        ]
    );
}

#[test]
fn a_trailing_slash_creates_a_folder() {
    let (_dir, root) = temp();
    let made = create_file(&root, Path::new(""), "src/new/").unwrap();
    assert_eq!(made, root.join("src/new"));
    assert!(made.is_dir());
    let made = create_dir(&root, Path::new("src"), "x/y").unwrap();
    assert_eq!(made, root.join("src/x/y"));
    let made = create_dir(&root, Path::new("src"), "z/").unwrap();
    assert_eq!(made, root.join("src/z"));
    assert_eq!(
        snapshot(&root),
        ["src/", "src/new/", "src/x/", "src/x/y/", "src/z/"]
    );
}

#[test]
fn an_existing_target_is_already_exists_with_nothing_changed() {
    let (_dir, root) = temp();
    write(&root, "a/b/c.rs", "keep");
    write(&root, "file", "plain");
    fs::create_dir(root.join("folder")).unwrap();
    let before = snapshot(&root);

    for (dir, name) in [
        ("", "a/b/c.rs"),
        ("a", "b/c.rs"),
        ("a/b", "c.rs"),
        ("", "a/b/c.rs/"),
        ("", "a/"),
        ("", "file"),
        ("", "folder"),
        ("", "folder/"),
    ] {
        let err = create_file(&root, Path::new(dir), name).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::AlreadyExists, "{dir:?} {name:?}");
    }
    for name in ["folder", "a/b", "file"] {
        let err = create_dir(&root, &root, name).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::AlreadyExists, "{name:?}");
    }
    // A file where a folder should be fails before any folder is made.
    let err = create_file(&root, &root, "file/new/x.rs").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotADirectory);
    let err = create_file(&root, Path::new("file"), "x.rs").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotADirectory);
    assert_eq!(snapshot(&root), before);
}

// C2: names and the root.

#[test]
fn validate_name_refuses_what_is_not_a_name() {
    for (name, err) in [
        ("", NameError::Empty),
        (".", NameError::Dot),
        ("..", NameError::DotDot),
        ("a/./b", NameError::Dot),
        ("a/..", NameError::DotDot),
        ("../x", NameError::DotDot),
        ("a\0b", NameError::Nul),
        ("/", NameError::EmptyComponent),
        ("/a", NameError::EmptyComponent),
        ("a//b", NameError::EmptyComponent),
        ("a//", NameError::EmptyComponent),
    ] {
        assert_eq!(validate_name(name), Err(err), "{name:?}");
    }
    for name in [
        "a", ".env", "a.b.c", "...", "a/b/c.rs", "dir/", "a b", "ünï",
    ] {
        assert_eq!(validate_name(name), Ok(()), "{name:?}");
    }
}

#[test]
fn a_refused_name_creates_nothing() {
    let (_dir, root) = temp();
    for name in [
        "",
        ".",
        "..",
        "../escape.rs",
        "a/../../escape.rs",
        "a//b",
        "/abs",
        "x\0",
    ] {
        let err = create_file(&root, &root, name).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput, "{name:?}");
    }
    assert!(snapshot(&root).is_empty());
    assert!(!root.parent().unwrap().join("escape.rs").exists());
}

#[test]
fn a_target_outside_the_root_through_dot_dot_is_refused() {
    let (_outer, outside) = temp();
    let root = outside.join("project");
    fs::create_dir(&root).unwrap();
    write(&root, "f.txt", "f");
    write(&outside, "other.txt", "other");
    let before = snapshot(&outside);

    let invalid = |result: std::io::Result<()>| {
        assert_eq!(result.unwrap_err().kind(), ErrorKind::InvalidInput);
    };
    invalid(create_file(&root, Path::new(".."), "x.rs").map(drop));
    invalid(create_file(&root, &root.join(".."), "x.rs").map(drop));
    invalid(create_dir(&root, Path::new("sub/../.."), "x").map(drop));
    invalid(create_file(&root, &outside, "x.rs").map(drop));
    invalid(rename(&root, Path::new("../other.txt"), "y.txt").map(drop));
    invalid(rename(&root, &outside.join("other.txt"), "y.txt").map(drop));
    invalid(rename(&root, Path::new("f.txt"), "../y.txt").map(drop));
    invalid(duplicate(&root, Path::new("../other.txt")).map(drop));
    invalid(duplicate(&root, &outside.join("other.txt")).map(drop));
    invalid(trash(&root, &[Path::new("../other.txt")]));
    invalid(trash(&root, &[outside.join("other.txt")]));
    // Nor is the root itself an entry to change.
    invalid(rename(&root, &root, "renamed").map(drop));
    invalid(duplicate(&root, Path::new("")).map(drop));
    invalid(trash(&root, &[Path::new(".")]));
    assert_eq!(snapshot(&outside), before);
}

#[test]
fn a_target_through_a_symlinked_parent_is_refused() {
    let (_outer, outside) = temp();
    let root = outside.join("project");
    fs::create_dir(&root).unwrap();
    fs::create_dir(outside.join("elsewhere")).unwrap();
    write(&outside, "elsewhere/secret.txt", "secret");
    symlink(outside.join("elsewhere"), root.join("link")).unwrap();
    symlink("../elsewhere", root.join("relative-link")).unwrap();
    let before = snapshot(&outside);

    for link in ["link", "relative-link"] {
        let invalid = |result: std::io::Result<()>| {
            let err = result.unwrap_err();
            assert_eq!(err.kind(), ErrorKind::InvalidInput, "{link}: {err}");
        };
        let under = |rel: &str| Path::new(link).join(rel);
        invalid(create_file(&root, Path::new(link), "x.rs").map(drop));
        invalid(create_file(&root, &root, &format!("{link}/x.rs")).map(drop));
        invalid(create_dir(&root, Path::new(link), "x").map(drop));
        invalid(create_dir(&root, &root, &format!("{link}/x/")).map(drop));
        invalid(rename(&root, &under("secret.txt"), "y.txt").map(drop));
        invalid(duplicate(&root, &under("secret.txt")).map(drop));
        invalid(trash(&root, &[under("secret.txt")]));
    }
    assert_eq!(snapshot(&outside), before);
}

#[test]
fn a_symlink_itself_is_renamed_and_duplicated_as_a_link() {
    let (_outer, outside) = temp();
    let root = outside.join("project");
    fs::create_dir(&root).unwrap();
    write(&outside, "elsewhere/secret.txt", "secret");
    symlink("../elsewhere", root.join("link")).unwrap();

    let renamed = rename(&root, Path::new("link"), "renamed").unwrap();
    assert_eq!(renamed, root.join("renamed"));
    let copy = duplicate(&root, &renamed).unwrap();
    assert_eq!(copy, root.join("renamed copy"));
    assert_eq!(
        snapshot(&root),
        ["renamed -> ../elsewhere", "renamed copy -> ../elsewhere"]
    );
    assert_eq!(
        snapshot(&outside.join("elsewhere")),
        ["secret.txt = secret"]
    );
}

// C3: renaming and duplicating.

#[test]
fn rename_renames_in_the_same_folder() {
    let (_dir, root) = temp();
    write(&root, "src/old.rs", "body");
    fs::create_dir(root.join("src/folder")).unwrap();
    write(&root, "src/folder/inner.rs", "inner");

    assert_eq!(
        rename(&root, Path::new("src/old.rs"), "new.rs").unwrap(),
        root.join("src/new.rs")
    );
    assert_eq!(
        rename(&root, &root.join("src/folder"), "moved").unwrap(),
        root.join("src/moved")
    );
    // The same name is no change.
    assert_eq!(
        rename(&root, Path::new("src/new.rs"), "new.rs").unwrap(),
        root.join("src/new.rs")
    );
    assert_eq!(
        snapshot(&root),
        [
            "src/",
            "src/moved/",
            "src/moved/inner.rs = inner",
            "src/new.rs = body"
        ]
    );

    let err = rename(&root, Path::new("src/new.rs"), "a/b.rs").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
    let err = rename(&root, Path::new("src/missing.rs"), "b.rs").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotFound);
}

#[test]
fn rename_refuses_an_existing_target() {
    let (_dir, root) = temp();
    write(&root, "a.txt", "a");
    write(&root, "b.txt", "b");
    fs::create_dir(root.join("dir")).unwrap();
    fs::create_dir(root.join("empty")).unwrap();
    let before = snapshot(&root);

    for (from, to) in [
        ("a.txt", "b.txt"),
        ("a.txt", "dir"),
        ("dir", "a.txt"),
        ("dir", "empty"),
    ] {
        let err = rename(&root, Path::new(from), to).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::AlreadyExists, "{from} -> {to}");
    }
    assert_eq!(snapshot(&root), before);
}

/// Whether names under `dir` match regardless of case.
fn ignores_case(dir: &Path) -> bool {
    let probe = dir.join("case-probe");
    fs::write(&probe, "").unwrap();
    let ignores = dir.join("CASE-PROBE").exists();
    fs::remove_file(probe).unwrap();
    ignores
}

#[test]
fn a_case_only_rename_succeeds_on_a_case_insensitive_volume() {
    let (_dir, root) = temp();
    if !ignores_case(&root) {
        eprintln!(
            "skipped: the temporary volume at {} is case-sensitive, so a case-only rename \
             is a plain one there",
            root.display()
        );
        return;
    }
    write(&root, "readme.md", "read me");
    fs::create_dir(root.join("docs")).unwrap();
    write(&root, "docs/a.md", "a");

    assert_eq!(
        rename(&root, Path::new("readme.md"), "README.md").unwrap(),
        root.join("README.md")
    );
    assert_eq!(
        rename(&root, Path::new("docs"), "Docs").unwrap(),
        root.join("Docs")
    );
    assert_eq!(
        snapshot(&root),
        ["Docs/", "Docs/a.md = a", "README.md = read me"]
    );
    // A name as long as a name can be, through a temporary name that isn't
    // built from it.
    let long = format!("{}.md", "n".repeat(250));
    write(&root, &long, "long");
    let upper = long.to_uppercase();
    assert_eq!(
        rename(&root, Path::new(&long), &upper).unwrap(),
        root.join(&upper)
    );
    fs::remove_file(root.join(&upper)).unwrap();
    // Another entry by the same name but for case is still refused.
    write(&root, "other.md", "other");
    let err = rename(&root, Path::new("other.md"), "readme.md").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
}

#[test]
fn a_case_only_rename_is_a_plain_rename_on_a_case_sensitive_volume() {
    let (_dir, root) = temp();
    if ignores_case(&root) {
        eprintln!(
            "skipped: the temporary volume at {} ignores case",
            root.display()
        );
        return;
    }
    write(&root, "readme.md", "read me");
    assert_eq!(
        rename(&root, Path::new("readme.md"), "README.md").unwrap(),
        root.join("README.md")
    );
    assert_eq!(snapshot(&root), ["README.md = read me"]);
    write(&root, "readme.md", "another");
    let err = rename(&root, Path::new("readme.md"), "README.md").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
}

#[test]
fn renaming_onto_another_link_to_the_same_file_is_refused_and_put_back() {
    // `b` finds the same file as `a`, as `README.md` finds `readme.md` on a
    // case-insensitive volume, but is another entry: the rename through a
    // temporary name finds it taken and puts `a` back.
    let (_dir, root) = temp();
    write(&root, "a", "shared");
    fs::hard_link(root.join("a"), root.join("b")).unwrap();
    let err = rename(&root, Path::new("a"), "b").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
    assert_eq!(snapshot(&root), ["a = shared", "b = shared"]);
}

#[test]
fn duplicate_names_copies_as_the_finder_does() {
    let (_dir, root) = temp();
    write(&root, "src/name.ext", "body");
    write(&root, "src/.env", "env");
    write(&root, "src/archive.tar.gz", "gz");
    write(&root, "src/Makefile", "make");
    let file = root.join("src/name.ext");

    assert_eq!(
        duplicate(&root, &file).unwrap(),
        root.join("src/name copy.ext")
    );
    assert_eq!(
        duplicate(&root, &file).unwrap(),
        root.join("src/name copy 2.ext")
    );
    // A copy's copy counts on.
    assert_eq!(
        duplicate(&root, Path::new("src/name copy.ext")).unwrap(),
        root.join("src/name copy 3.ext")
    );
    assert_eq!(
        duplicate(&root, Path::new("src/.env")).unwrap(),
        root.join("src/.env copy")
    );
    assert_eq!(
        duplicate(&root, Path::new("src/archive.tar.gz")).unwrap(),
        root.join("src/archive.tar copy.gz")
    );
    assert_eq!(
        duplicate(&root, Path::new("src/Makefile")).unwrap(),
        root.join("src/Makefile copy")
    );
    assert_eq!(
        snapshot(&root),
        [
            "src/",
            "src/.env = env",
            "src/.env copy = env",
            "src/Makefile = make",
            "src/Makefile copy = make",
            "src/archive.tar copy.gz = gz",
            "src/archive.tar.gz = gz",
            "src/name copy 2.ext = body",
            "src/name copy 3.ext = body",
            "src/name copy.ext = body",
            "src/name.ext = body",
        ]
    );
}

#[test]
fn duplicate_copies_a_folder_recursively() {
    let (_dir, root) = temp();
    write(&root, "pkg.v2/a.rs", "a");
    write(&root, "pkg.v2/sub/b.rs", "b");
    fs::create_dir(root.join("pkg.v2/empty")).unwrap();
    symlink("sub/b.rs", root.join("pkg.v2/link")).unwrap();
    let script = root.join("pkg.v2/sub/run.sh");
    fs::write(&script, "#!/bin/sh").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o750)).unwrap();

    // A folder's name isn't split at a dot.
    let copy = duplicate(&root, Path::new("pkg.v2")).unwrap();
    assert_eq!(copy, root.join("pkg.v2 copy"));
    let copy = duplicate(&root, Path::new("pkg.v2")).unwrap();
    assert_eq!(copy, root.join("pkg.v2 copy 2"));

    let original = snapshot(&root.join("pkg.v2"));
    assert_eq!(
        original,
        [
            "a.rs = a",
            "empty/",
            "link -> sub/b.rs",
            "sub/",
            "sub/b.rs = b",
            "sub/run.sh = #!/bin/sh"
        ]
    );
    assert_eq!(snapshot(&root.join("pkg.v2 copy")), original);
    assert_eq!(snapshot(&root.join("pkg.v2 copy 2")), original);
    let mode = fs::metadata(root.join("pkg.v2 copy/sub/run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o750);
}

#[test]
fn duplicate_leaves_out_what_is_not_a_file_folder_or_symlink() {
    let (_dir, root) = temp();
    write(&root, "dir/a.rs", "a");
    let fifo = std::ffi::CString::new(root.join("dir/fifo").into_os_string().into_encoded_bytes())
        .unwrap();
    // SAFETY: `fifo` is NUL-terminated.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);

    let copy = duplicate(&root, Path::new("dir")).unwrap();
    assert_eq!(snapshot(&copy), ["a.rs = a"]);
    let err = duplicate(&root, Path::new("dir/fifo")).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unsupported);
    assert!(!root.join("dir/fifo copy").exists());
}

#[test]
fn duplicate_leaves_out_a_socket_in_a_folder_and_refuses_one_alone() {
    let (_dir, root) = temp();
    write(&root, "dir/a.rs", "a");
    let _listener = std::os::unix::net::UnixListener::bind(root.join("dir/sock")).unwrap();

    let copy = duplicate(&root, Path::new("dir")).unwrap();
    assert_eq!(snapshot(&copy), ["a.rs = a"]);
    let err = duplicate(&root, Path::new("dir/sock")).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unsupported);
    assert!(!root.join("dir/sock copy").exists());
}

#[test]
fn duplicate_gives_folders_their_permissions_once_all_is_copied() {
    let (_dir, root) = temp();
    write(&root, "dir/a_read_only/inner.txt", "inner");
    let read_only = root.join("dir/a_read_only");
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o555)).unwrap();

    let copy = duplicate(&root, Path::new("dir")).unwrap();
    assert_eq!(
        snapshot(&copy),
        ["a_read_only/", "a_read_only/inner.txt = inner"]
    );
    let mode = fs::metadata(copy.join("a_read_only"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o555);

    for dir in [&read_only, &copy.join("a_read_only")] {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn a_failed_duplicate_leaves_no_partial_copy() {
    // SAFETY: always safe to call.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root reads an unreadable file, so nothing fails to copy");
        return;
    }
    let (_dir, root) = temp();
    // Copied in name order: the read-only folder is complete, and has its
    // permissions back, before the unreadable file fails.
    write(&root, "dir/a_read_only/inner.txt", "inner");
    write(&root, "dir/z_unreadable", "secret");
    let read_only = root.join("dir/a_read_only");
    let unreadable = root.join("dir/z_unreadable");
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o555)).unwrap();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();

    let err = duplicate(&root, Path::new("dir")).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::PermissionDenied);
    assert!(!root.join("dir copy").exists());

    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        snapshot(&root),
        [
            "dir/",
            "dir/a_read_only/",
            "dir/a_read_only/inner.txt = inner",
            "dir/z_unreadable = secret"
        ]
    );
}

#[test]
fn relative_path_is_slash_separated_under_the_root() {
    let root = Path::new("/work/project");
    assert_eq!(
        relative_path(root, Path::new("/work/project/src/main.rs")),
        "src/main.rs"
    );
    assert_eq!(
        relative_path(root, Path::new("/work/project/./src//a")),
        "src/a"
    );
    assert_eq!(relative_path(root, Path::new("src/lib.rs")), "src/lib.rs");
    assert_eq!(relative_path(root, Path::new("/work/project")), "");
    assert_eq!(
        relative_path(root, Path::new("/elsewhere/x")),
        "/elsewhere/x"
    );
}

// C4: the Trash. These tests' trashes go to the test trash
// (`fs_ops::testing`) unless they hold `testing::real_trash()`. Those that
// reach the user's own Trash (which macOS won't let them empty without Full
// Disk Access) are `#[ignore]`d, run by hand with `--ignored`; those that
// reach a trash of their own temporary folders, on Linux, run.

/// A temporary project the platform's trash takes into the user's Trash, or
/// `None` (saying why) where there is none to use.
fn trashable_temp() -> Option<(TempDir, PathBuf)> {
    #[cfg(target_os = "macos")]
    let base = std::env::temp_dir();
    // The freedesktop.org home trash is `$XDG_DATA_HOME/Trash`; a project on
    // the same volume is trashed there rather than to a `.Trash-$uid` the
    // trash would make at the volume's top.
    #[cfg(target_os = "linux")]
    let base = match std::env::var_os("XDG_DATA_HOME").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => match std::env::var_os("HOME").filter(|dir| !dir.is_empty()) {
            Some(home) => Path::new(&home).join(".local/share"),
            None => {
                eprintln!("skipped: no HOME, so no home trash");
                return None;
            }
        },
    };
    match tempfile::Builder::new()
        .prefix("fe-project-trash-")
        .tempdir_in(&base)
    {
        Ok(dir) => {
            let path = fs::canonicalize(dir.path()).unwrap();
            Some((dir, path))
        }
        Err(err) => {
            eprintln!(
                "skipped: no temporary folder for the trash under {}: {err}",
                base.display()
            );
            None
        }
    }
}

#[test]
fn trash_moves_a_file_and_a_folder_to_the_trash() {
    let (_dir, root) = temp();
    let (gone, folder) = trash_a_file_and_a_folder(&root);
    let found = |name: &str| {
        let found = testing::trashed(name);
        assert_eq!(found.len(), 1, "{name}: {found:?}");
        assert!(found[0].starts_with(testing::trash_dir()));
        found.into_iter().next().unwrap()
    };
    assert_eq!(fs::read_to_string(found(&gone)).unwrap(), "gone");
    assert_eq!(
        snapshot(&found(&folder)),
        ["inner.txt = inner", "sub/", "sub/deep.txt = deep"]
    );
}

/// [`trash_moves_a_file_and_a_folder_to_the_trash`] into the user's Trash
/// (`NSFileManager` on macOS, the freedesktop.org home trash on Linux).
#[test]
#[ignore = "reaches the user's Trash"]
fn trash_moves_a_file_and_a_folder_to_the_system_trash() {
    let _real = testing::real_trash();
    let Some((_dir, root)) = trashable_temp() else {
        return;
    };
    let (gone, folder) = trash_a_file_and_a_folder(&root);
    assert_in_trash_and_purge(&root, &[&folder, &gone]);
}

/// Trashes a file and a folder of `root` (and an entry in that folder, given
/// with it), keeping another; their names.
fn trash_a_file_and_a_folder(root: &Path) -> (String, String) {
    // Names of their own, as the macOS Trash is emptied of them by name.
    let tag = root.file_name().unwrap().to_str().unwrap().to_owned();
    let (gone, folder) = (format!("{tag}.txt"), format!("{tag}-folder"));
    write(root, "keep.txt", "keep");
    write(root, &gone, "gone");
    write(root, &format!("{folder}/inner.txt"), "inner");
    write(root, &format!("{folder}/sub/deep.txt"), "deep");

    // An entry inside another one given goes with it.
    trash(
        root,
        &[
            Path::new(&gone),
            &Path::new(&folder).join("sub/deep.txt"),
            &root.join(&folder),
        ],
    )
    .unwrap();
    assert_eq!(snapshot(root), ["keep.txt = keep"]);

    let err = trash(root, &[Path::new(&gone)]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotFound);
    (gone, folder)
}

/// Off macOS, a project on another volume than the home trash's goes to the
/// trash at the top of its own (`.Trash-$uid` there, made if need be).
#[cfg(target_os = "linux")]
#[test]
fn trash_moves_to_the_volume_trash_off_the_home_volume() {
    use std::os::unix::fs::MetadataExt;

    let _real = testing::real_trash();
    let data = match std::env::var_os("XDG_DATA_HOME").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => match std::env::var_os("HOME").filter(|dir| !dir.is_empty()) {
            Some(home) => Path::new(&home).join(".local/share"),
            None => {
                eprintln!("skipped: no HOME, so no home trash");
                return;
            }
        },
    };
    let (_dir, root) = temp();
    let home_dev = data
        .ancestors()
        .find_map(|dir| fs::metadata(dir).ok())
        .unwrap()
        .dev();
    if fs::metadata(&root).unwrap().dev() == home_dev {
        eprintln!(
            "skipped: the temporary folder {} is on the home trash's volume",
            root.display()
        );
        return;
    }
    let name = format!("{}.txt", root.file_name().unwrap().to_str().unwrap());
    write(&root, &name, "gone");
    match trash(&root, &[Path::new(&name)]) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::PermissionDenied => {
            eprintln!(
                "skipped: no trash of our own at the top of {}: {err}",
                root.display()
            );
            return;
        }
        Err(err) => panic!("{err}"),
    }
    assert!(snapshot(&root).is_empty());
    assert_in_trash_and_purge(&root, &[&name]);
}

/// Checks the Trash holds exactly `names` from `root`, then empties them from
/// it.
#[cfg(target_os = "linux")]
fn assert_in_trash_and_purge(root: &Path, names: &[&str]) {
    let items: Vec<_> = trash::os_limited::list()
        .unwrap()
        .into_iter()
        .filter(|item| item.original_parent == root)
        .collect();
    let mut trashed: Vec<_> = items
        .iter()
        .map(|item| item.name.to_string_lossy())
        .collect();
    trashed.sort();
    assert_eq!(trashed, names);
    trash::os_limited::purge_all(items).unwrap();
}

/// The Trash can't be listed on macOS (reading `~/.Trash` takes Full Disk
/// Access), so this empties the items from it where it can.
#[cfg(target_os = "macos")]
fn assert_in_trash_and_purge(_root: &Path, names: &[&str]) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    for name in names {
        let item = Path::new(&home).join(".Trash").join(name);
        let _ = fs::remove_dir_all(&item).or_else(|_| fs::remove_file(&item));
    }
}

#[test]
fn duplicate_names_near_the_longest_name_fit() {
    let (_dir, root) = temp();
    let file = format!("{}.txt", "f".repeat(251));
    let folder = "d".repeat(255);
    write(&root, &file, "file");
    write(&root, &format!("{folder}/inner"), "inner");

    let copy = duplicate(&root, Path::new(&file)).unwrap();
    let copy_name = copy.file_name().unwrap().to_str().unwrap();
    assert_eq!(copy_name.len(), 255);
    assert!(copy_name.ends_with("f copy.txt"));
    let copy = duplicate(&root, Path::new(&file)).unwrap();
    assert!(copy.to_str().unwrap().ends_with("f copy 2.txt"));
    assert_eq!(fs::read_to_string(&copy).unwrap(), "file");

    let copy = duplicate(&root, Path::new(&folder)).unwrap();
    let copy_name = copy.file_name().unwrap().to_str().unwrap();
    assert_eq!(copy_name.len(), 255);
    assert!(copy_name.ends_with("d copy"));
    assert_eq!(snapshot(&copy), ["inner = inner"]);
}

// Tests of process-wide state run in a child process: this test binary run
// again for one test, which does its work only when `CHILD` is set.

const CHILD: &str = "FE_PROJECT_FS_OPS_CHILD";

/// Runs `test` alone in a child process (behind `wrapper`, a command and its
/// arguments to run it through, if any), with `envs` set. `None`, saying why, if the wrapper
/// couldn't start it; panics if the test failed.
fn run_child(test: &str, wrapper: &[&str], envs: &[(&str, PathBuf)]) -> Option<()> {
    let exe = std::env::current_exe().unwrap();
    let mut command = match wrapper.split_first() {
        Some((program, args)) => {
            let mut command = std::process::Command::new(program);
            command.args(args).arg(&exe);
            command
        }
        None => std::process::Command::new(&exe),
    };
    let output = match command
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .envs(envs.iter().map(|(key, value)| (key, value)))
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            eprintln!("skipped: couldn't run {wrapper:?}: {err}");
            return None;
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() && stdout.contains("1 passed") {
        return Some(());
    }
    // A failed test exits with 101; the wrapper failing to set up, otherwise.
    if output.status.code() != Some(101) && !wrapper.is_empty() {
        eprintln!("skipped: {wrapper:?} couldn't run the test: {stderr}");
        return None;
    }
    panic!("{test} failed in a child process:\n{stdout}\n{stderr}");
}

fn in_child() -> bool {
    std::env::var_os(CHILD).is_some()
}

/// Lowers this process's limit on open files to `limit`.
fn limit_open_files(limit: libc::rlim_t) {
    let mut rlimit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `rlimit` is valid for writes.
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlimit) },
        0
    );
    rlimit.rlim_cur = limit.min(rlimit.rlim_max);
    // SAFETY: `rlimit` is valid for reads.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &rlimit) }, 0);
}

#[test]
fn broad_folders_and_selections_need_few_open_files() {
    run_child(
        "broad_folders_and_selections_need_few_open_files_child",
        &[],
        &[],
    )
    .unwrap();
}

#[test]
fn broad_folders_and_selections_need_few_open_files_child() {
    if !in_child() {
        return;
    }
    const BROAD: usize = 300;
    limit_open_files(64);

    // A copy of a broad folder, then one that fails at its end and is
    // cleaned up.
    let (_dir, root) = temp();
    for ix in 0..BROAD {
        write(&root, &format!("broad/{ix:03}/file"), "x");
    }
    let copy = duplicate(&root, Path::new("broad")).unwrap();
    assert_eq!(fs::read_dir(&copy).unwrap().count(), BROAD);
    // SAFETY: always safe to call.
    if unsafe { libc::geteuid() } != 0 {
        write(&root, "broad/zzz", "secret");
        fs::set_permissions(root.join("broad/zzz"), fs::Permissions::from_mode(0o000)).unwrap();
        let err = duplicate(&root, Path::new("broad")).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::PermissionDenied);
        assert!(!root.join("broad copy 2").exists());
        fs::set_permissions(root.join("broad/zzz"), fs::Permissions::from_mode(0o644)).unwrap();
    }

    // Many entries of one folder, trashed together.
    let (_trash_dir, root) = temp();
    let names: Vec<String> = (0..BROAD).map(|ix| format!("{ix:03}.txt")).collect();
    for name in &names {
        write(&root, name, "x");
    }
    trash(&root, &names).unwrap();
    assert!(snapshot(&root).is_empty());
    // This child process's own test trash holds them all.
    let mut trashed: Vec<_> = fs::read_dir(testing::trash_dir())
        .unwrap()
        .flat_map(|slot| fs::read_dir(slot.unwrap().path()).unwrap())
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    trashed.sort();
    assert_eq!(trashed, names);
}

/// A project on a bind mount of the home trash's file system can't be
/// renamed into the home trash (a rename between mounts fails): it goes to
/// the trash at the top of its own mount, which records its path below that
/// top, so it restores where it was with the volume mounted elsewhere. Run
/// where bubblewrap can make the bind mounts.
#[cfg(target_os = "linux")]
#[test]
fn a_bind_mounted_project_trashes_on_its_own_mount() {
    // The home trash, the folder bind-mounted and the mount, all on the one
    // file system.
    let (_dir, base) = temp();
    fs::create_dir(base.join("home")).unwrap();
    write(&base, "source/project/gone.txt", "gone");
    fs::create_dir(base.join("mount")).unwrap();
    fs::create_dir(base.join("elsewhere")).unwrap();
    let source = base.join("source");
    let bind = |at: &str| {
        [
            "bwrap".to_owned(),
            "--dev-bind".to_owned(),
            "/".to_owned(),
            "/".to_owned(),
            "--bind".to_owned(),
            source.to_str().unwrap().to_owned(),
            base.join(at).to_str().unwrap().to_owned(),
        ]
    };
    let envs = [
        ("XDG_DATA_HOME", base.join("home")),
        ("FE_PROJECT_FS_OPS_BASE", base.clone()),
    ];
    let wrapper = bind("mount");
    let wrapper: Vec<&str> = wrapper.iter().map(String::as_str).collect();
    if run_child(
        "a_bind_mounted_project_trashes_on_its_own_mount_child",
        &wrapper,
        &envs,
    )
    .is_none()
    {
        return;
    }
    assert!(!source.join("project/gone.txt").exists());
    // SAFETY: always safe to call.
    let uid = unsafe { libc::getuid() };
    let trashed = source.join(format!(".Trash-{uid}"));
    assert_eq!(
        fs::read_to_string(trashed.join("files/gone.txt")).unwrap(),
        "gone"
    );
    let info = fs::read_to_string(trashed.join("info/gone.txt.trashinfo")).unwrap();
    assert!(info.contains("\nPath=project/gone.txt\n"), "{info}");
    assert!(fs::read_dir(base.join("home")).unwrap().next().is_none());

    // The volume mounted elsewhere: the trash lists the entry as from there.
    let wrapper = bind("elsewhere");
    let wrapper: Vec<&str> = wrapper.iter().map(String::as_str).collect();
    run_child(
        "a_bind_mounted_project_trashes_on_its_own_mount_remounted_child",
        &wrapper,
        &envs,
    )
    .expect("bubblewrap made the first mount");
}

#[cfg(target_os = "linux")]
#[test]
fn a_bind_mounted_project_trashes_on_its_own_mount_child() {
    if !in_child() {
        return;
    }
    let _real = testing::real_trash();
    let base = PathBuf::from(std::env::var_os("FE_PROJECT_FS_OPS_BASE").unwrap());
    let root = base.join("mount/project");
    trash(&root, &[Path::new("gone.txt")]).unwrap();
    assert!(snapshot(&root).is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn a_bind_mounted_project_trashes_on_its_own_mount_remounted_child() {
    if !in_child() {
        return;
    }
    let base = PathBuf::from(std::env::var_os("FE_PROJECT_FS_OPS_BASE").unwrap());
    let parents: Vec<_> = trash::os_limited::list()
        .unwrap()
        .into_iter()
        .filter(|item| item.name == "gone.txt" && item.original_parent.starts_with(&base))
        .map(|item| item.original_parent)
        .collect();
    assert_eq!(parents, [base.join("elsewhere/project")]);
}

/// As [`a_bind_mounted_project_trashes_on_its_own_mount`], where `statx`
/// fails (as before Linux 5.8, or under a seccomp policy that denies it), so
/// it can't tell the project's mount: the mount is told from
/// `/proc/self/fdinfo` instead, never taken to be the home trash's for being
/// on its device, whose rename into the home trash would fail.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
fn a_bind_mounted_project_trashes_on_its_own_mount_without_statx() {
    let (_dir, base) = temp();
    fs::create_dir(base.join("home")).unwrap();
    write(&base, "source/project/gone.txt", "gone");
    fs::create_dir(base.join("mount")).unwrap();
    let source = base.join("source");
    let mount = base.join("mount");
    let wrapper = [
        "bwrap",
        "--dev-bind",
        "/",
        "/",
        "--bind",
        source.to_str().unwrap(),
        mount.to_str().unwrap(),
    ];
    let envs = [
        ("XDG_DATA_HOME", base.join("home")),
        ("FE_PROJECT_FS_OPS_BASE", base.clone()),
    ];
    if run_child(
        "a_bind_mounted_project_trashes_on_its_own_mount_without_statx_child",
        &wrapper,
        &envs,
    )
    .is_none()
    {
        return;
    }
    // SAFETY: always safe to call.
    let uid = unsafe { libc::getuid() };
    let trashed = source.join(format!(".Trash-{uid}"));
    assert_eq!(
        fs::read_to_string(trashed.join("files/gone.txt")).unwrap(),
        "gone"
    );
    assert!(fs::read_dir(base.join("home")).unwrap().next().is_none());
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
fn a_bind_mounted_project_trashes_on_its_own_mount_without_statx_child() {
    if !in_child() {
        return;
    }
    deny_statx();
    let _real = testing::real_trash();
    let base = PathBuf::from(std::env::var_os("FE_PROJECT_FS_OPS_BASE").unwrap());
    let root = base.join("mount/project");
    trash(&root, &[Path::new("gone.txt")]).unwrap();
    assert!(snapshot(&root).is_empty());
}

/// Makes `statx` fail with `ENOSYS` on this thread from now on, as on a
/// kernel without it, through a seccomp filter.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn deny_statx() {
    // `AUDIT_ARCH_*` in <linux/audit.h>.
    #[cfg(target_arch = "x86_64")]
    const ARCH: u32 = 0xC000_003E;
    #[cfg(target_arch = "aarch64")]
    const ARCH: u32 = 0xC000_00B7;
    // The offsets of `nr` and `arch` in `struct seccomp_data`.
    const NR: u32 = 0;
    const ARCH_AT: u32 = 4;
    let op = |code: u32, k: u32, jt: u8, jf: u8| libc::sock_filter {
        code: code as u16,
        jt,
        jf,
        k,
    };
    let load = libc::BPF_LD | libc::BPF_W | libc::BPF_ABS;
    let equal = libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K;
    let ret = libc::BPF_RET | libc::BPF_K;
    let filter = [
        // Another architecture's calls are let through.
        op(load, ARCH_AT, 0, 0),
        op(equal, ARCH, 1, 0),
        op(ret, libc::SECCOMP_RET_ALLOW, 0, 0),
        op(load, NR, 0, 0),
        op(equal, libc::SYS_statx as u32, 0, 1),
        op(ret, libc::SECCOMP_RET_ERRNO | libc::ENOSYS as u32, 0, 0),
        op(ret, libc::SECCOMP_RET_ALLOW, 0, 0),
    ];
    let program = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_ptr().cast_mut(),
    };
    // SAFETY: `program` points to `filter`, which outlives the calls; the
    // kernel copies it.
    unsafe {
        assert_eq!(libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        assert_eq!(
            libc::prctl(
                libc::PR_SET_SECCOMP,
                libc::SECCOMP_MODE_FILTER,
                &raw const program,
            ),
            0,
            "{}",
            std::io::Error::last_os_error()
        );
    }
}

/// A copy made under a umask that takes its owner's access from new folders
/// still fills them, gives them the source's modes, and is removed whole
/// should it fail.
#[test]
fn duplicate_works_under_a_restrictive_umask() {
    run_child("duplicate_works_under_a_restrictive_umask_child", &[], &[]).unwrap();
}

#[test]
fn duplicate_works_under_a_restrictive_umask_child() {
    if !in_child() {
        return;
    }
    let (_dir, root) = temp();
    write(&root, "dir/sub/inner.txt", "inner");
    write(&root, "dir/a.txt", "a");
    // SAFETY: this child process runs this one test on one thread.
    unsafe { libc::umask(0o700) };

    // Off Linux nothing opens a folder its owner can't read, and its
    // permissions are never changed by name: the copy fails, leaving nothing.
    // SAFETY: always safe to call.
    if cfg!(not(target_os = "linux")) && unsafe { libc::geteuid() } != 0 {
        let err = duplicate(&root, Path::new("dir")).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::PermissionDenied, "{err}");
        assert_eq!(
            snapshot(&root),
            [
                "dir/",
                "dir/a.txt = a",
                "dir/sub/",
                "dir/sub/inner.txt = inner"
            ]
        );
        return;
    }

    let copy = duplicate(&root, Path::new("dir")).unwrap();
    assert_eq!(
        snapshot(&copy),
        ["a.txt = a", "sub/", "sub/inner.txt = inner"]
    );
    for rel in ["", "sub", "a.txt", "sub/inner.txt"] {
        let mode = |dir: &Path| fs::metadata(dir.join(rel)).unwrap().permissions().mode();
        assert_eq!(mode(&copy), mode(&root.join("dir")), "{rel:?}");
    }

    // SAFETY: always safe to call.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    write(&root, "dir/zzz", "secret");
    fs::set_permissions(root.join("dir/zzz"), fs::Permissions::from_mode(0o000)).unwrap();
    let err = duplicate(&root, Path::new("dir")).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::PermissionDenied);
    assert!(!root.join("dir copy 2").exists());
    fs::set_permissions(root.join("dir/zzz"), fs::Permissions::from_mode(0o644)).unwrap();
}

/// Under a umask that takes its owner's access from new folders, creating a
/// file below a new folder fails, as the folder can't be opened: the folder
/// made is still removed.
#[test]
fn create_removes_its_folders_under_a_restrictive_umask() {
    run_child(
        "create_removes_its_folders_under_a_restrictive_umask_child",
        &[],
        &[],
    )
    .unwrap();
}

#[test]
fn create_removes_its_folders_under_a_restrictive_umask_child() {
    if !in_child() {
        return;
    }
    // SAFETY: always safe to call.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let (_dir, root) = temp();
    // SAFETY: this child process runs this one test on one thread.
    unsafe { libc::umask(0o700) };
    let err = create_file(&root, &root, "a/b.txt").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::PermissionDenied, "{err}");
    assert!(snapshot(&root).is_empty());
}
