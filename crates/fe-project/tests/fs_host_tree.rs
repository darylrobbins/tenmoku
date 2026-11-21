//! The project tree's operations through `&dyn FsHost` ([`LocalTreeFs`])
//! give what fs_ops's and the listing's free functions give: each is run on
//! one of two copies of a small fixture tree, the free function on the
//! other, and the results compared by their paths under the root and by
//! what the entries are. Trashes go to the test trash of the process's own,
//! never the user's.
#![cfg(unix)]

use std::{
    ffi::OsString,
    fs, io,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

use fe_host::{FsHost, LocalFs};
use fe_project::{FileTree, LocalTreeFs, TreeOptions, fs_ops};

/// The host under test, as the workbench will hold it.
fn host() -> Box<dyn FsHost> {
    Box::new(LocalTreeFs::new())
}

/// A temporary project, by its canonical path, holding `src/main.rs`,
/// `src/lib/` with `mod.rs`, an ignored `src/debug.log`, `notes.txt`, an
/// ignored `target/` and a `.gitignore` naming them.
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("fe-project-host-tree-")
        .tempdir()
        .unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join("src/lib")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("src/lib/mod.rs"), "").unwrap();
    fs::write(root.join("src/debug.log"), "").unwrap();
    fs::write(root.join("notes.txt"), "notes\n").unwrap();
    fs::create_dir(root.join("target")).unwrap();
    fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
    (dir, root)
}

/// Two copies of the fixture: one for the free function, one for the host.
fn twins() -> ((tempfile::TempDir, PathBuf), (tempfile::TempDir, PathBuf)) {
    (fixture(), fixture())
}

/// `path` under `root`.
fn rel<'a>(root: &Path, path: &'a Path) -> &'a Path {
    path.strip_prefix(root).unwrap()
}

/// What the entry at `path` is, as the workbench looks it up.
fn id(path: &Path) -> fs_ops::EntryId {
    fs_ops::entry_identity(path).unwrap()
}

/// The names under `root`, sorted, folders with a trailing `/`.
fn names(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let mut name = rel(root, &path).to_string_lossy().into_owned();
            if fs::symlink_metadata(&path).unwrap().is_dir() {
                name.push('/');
                dirs.push(path);
            }
            names.push(name);
        }
    }
    names.sort();
    names
}

#[test]
fn listing_through_the_host_is_the_listing_run() {
    for show_ignored in [false, true] {
        let (_dir, root) = fixture();
        // The tree lists its root as it opens.
        let tree = FileTree::open(&root, TreeOptions { show_ignored }).unwrap();
        let listing = tree.listing(tree.find("src").unwrap()).unwrap();
        let here = listing.run();
        let there = listing.run_on(&*host());
        assert_eq!(there.result().unwrap(), here.result().unwrap());
        assert_eq!(
            there.result().unwrap().iter().any(|entry| entry.ignored),
            show_ignored
        );
    }
}

#[test]
fn a_listing_fills_the_tree_as_the_run_does() {
    let (_dir, root) = fixture();
    let mut here = FileTree::open(&root, TreeOptions::default()).unwrap();
    let mut there = FileTree::open(&root, TreeOptions::default()).unwrap();
    for (tree, host) in [(&mut here, None), (&mut there, Some(host()))] {
        let mut owed: Vec<_> = tree.find("src").into_iter().collect();
        while let Some(dir) = owed.pop() {
            let Some(listing) = tree.listing(dir) else {
                continue;
            };
            let listed = match &host {
                Some(host) => listing.run_on(&**host),
                None => listing.run(),
            };
            tree.fill(dir, listed);
            owed.extend(tree.find("src/lib"));
        }
    }
    let rows = |tree: &FileTree| -> Vec<String> {
        tree.rows(0..usize::MAX)
            .map(|row| format!("{} {:?}", row.name.to_string_lossy(), row.kind))
            .collect()
    };
    assert_eq!(rows(&there), rows(&here));
}

#[test]
fn a_listing_of_a_folder_gone_is_lost_as_the_run_says() {
    let (_dir, root) = fixture();
    let tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    let src = tree.find("src").unwrap();
    let listing = tree.listing(src).unwrap();
    fs::rename(root.join("src"), root.join("moved")).unwrap();
    fs::create_dir(root.join("src")).unwrap();
    let here = listing.run();
    let there = listing.run_on(&*host());
    assert_eq!(
        there.result().unwrap_err().to_string(),
        here.result().unwrap_err().to_string()
    );
}

#[test]
fn create_recorded_through_the_host_makes_what_the_free_function_makes() {
    let ((_a, here), (_b, there)) = twins();
    for (name, contents, folder) in [
        ("src/new/deep.rs", &b"fn f() {}\n"[..], false),
        ("docs/", &b""[..], false),
        ("assets", &b""[..], true),
    ] {
        let made = fs_ops::create_recorded_with(&here, &here, name, contents, folder).unwrap();
        let hosted = host()
            .create_recorded(&there, &there, name, contents, folder)
            .unwrap();
        assert_eq!(rel(&there, &hosted.path), rel(&here, &made.path));
        assert_eq!(
            rel(&there, &hosted.first.path),
            rel(&here, &made.first.path)
        );
        assert_eq!(hosted.first.id, id(&hosted.first.path));
    }
    assert_eq!(
        fs::read(there.join("src/new/deep.rs")).unwrap(),
        b"fn f() {}\n"
    );
    assert_eq!(names(&there), names(&here));
}

#[test]
fn an_existing_name_fails_a_create_through_the_host_too() {
    let ((_a, here), (_b, there)) = twins();
    let free = fs_ops::create_recorded(&here, &here, "notes.txt", false).unwrap_err();
    let hosted = (host().create_recorded(&there, &there, "notes.txt", b"", false)).unwrap_err();
    assert_eq!(hosted.kind(), free.kind());
    assert_eq!(hosted.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn rename_through_the_host_renames_as_the_free_function_does() {
    let ((_a, here), (_b, there)) = twins();
    let renamed = fs_ops::rename(&here, &here.join("notes.txt"), "README.md").unwrap();
    let before = id(&there.join("notes.txt"));
    let hosted = host()
        .rename(&there, &there.join("notes.txt"), "README.md")
        .unwrap();
    assert_eq!(rel(&there, &hosted), rel(&here, &renamed));
    assert_eq!(id(&hosted), before);
    assert_eq!(names(&there), names(&here));
}

#[test]
fn duplicate_recorded_through_the_host_copies_as_the_free_function_does() {
    let ((_a, here), (_b, there)) = twins();
    for path in ["src/main.rs", "src/main.rs", "src/lib"] {
        let copy = fs_ops::duplicate_recorded(&here, &here.join(path)).unwrap();
        let hosted = host()
            .duplicate_recorded(&there, &there.join(path))
            .unwrap();
        assert_eq!(rel(&there, &hosted.path), rel(&here, &copy.path));
        assert_eq!(hosted.id, id(&hosted.path));
    }
    assert_eq!(names(&there), names(&here));
}

#[test]
fn copy_into_recorded_through_the_host_copies_as_the_free_function_does() {
    let ((_a, here), (_b, there)) = twins();
    for (path, dir) in [("src/lib", ""), ("notes.txt", "src"), ("notes.txt", "")] {
        let copy = fs_ops::copy_into_recorded(&here, &here.join(path), &here.join(dir)).unwrap();
        let hosted = host()
            .copy_into_recorded(&there, &there.join(path), &there.join(dir))
            .unwrap();
        assert_eq!(rel(&there, &hosted.path), rel(&here, &copy.path));
        assert_eq!(hosted.id, id(&hosted.path));
    }
    assert_eq!(names(&there), names(&here));
}

#[test]
fn a_folder_copied_into_itself_is_refused_through_the_host_too() {
    let ((_a, here), (_b, there)) = twins();
    let free =
        fs_ops::copy_into_recorded(&here, &here.join("src"), &here.join("src/lib")).unwrap_err();
    let hosted = host()
        .copy_into_recorded(&there, &there.join("src"), &there.join("src/lib"))
        .unwrap_err();
    assert_eq!(hosted.kind(), free.kind());
    assert_eq!(hosted.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(names(&there), names(&here));
}

#[test]
fn paste_names_through_the_host_are_the_free_functions() {
    let ((_a, here), (_b, there)) = twins();
    let planned = [OsString::from("notes copy.txt")];
    for (path, dir, planned) in [
        ("notes.txt", "src", &[][..]),
        ("notes.txt", "", &[][..]),
        ("notes.txt", "", &planned[..]),
        ("src/lib", "", &[][..]),
    ] {
        let free =
            fs_ops::paste_name_besides(&here, &here.join(path), &here.join(dir), planned).unwrap();
        let hosted = host()
            .paste_name(&there, &there.join(path), &there.join(dir), planned)
            .unwrap();
        assert_eq!(hosted, free);
    }
    assert_eq!(
        fs_ops::paste_name(&here, &here.join("notes.txt"), &here).unwrap(),
        host()
            .paste_name(&there, &there.join("notes.txt"), &there, &[])
            .unwrap()
    );
}

#[test]
fn move_into_through_the_host_moves_as_the_free_function_does() {
    let ((_a, here), (_b, there)) = twins();
    let moved =
        fs_ops::move_into(&here, &here.join("notes.txt"), &here.join("src"), "n.txt").unwrap();
    let before = id(&there.join("notes.txt"));
    let hosted = host()
        .move_into(
            &there,
            &there.join("notes.txt"),
            &there.join("src"),
            "n.txt".as_ref(),
        )
        .unwrap();
    assert_eq!(rel(&there, &hosted), rel(&here, &moved));
    assert_eq!(id(&hosted), before);
    assert_eq!(names(&there), names(&here));
}

#[test]
fn a_move_onto_an_entry_fails_through_the_host_too() {
    let ((_a, here), (_b, there)) = twins();
    let free = fs_ops::move_into(&here, &here.join("notes.txt"), &here.join("src"), "main.rs")
        .unwrap_err();
    let hosted = host()
        .move_into(
            &there,
            &there.join("notes.txt"),
            &there.join("src"),
            "main.rs".as_ref(),
        )
        .unwrap_err();
    assert_eq!(hosted.kind(), free.kind());
    assert_eq!(hosted.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(names(&there), names(&here));
}

#[test]
fn exists_through_the_host_is_the_free_functions() {
    let (_dir, root) = fixture();
    symlink(root.join("src"), root.join("linked")).unwrap();
    symlink("nowhere", root.join("dangling")).unwrap();
    let host = host();
    for path in ["notes.txt", "src/lib", "dangling", "missing", "notes.txt/x"] {
        assert_eq!(
            host.exists(&root, &root.join(path)).unwrap(),
            fs_ops::exists(&root, &root.join(path)).unwrap(),
            "{path}"
        );
    }
    let free = fs_ops::exists(&root, &root.join("linked/main.rs")).unwrap_err();
    let hosted = host
        .exists(&root, &root.join("linked/main.rs"))
        .unwrap_err();
    assert_eq!(hosted.kind(), free.kind());
    assert_eq!(hosted.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn a_path_leaving_the_root_is_refused_through_the_host_as_by_the_free_function() {
    let (_dir, root) = fixture();
    let outside = root.join("src/../../elsewhere.txt");
    let free = fs_ops::rename(&root, &outside, "x").unwrap_err();
    let hosted = host().rename(&root, &outside, "x").unwrap_err();
    assert_eq!(hosted.kind(), free.kind());
    assert_eq!(hosted.kind(), io::ErrorKind::InvalidInput);
    let free = fs_ops::trash_recorded(&root, &[&outside]).unwrap_err();
    let hosted = host().trash_recorded(&root, &[outside]).unwrap_err();
    assert_eq!(hosted.kind(), free.kind());
}

#[test]
fn truncate_through_the_host_empties_the_file_in_place() {
    let ((_a, here), (_b, there)) = twins();
    fs_ops::open_truncated(&here, &here.join("src/main.rs")).unwrap();
    let before = id(&there.join("src/main.rs"));
    host().truncate(&there, &there.join("src/main.rs")).unwrap();
    assert_eq!(fs::read(there.join("src/main.rs")).unwrap(), b"");
    assert_eq!(fs::read(here.join("src/main.rs")).unwrap(), b"");
    assert_eq!(id(&there.join("src/main.rs")), before);
    let free = fs_ops::open_truncated(&here, &here.join("src")).unwrap_err();
    let hosted = host().truncate(&there, &there.join("src")).unwrap_err();
    assert_eq!(hosted.kind(), free.kind());
}

#[test]
fn trash_recorded_through_the_host_records_what_the_free_function_does() {
    let ((_a, here), (_b, there)) = twins();
    let paths = ["src/lib", "src/lib/mod.rs", "notes.txt"];
    let ids: Vec<_> = paths.iter().map(|path| id(&there.join(path))).collect();
    let free = fs_ops::trash_recorded(&here, &paths.map(|path| here.join(path))).unwrap();
    let hosted = host()
        .trash_recorded(&there, &paths.map(|path| there.join(path)))
        .unwrap();
    // One record for each entry moved, `mod.rs` going with its folder, in
    // the order of their paths.
    let rels = |root: &Path, paths: Vec<&Path>| -> Vec<PathBuf> {
        paths
            .into_iter()
            .map(|path| rel(root, path).to_path_buf())
            .collect()
    };
    assert_eq!(
        rels(&there, hosted.iter().map(|record| &*record.path).collect()),
        rels(&here, free.iter().map(fs_ops::Trashed::path).collect()),
    );
    assert_eq!(
        hosted.iter().map(|record| record.id).collect::<Vec<_>>(),
        [ids[2], ids[0]]
    );
    assert!(hosted.iter().all(|record| record.at.is_some()));
    assert_eq!(names(&there), names(&here));
}

#[test]
fn trash_then_restore_through_the_host_keeps_the_entry_id() {
    let (_dir, root) = fixture();
    let host = host();
    assert!(host.restores_from_trash());
    assert_eq!(host.restores_from_trash(), fs_ops::restores_from_trash());
    let path = root.join("src");
    let before = id(&path);
    let mut trashed = host
        .trash_recorded(&root, std::slice::from_ref(&path))
        .unwrap();
    assert_eq!(trashed.len(), 1);
    let mut record = trashed.pop().unwrap();
    assert_eq!(record.id, before);
    assert!(host.in_trash(&record).unwrap());
    assert!(!path.exists());

    let restored = host.restore(&root, &mut record).unwrap();
    assert_eq!(restored, path);
    assert_eq!(id(&path), before);
    assert_eq!(record.id, before);
    assert!(!host.in_trash(&record).unwrap());
    assert!(path.join("lib/mod.rs").is_file());
}

#[test]
fn a_restore_onto_a_taken_path_fails_through_the_host_and_stays_in_the_trash() {
    let (_dir, root) = fixture();
    let host = host();
    let path = root.join("notes.txt");
    let mut record = host
        .trash_recorded(&root, std::slice::from_ref(&path))
        .unwrap()
        .remove(0);
    fs::write(&path, "theirs\n").unwrap();
    let error = host.restore(&root, &mut record).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&path).unwrap(), b"theirs\n");
    assert!(host.in_trash(&record).unwrap());
}

#[test]
fn a_record_restores_through_the_free_function_and_back() {
    let (_dir, root) = fixture();
    let host = host();
    let path = root.join("notes.txt");
    let before = id(&path);
    let record = host
        .trash_recorded(&root, std::slice::from_ref(&path))
        .unwrap()
        .remove(0);
    let mut trashed = fs_ops::Trashed::from(record.clone());
    assert_eq!(trashed.id(), before);
    assert_eq!(trashed.path(), path);
    assert_eq!(trashed.location(), record.at.as_deref());
    assert_eq!(fe_host::TrashRecord::from(trashed.clone()), record);
    fs_ops::restore(&root, &mut trashed).unwrap();
    assert_eq!(id(&path), before);
}

#[test]
fn fe_hosts_local_fs_has_no_tree_operations() {
    let (_dir, root) = fixture();
    let error = LocalFs.exists(&root, &root.join("notes.txt")).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert!(!LocalFs.restores_from_trash());
    assert!(root.join("notes.txt").is_file());
}

#[test]
fn the_local_hosts_list_the_tree() {
    let (_dir, root) = fixture();
    let hosts = fe_project::local_hosts();
    assert!(hosts.fs.exists(&root, &root.join("notes.txt")).unwrap());
    let tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    let listing = tree.listing(tree.find("src").unwrap()).unwrap();
    let listed = listing.run_on(&*hosts.fs);
    assert_eq!(listed.result().unwrap(), listing.run().result().unwrap());
}

/// A request for `rel` under `root` with `ids` unknown ids, as no tree
/// would make it.
fn request(root: &Path, rel: &str, ids: usize) -> fe_host::ListRequest {
    fe_host::ListRequest {
        root: root.to_path_buf(),
        rel: PathBuf::from(rel),
        show_ignored: true,
        parent_ignored: false,
        ids: vec![None; ids],
        global_excludes: None,
    }
}

/// The error a request fails with, which reads nothing.
fn refused(request: &fe_host::ListRequest) -> io::Error {
    let reply = host().list_tree(request);
    assert_eq!((reply.id, reply.lost), (None, None));
    reply.result.unwrap_err()
}

#[test]
fn a_listing_of_a_parent_component_is_refused() {
    let (_dir, root) = fixture();
    for (rel, ids) in [("..", 2), ("src/../..", 4), ("src/..", 3), ("./src", 3)] {
        let error = refused(&request(&root.join("src"), rel, ids));
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{rel}");
        assert_eq!(
            fe_host::RequestError::of(&error),
            Some(fe_host::RequestError::LeavesTheRoot)
        );
    }
}

#[test]
fn a_listing_of_an_absolute_folder_is_refused() {
    let (_dir, root) = fixture();
    let error = refused(&request(&root, &root.join("src").to_string_lossy(), 2));
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn a_listing_with_too_many_ids_is_refused() {
    let (_dir, root) = fixture();
    let error = refused(&request(&root, "src", 3));
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        fe_host::RequestError::of(&error),
        Some(fe_host::RequestError::IdsMismatch)
    );
}

#[test]
fn a_listing_with_too_few_ids_is_refused() {
    let (_dir, root) = fixture();
    for (rel, ids) in [("src/lib", 2), ("src", 1), ("", 0)] {
        let error = refused(&request(&root, rel, ids));
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{rel}");
        assert_eq!(
            fe_host::RequestError::of(&error),
            Some(fe_host::RequestError::IdsMismatch)
        );
    }
}

#[test]
fn a_well_formed_request_lists_through_the_host() {
    let (_dir, root) = fixture();
    let listed = host().list_tree(&request(&root, "src/lib", 3));
    let names: Vec<_> = (listed.result.unwrap().into_iter())
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, ["mod.rs"]);
}

#[test]
fn a_trees_listings_carry_the_excludes_it_read_when_it_opened() {
    let (_dir, root) = fixture();
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    let src = tree.find("src").unwrap();
    let first = tree.listing(src).unwrap();
    tree.fill(src, first.run_on(&*host()));
    let lib = tree.find("src/lib").unwrap();
    let later = tree.listing(lib).unwrap();
    // One snapshot for the tree, whatever git's configuration says since.
    let (first, later) = (first.request(), later.request());
    let (first, later) = (
        first.global_excludes.unwrap(),
        later.global_excludes.unwrap(),
    );
    assert!(first.same(&later));
}

#[test]
fn the_host_lists_with_the_excludes_the_request_carries() {
    let (_dir, root) = fixture();
    let tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    let mut request = tree.listing(tree.find("src").unwrap()).unwrap().request();
    // Excludes that ignore `*.rs`, as a global file changed since the
    // host's own read could: the request's are the ones used.
    let mut builder = ignore::gitignore::GitignoreBuilder::new(&root);
    builder.add_line(None, "*.rs").unwrap();
    let excludes = std::sync::Arc::new(builder.build().unwrap());
    request.global_excludes = Some(fe_host::Excludes::new(excludes));
    let names: Vec<_> = (host().list_tree(&request).result.unwrap().into_iter())
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, ["lib"]);
}

#[test]
fn excludes_the_host_cant_read_are_refused() {
    let (_dir, root) = fixture();
    let mut request = request(&root, "src", 2);
    request.global_excludes = Some(fe_host::Excludes::new(std::sync::Arc::new(1_u8)));
    let error = refused(&request);
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        fe_host::RequestError::of(&error),
        Some(fe_host::RequestError::NotExcludes)
    );
}
