use std::{
    cmp::Ordering,
    ffi::OsStr,
    fs, io,
    os::unix::fs::{PermissionsExt, symlink},
    thread,
};

use super::*;
use crate::listing::{self, Lost, Point};

#[test]
fn open_lists_only_the_root() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "Zeta.txt",
            "alpha.txt",
            "Beta.txt",
            "src/main.rs",
            "src/nested/deep.rs",
            "Docs/",
            "_build.sh",
        ],
    );
    let tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    assert_eq!(
        layout(&tree),
        [
            "> Docs",
            "> src",
            "_build.sh",
            "alpha.txt",
            "Beta.txt",
            "Zeta.txt"
        ]
    );
    let stats = tree.stats();
    assert_eq!(stats.listings, 1);
    assert_eq!(stats.loaded_entries, 6);
    assert_eq!(
        tree.listing(id(&tree, "src")).unwrap().dir(),
        root.join("src")
    );
    assert!(tree.children(id(&tree, "src")).is_none());

    // Names equal but for case are ordered by their bytes, never equal.
    let file = EntryKind::File;
    let cmp = |a: &str, b: &str| listing::compare(file, OsStr::new(a), file, OsStr::new(b));
    assert_eq!(cmp("README", "readme"), Ordering::Less);
    assert_eq!(cmp("readme", "README.md"), Ordering::Less);
    assert_eq!(cmp("b", "B"), Ordering::Greater);
}

#[test]
fn expand_lists_a_directory_once() {
    let (_dir, root) = temp();
    make(&root, &["src/main.rs", "src/lib.rs", "src/util/", "z.txt"]);
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    let before = tree.stats();
    let src = id(&tree, "src");

    tree.expand(src);
    assert_eq!(
        layout(&tree),
        ["v src", "  > util", "  lib.rs", "  main.rs", "z.txt"]
    );
    assert_eq!(tree.stats().listings, before.listings + 1);
    assert_eq!(tree.stats().loaded_entries, before.loaded_entries + 3);

    tree.collapse(src);
    assert_eq!(layout(&tree), ["> src", "z.txt"]);
    assert_eq!(tree.stats().loaded_entries, before.loaded_entries + 3);

    tree.expand(src);
    assert_eq!(
        layout(&tree),
        ["v src", "  > util", "  lib.rs", "  main.rs", "z.txt"]
    );
    assert_eq!(tree.stats().listings, before.listings + 1);

    // Expanding a file does nothing.
    let version = tree.version();
    tree.expand(id(&tree, "z.txt"));
    assert_eq!(tree.version(), version);
    assert_eq!(tree.stats().listings, before.listings + 1);
}

#[test]
fn listing_runs_off_the_tree() {
    const fn assert_send<T: Send>() {}
    const _: () = assert_send::<Listing>();

    let (_dir, root) = temp();
    make(&root, &["src/b.rs", "src/A.rs", "src/mod/", "src/.hidden"]);
    let mut expanded = FileTree::open(&root, TreeOptions::default()).unwrap();
    expanded.expand(id(&expanded, "src"));

    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    let src = id(&tree, "src");
    let listing = tree.listing(src).unwrap();
    let result = thread::spawn(move || listing.run()).join().unwrap();
    let listings = tree.stats().listings;
    tree.fill(src, result);
    assert_eq!(tree.stats().listings, listings + 1);
    assert_eq!(layout(&tree), ["> src"]);
    tree.expand(src);
    assert_eq!(tree.stats().listings, listings + 1);
    assert_eq!(layout(&tree), layout(&expanded));
    assert_eq!(
        layout(&tree),
        ["v src", "  > mod", "  .hidden", "  A.rs", "  b.rs"]
    );

    assert!(tree.listing(src).is_none());
    assert!(tree.listing(tree.root()).is_none());
    assert!(tree.listing(id(&tree, "src/A.rs")).is_none());

    // A change that arrives while a listing runs can't be applied to the
    // unlisted directory, so the listing is offered again after its fill.
    let module = id(&tree, "src/mod");
    let listing = tree.listing(module).unwrap();
    let listed = listing.run();
    make(&root, &["src/mod/late.rs"]);
    let stats = tree.stats();
    tree.apply(&changes(&root, &["src/mod/late.rs"]));
    assert_eq!(tree.stats(), stats);
    tree.fill(module, listed);
    assert!(tree.children(module).unwrap().is_empty());
    let listing = tree.listing(module).unwrap();
    tree.fill(module, listing.run());
    assert!(tree.listing(module).is_none());
    tree.expand(module);
    assert_eq!(tree.stats().listings, stats.listings + 2);
    assert_eq!(layout(&tree)[..3], ["v src", "  v mod", "    late.rs"]);
}

#[test]
fn ignored_entries_and_git_are_hidden() {
    let files = [
        ".gitignore",
        ".ignore",
        ".env",
        "keep.txt",
        "app.log",
        "secret.txt",
        "build/out.o",
        "sub/.gitignore",
        "sub/main.rs",
        "sub/local.tmp",
        "sub/x.log",
    ];
    for in_repo in [false, true] {
        let (_dir, root) = temp();
        make(&root, &files);
        if in_repo {
            make(&root, &[".git/HEAD", ".git/objects/"]);
        }
        fs::write(root.join(".gitignore"), "*.log\nbuild/\n").unwrap();
        fs::write(root.join(".ignore"), "secret.txt\n").unwrap();
        fs::write(root.join("sub/.gitignore"), "local.tmp\n").unwrap();

        let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
        tree.expand(id(&tree, "sub"));
        assert_eq!(
            layout(&tree),
            [
                "v sub",
                "  .gitignore",
                "  main.rs",
                ".env",
                ".gitignore",
                ".ignore",
                "keep.txt",
            ],
            "in a repo: {in_repo}"
        );
        // Created files are classified like listed ones.
        make(
            &root,
            &["new.log", "new.txt", "sub/new.tmp", "sub/local.tmp2"],
        );
        tree.apply(&changes(
            &root,
            &[
                "new.log",
                "new.txt",
                "sub/new.tmp",
                "sub/local.tmp2",
                ".git/index",
            ],
        ));
        assert_eq!(
            layout(&tree),
            [
                "v sub",
                "  .gitignore",
                "  local.tmp2",
                "  main.rs",
                "  new.tmp",
                ".env",
                ".gitignore",
                ".ignore",
                "keep.txt",
                "new.txt",
            ],
            "in a repo: {in_repo}"
        );

        let options = TreeOptions { show_ignored: true };
        let mut tree = FileTree::open(&root, options).unwrap();
        tree.expand(id(&tree, "sub"));
        tree.expand(id(&tree, "build"));
        let all = [
            "v build (ignored)",
            "  out.o (ignored)",
            "v sub",
            "  .gitignore",
            "  local.tmp (ignored)",
            "  local.tmp2",
            "  main.rs",
            "  new.tmp",
            "  x.log (ignored)",
            ".env",
            ".gitignore",
            ".ignore",
            "app.log (ignored)",
            "keep.txt",
            "new.log (ignored)",
            "new.txt",
            "secret.txt (ignored)",
        ];
        assert_eq!(layout(&tree), all, "in a repo: {in_repo}");
        // Through apply too, and a rescan agrees.
        make(&root, &["build/more.o", "other.log"]);
        tree.apply(&changes(&root, &["build/more.o", "other.log", ".git"]));
        let mut with_new = all.to_vec();
        with_new.insert(1, "  more.o (ignored)");
        with_new.insert(17, "other.log (ignored)");
        assert_eq!(layout(&tree), with_new, "in a repo: {in_repo}");
        tree.apply(&[FsChange::Rescan]);
        assert_eq!(layout(&tree), with_new, "in a repo: {in_repo}");
    }
}

#[test]
fn rows_flatten_expanded_directories() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "a/b/c/deep.txt",
            "a/b/one.txt",
            "a/two.txt",
            "a/z/",
            "m/x.txt",
            "top.txt",
        ],
    );
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    tree.expand(id(&tree, "a"));
    tree.expand(id(&tree, "a/b"));
    tree.expand(id(&tree, "a/b/c"));
    tree.expand(id(&tree, "m"));
    tree.collapse(id(&tree, "m"));
    let all = [
        "v a",
        "  v b",
        "    v c",
        "      deep.txt",
        "    one.txt",
        "  > z",
        "  two.txt",
        "> m",
        "top.txt",
    ];
    assert_eq!(layout(&tree), all);
    assert_eq!(tree.row_count(), all.len());

    for start in 0..=all.len() {
        for end in start..=all.len() {
            let rows: Vec<_> = tree.rows(start..end).map(|row| describe(&row)).collect();
            assert_eq!(rows, all[start..end], "rows({start}..{end})");
        }
    }
    let row = tree.rows(2..3).next().unwrap();
    assert_eq!(row.id, id(&tree, "a/b/c"));
    assert_eq!(
        (row.depth, row.name, row.kind, row.expanded),
        (2, OsStr::new("c"), EntryKind::Dir, true)
    );
    let row = tree.rows(3..4).next().unwrap();
    assert_eq!(
        (row.depth, row.kind, row.expanded),
        (3, EntryKind::File, false)
    );
    assert_eq!(tree.row(id(&tree, "a/b/c/deep.txt")).unwrap().depth, 3);
    assert_eq!(tree.rows(7..100).count(), 2);

    // Collapsing an outer directory hides its expanded descendants, and
    // expanding it again brings them back as they were.
    tree.collapse(id(&tree, "a"));
    assert_eq!(layout(&tree), ["> a", "> m", "top.txt"]);
    tree.expand(id(&tree, "a"));
    assert_eq!(layout(&tree), all);
}

#[test]
fn apply_reconciles_loaded_directories() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "dir/sub/x.txt",
            "dir/sub/inner/y.txt",
            "dir/old.txt",
            "dir/gone.txt",
            "dir/b.txt",
            "dir/swap",
            "z.txt",
        ],
    );
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    let dir = id(&tree, "dir");
    tree.expand(dir);
    tree.expand(id(&tree, "dir/sub"));
    tree.expand(id(&tree, "dir/sub/inner"));
    assert_eq!(
        layout(&tree),
        [
            "v dir",
            "  v sub",
            "    v inner",
            "      y.txt",
            "    x.txt",
            "  b.txt",
            "  gone.txt",
            "  old.txt",
            "  swap",
            "z.txt",
        ]
    );
    let loaded = tree.stats().loaded_entries;
    let version = tree.version();

    make(&root, &["dir/a.txt", "dir/c.txt", "Adir/"]);
    fs::remove_file(root.join("dir/gone.txt")).unwrap();
    fs::rename(root.join("dir/old.txt"), root.join("dir/renamed.txt")).unwrap();
    fs::rename(root.join("dir/sub"), root.join("dir/moved")).unwrap();
    fs::remove_file(root.join("dir/swap")).unwrap();
    make(&root, &["dir/swap/"]);
    let batch = changes(
        &root,
        &[
            "dir/a.txt",
            "dir/c.txt",
            "Adir",
            "dir/gone.txt",
            "dir/old.txt",
            "dir/renamed.txt",
            "dir/sub",
            "dir/moved",
            "dir/swap",
            "dir/a.txt",
        ],
    );
    let mut changed = tree.apply(&batch);
    changed.sort();
    let mut expected = vec![tree.root(), dir];
    expected.sort();
    assert_eq!(changed, expected);
    assert!(tree.version() > version);
    assert_eq!(
        layout(&tree),
        [
            "> Adir",
            "v dir",
            "  > moved",
            "  > swap",
            "  a.txt",
            "  b.txt",
            "  c.txt",
            "  renamed.txt",
            "z.txt",
        ]
    );
    // sub, inner, x.txt, y.txt, gone.txt and old.txt went; Adir, moved,
    // a.txt, c.txt and renamed.txt came; swap changed kind.
    assert_eq!(tree.stats().loaded_entries, loaded - 6 + 5);
    assert!(tree.find("dir/sub/x.txt").is_none());
    assert!(tree.children(id(&tree, "dir/moved")).is_none());

    let version = tree.version();
    let stats = tree.stats();
    assert_eq!(tree.apply(&batch), []);
    assert_eq!(tree.version(), version);
    assert_eq!(tree.stats().loaded_entries, stats.loaded_entries);
    assert_eq!(tree.stats().listings, stats.listings);
}

#[test]
fn changes_outside_loaded_directories_do_no_io() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "unloaded/a/b.txt",
            "build/out.o",
            "src/main.rs",
            ".gitignore",
        ],
    );
    fs::write(root.join(".gitignore"), "build/\n").unwrap();
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    tree.expand(id(&tree, "src"));
    let stats = tree.stats();
    let io = tree.io_calls;
    let version = tree.version();
    let rows = layout(&tree);

    let mut batch = Vec::new();
    for ix in 0..2_500 {
        batch.push(FsChange::Path(root.join(format!("unloaded/f{ix}.txt"))));
        batch.push(FsChange::Path(root.join(format!("unloaded/a/f{ix}.txt"))));
        batch.push(FsChange::Path(root.join(format!("build/f{ix}.o"))));
        batch.push(FsChange::Path(root.join(format!("build/deep/f{ix}.o"))));
    }
    assert_eq!(batch.len(), 10_000);
    assert_eq!(tree.apply(&batch), []);
    assert_eq!(tree.stats(), stats);
    assert_eq!(tree.version(), version);
    assert_eq!(layout(&tree), rows);

    // So are paths outside the root and inside `.git`.
    let elsewhere = changes(root.parent().unwrap(), &["x", "y/z"]);
    let git = changes(&root, &[".git/index", ".git/objects/ab/cd"]);
    assert_eq!(tree.apply(&elsewhere), []);
    assert_eq!(tree.apply(&git), []);
    assert_eq!(tree.stats(), stats);
    // No file system calls at all: no opens, stats or ignore-file reads.
    assert_eq!(tree.io_calls, io);
}

#[test]
fn rescan_relists_each_loaded_directory_once() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "a/b/one.txt",
            "a/b/two.txt",
            "a/x.txt",
            "c/unseen.txt",
            "d/e.txt",
            "top.txt",
        ],
    );
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    tree.expand(id(&tree, "a"));
    tree.expand(id(&tree, "a/b"));
    // Loaded but collapsed still counts.
    tree.expand(id(&tree, "d"));
    tree.collapse(id(&tree, "d"));
    let loaded_dirs = 4; // the root, a, a/b and d
    let before = tree.stats();

    make(&root, &["a/new.txt", "c/new.txt", "d/f.txt", "new/"]);
    fs::remove_file(root.join("a/b/one.txt")).unwrap();
    fs::remove_file(root.join("top.txt")).unwrap();
    let mut changed = tree.apply(&[FsChange::Path(root.join("ignored")), FsChange::Rescan]);
    changed.sort();
    let mut expected = vec![
        tree.root(),
        id(&tree, "a"),
        id(&tree, "a/b"),
        id(&tree, "d"),
    ];
    expected.sort();
    assert_eq!(changed, expected);
    assert_eq!(tree.stats().listings, before.listings + loaded_dirs);
    assert_eq!(
        layout(&tree),
        [
            "v a",
            "  v b",
            "    two.txt",
            "  new.txt",
            "  x.txt",
            "> c",
            "> d",
            "> new",
        ]
    );
    assert!(tree.children(id(&tree, "c")).is_none());
    tree.expand(id(&tree, "d"));
    assert_eq!(tree.stats().listings, before.listings + loaded_dirs);
    assert_eq!(layout(&tree)[6..], ["v d", "  e.txt", "  f.txt", "> new"]);
}

#[test]
fn unreadable_directory_keeps_an_error() {
    let (_dir, root) = temp();
    make(&root, &["locked/secret.txt", "open/file.txt", "z.txt"]);
    let locked = root.join("locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(&locked).is_ok() {
        // Root reads everything.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }

    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    let id_locked = id(&tree, "locked");
    tree.expand(id_locked);
    let row = tree.row(id_locked).unwrap();
    assert_eq!(row.error.unwrap().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(layout(&tree), ["v locked (error)", "> open", "z.txt"]);

    tree.expand(id(&tree, "open"));
    assert_eq!(
        layout(&tree),
        ["v locked (error)", "v open", "  file.txt", "z.txt"]
    );
    // Changes inside it are dropped; it was never listed.
    let stats = tree.stats();
    tree.apply(&changes(&root, &["locked/secret.txt"]));
    assert_eq!(tree.stats(), stats);

    // Once readable again, it lists.
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    tree.collapse(id_locked);
    assert!(tree.listing(id_locked).is_some());
    tree.expand(id_locked);
    assert_eq!(
        layout(&tree),
        ["v locked", "  secret.txt", "v open", "  file.txt", "z.txt"]
    );

    // Listed, then unreadable: a change inside it shows the error.
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(
        tree.apply(&changes(&root, &["locked/secret.txt"])),
        [id_locked]
    );
    assert_eq!(
        layout(&tree),
        ["v locked (error)", "v open", "  file.txt", "z.txt"]
    );
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn apply_holds_a_paths_worth_of_directories_open() {
    let (_dir, root) = temp();
    let dirs: Vec<String> = (0..40).map(|ix| format!("d{ix:02}/")).collect();
    let dirs: Vec<&str> = dirs.iter().map(String::as_str).collect();
    make(&root, &dirs);
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    for dir in &dirs {
        tree.expand(id(&tree, dir.trim_end_matches('/')));
    }
    // A file new in each, reported in no particular order.
    let mut paths: Vec<String> = dirs.iter().map(|dir| format!("{dir}new.txt")).collect();
    paths.reverse();
    paths.swap(3, 30);
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    make(&root, &paths);
    assert_eq!(tree.apply(&changes(&root, &paths)).len(), dirs.len());
    assert_eq!(tree.stats().loaded_entries, 2 * dirs.len());
    // The root and the directory being updated, at most.
    assert_eq!(tree.open_peak, 2);
}

#[test]
fn listings_racing_apply_never_store_older_state() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "a/",
            "b/old.txt",
            "c/",
            "d/",
            "e/",
            "f/kept.txt",
            "g/kept.txt",
            "h/i/kept.txt",
            "h/j/kept.txt",
            "k/kept.txt",
        ],
    );
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();

    // A rescan while a listing runs: the listing may predate anything.
    let a = id(&tree, "a");
    let listed = tree.listing(a).unwrap().run();
    make(&root, &["a/new.txt"]);
    tree.apply(&[FsChange::Rescan]);
    tree.fill(a, listed);
    assert!(tree.children(a).unwrap().is_empty());
    let listed = tree.listing(a).unwrap().run();
    tree.fill(a, listed);
    assert!(tree.listing(a).is_none());
    assert!(tree.find("a/new.txt").is_some());

    // A refresh of a listed directory that `apply` updated meanwhile is
    // discarded; the refresh is still owed until one runs undisturbed.
    let b = id(&tree, "b");
    let listed = tree.listing(b).unwrap().run();
    make(&root, &["b/dropped.txt"]);
    tree.apply(&changes(&root, &["b/dropped.txt"]));
    tree.fill(b, listed);
    let refresh = tree.listing(b).unwrap();
    let listed = refresh.run();
    make(&root, &["b/applied.txt"]);
    tree.apply(&changes(&root, &["b/applied.txt"]));
    tree.fill(b, listed);
    tree.expand(b);
    assert_eq!(
        layout(&tree)[1..5],
        ["v b", "  applied.txt", "  dropped.txt", "  old.txt"]
    );
    assert!(tree.listing(b).is_none());

    // Listings finishing in reverse order: the older result is discarded.
    let c = id(&tree, "c");
    let older = tree.listing(c).unwrap().run();
    make(&root, &["c/one.txt"]);
    tree.apply(&changes(&root, &["c/one.txt"]));
    let newer = tree.listing(c).unwrap().run();
    tree.fill(c, newer);
    assert!(tree.listing(c).is_none());
    let listings = tree.stats().listings;
    tree.fill(c, older);
    assert_eq!(tree.stats().listings, listings + 1);
    assert!(tree.find("c/one.txt").is_some());

    // A change deeper than the unlisted directory still owes it a listing.
    let d = id(&tree, "d");
    let listed = tree.listing(d).unwrap().run();
    make(&root, &["d/sub/deep.txt"]);
    tree.apply(&changes(&root, &["d/sub/deep.txt"]));
    tree.fill(d, listed);
    let listed = tree.listing(d).unwrap().run();
    tree.fill(d, listed);
    assert!(tree.find("d/sub").is_some());

    // Two listings with no `apply` between them, finishing in reverse order:
    // the one that started first is discarded.
    let e = id(&tree, "e");
    let first = tree.listing(e).unwrap();
    let second = tree.listing(e).unwrap();
    let first = first.run();
    make(&root, &["e/late.txt"]);
    let second = second.run();
    tree.fill(e, second);
    tree.fill(e, first);
    assert!(tree.find("e/late.txt").is_some());
    assert!(tree.listing(e).is_none());

    // Two overlapping listings of unlisted `k`: the outer one fails, `k`
    // made unreadable as it runs, and the inner one, run before that, does
    // not. Filled in that order, the success is stored, as `k` isn't
    // listed, but `k` owes a listing, which finds the error.
    let k = id(&tree, "k");
    let probe = root.join("k");
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o000)).unwrap();
    let as_root = fs::read_dir(&probe).is_ok();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();
    if !as_root {
        let inner = std::rc::Rc::new(std::cell::RefCell::new(None));
        let hook = {
            let (inner, mut listing, path) = (inner.clone(), tree.listing(k), root.join("k"));
            move |point| {
                if point == Point::Opened
                    && let Some(listing) = listing.take()
                {
                    *inner.borrow_mut() = Some(listing.run());
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
                }
            }
        };
        let outer = tree.listing(k).unwrap();
        let outer = with_hook(hook, || outer.run());
        assert!(outer.result().is_err());
        tree.fill(k, outer);
        tree.fill(k, inner.take().unwrap());
        assert!(tree.find("k/kept.txt").is_some());
        let again = tree.listing(k).unwrap();
        tree.fill(k, again.run());
        assert!(tree.row(k).unwrap().error.is_some());
        fs::set_permissions(root.join("k"), fs::Permissions::from_mode(0o755)).unwrap();
    }

    // A listing that found its directory gone, filled after the directory
    // came back and `apply` saw it: the result is discarded, not the node.
    let f = id(&tree, "f");
    let listing = tree.listing(f).unwrap();
    fs::rename(root.join("f"), root.join("f.away")).unwrap();
    let gone = listing.run();
    assert_eq!(gone.lost, Some(Lost::Itself));
    fs::rename(root.join("f.away"), root.join("f")).unwrap();
    tree.apply(&changes(&root, &["f", "f.away"]));
    tree.fill(f, gone);
    assert!(tree.find("f").is_some());
    tree.expand(f);
    assert!(tree.find("f/kept.txt").is_some());

    // The same, with the return seen only by a rescan (which lists the
    // parent again): the node stays, and is still offered for listing.
    let g = id(&tree, "g");
    let listing = tree.listing(g).unwrap();
    fs::rename(root.join("g"), root.join("g.away")).unwrap();
    let gone = listing.run();
    assert_eq!(gone.lost, Some(Lost::Itself));
    fs::rename(root.join("g.away"), root.join("g")).unwrap();
    tree.apply(&[FsChange::Rescan]);
    tree.fill(g, gone);
    assert_eq!(tree.find("g"), Some(g));
    let listing = tree.listing(g).unwrap();
    tree.fill(g, listing.run());
    assert!(tree.find("g/kept.txt").is_some());

    // Unreachable because a directory above it was gone for a moment, and
    // filled before the events for that directory: those say only that it
    // is back, so the node mustn't go. It stays, still offered for listing.
    tree.expand(id(&tree, "h"));
    let i = id(&tree, "h/i");
    let listing = tree.listing(i).unwrap();
    fs::rename(root.join("h"), root.join("h.away")).unwrap();
    let gone = listing.run();
    assert_eq!(gone.lost, Some(Lost::Above));
    fs::rename(root.join("h.away"), root.join("h")).unwrap();
    tree.fill(i, gone);
    // Nothing of it was loaded: it keeps the error, until listed again.
    assert!(tree.row(i).unwrap().error.is_some());
    tree.apply(&changes(&root, &["h", "h.away"]));
    assert_eq!(tree.find("h/i"), Some(i));
    tree.expand(i);
    assert!(tree.find("h/i/kept.txt").is_some());
    assert!(tree.row(i).unwrap().error.is_none());
    // The same with another directory in its place for that moment, which
    // the listing finds by its identity.
    let j = id(&tree, "h/j");
    let listing = tree.listing(j).unwrap();
    swap_for_dir(&root.join("h"));
    let gone = listing.run();
    assert_eq!(gone.lost, Some(Lost::Above));
    fs::remove_dir(root.join("h")).unwrap();
    swap_back_dir(&root.join("h"));
    tree.fill(j, gone);
    tree.apply(&changes(&root, &["h", "h.aside"]));
    assert_eq!(tree.find("h/j"), Some(j));
}

/// Lists `dir` with a change dropped while its listing ran, so it is listed
/// but owes a refresh.
fn list_owing(tree: &mut FileTree, root: &Path, dir: &str) -> NodeId {
    let node = id(tree, dir);
    let listed = tree.listing(node).unwrap().run();
    let late = format!("{dir}/late.txt");
    make(root, &[&late]);
    tree.apply(&changes(root, &[&late]));
    tree.fill(node, listed);
    assert!(tree.listing(node).is_some());
    node
}

#[test]
fn older_listings_never_undo_newer_news_below() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "a/b/old.txt",
            "p/c/",
            "q/d/old.txt",
            "r/s/old.txt",
            "t/u/old.txt",
        ],
    );
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();

    // A refresh of `a` run while `a/b` is away, filled after `b` is back
    // and a change in it was applied: `b` and what was applied stay, and
    // `a` still owes the refresh.
    let a = list_owing(&mut tree, &root, "a");
    let b = id(&tree, "a/b");
    tree.expand(b);
    let refresh = tree.listing(a).unwrap();
    fs::rename(root.join("a/b"), aside(&root.join("a/b"))).unwrap();
    let stale = refresh.run();
    swap_back_dir(&root.join("a/b"));
    make(&root, &["a/b/new.txt"]);
    tree.apply(&changes(&root, &["a/b/new.txt"]));
    tree.fill(a, stale);
    assert_eq!(tree.find("a/b"), Some(b));
    assert!(tree.find("a/b/new.txt").is_some());
    let refresh = tree.listing(a).unwrap();
    tree.fill(a, refresh.run());
    assert!(tree.find("a/b/new.txt").is_some() && tree.find("a/late.txt").is_some());
    assert!(tree.listing(a).is_none());

    // A refresh of `p` that saw `p/c`, filled after a listing of `c` found
    // it gone: `c` isn't brought back.
    let p = list_owing(&mut tree, &root, "p");
    let c = id(&tree, "p/c");
    let stale = tree.listing(p).unwrap().run();
    fs::remove_dir(root.join("p/c")).unwrap();
    let gone = tree.listing(c).unwrap().run();
    tree.fill(c, gone);
    assert!(tree.find("p/c").is_none());
    tree.fill(p, stale);
    assert!(tree.find("p/c").is_none());
    assert!(tree.listing(p).is_some());

    // A refresh of `q` run while `q/d` is away, filled after a listing of
    // `d` that ran once it was back: `d` and its entries stay.
    let q = list_owing(&mut tree, &root, "q");
    let d = id(&tree, "q/d");
    fs::rename(root.join("q/d"), aside(&root.join("q/d"))).unwrap();
    let stale = tree.listing(q).unwrap().run();
    swap_back_dir(&root.join("q/d"));
    let listed = tree.listing(d).unwrap().run();
    tree.fill(d, listed);
    tree.fill(q, stale);
    assert_eq!(tree.find("q/d"), Some(d));
    assert!(tree.find("q/d/old.txt").is_some());

    // The same with `r/s` not loaded and only a change in it reported,
    // which is dropped: that too is news that `s` is there. `s` stays, and
    // still owes a listing, as `r` does.
    let r = list_owing(&mut tree, &root, "r");
    let s = id(&tree, "r/s");
    fs::rename(root.join("r/s"), aside(&root.join("r/s"))).unwrap();
    let stale = tree.listing(r).unwrap().run();
    swap_back_dir(&root.join("r/s"));
    make(&root, &["r/s/new.txt"]);
    tree.apply(&changes(&root, &["r/s/new.txt"]));
    tree.fill(r, stale);
    assert_eq!(tree.find("r/s"), Some(s));
    assert!(tree.listing(r).is_some());
    let listing = tree.listing(s).unwrap();
    tree.fill(s, listing.run());
    assert!(tree.find("r/s/new.txt").is_some());

    // And with the first listing of `t/u`, run before such a change, filled
    // before the stale result: `u` keeps what it loaded, and is offered
    // again for what it missed.
    let t = list_owing(&mut tree, &root, "t");
    let u = id(&tree, "t/u");
    let early = tree.listing(u).unwrap().run();
    fs::rename(root.join("t/u"), aside(&root.join("t/u"))).unwrap();
    let stale = tree.listing(t).unwrap().run();
    swap_back_dir(&root.join("t/u"));
    make(&root, &["t/u/new.txt"]);
    tree.apply(&changes(&root, &["t/u/new.txt"]));
    tree.fill(u, early);
    tree.fill(t, stale);
    assert_eq!(tree.find("t/u"), Some(u));
    assert!(tree.find("t/u/old.txt").is_some());
    let listing = tree.listing(u).unwrap();
    tree.fill(u, listing.run());
    assert!(tree.find("t/u/new.txt").is_some());
}

#[test]
fn listings_started_during_apply_are_older_than_what_it_sees_next() {
    use std::{cell::RefCell, rc::Rc};

    let (_dir, root) = temp();
    make(&root, &["a.txt", "d/one.txt", "u/", "v/", "w/x.tmp"]);
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    // A hook that runs `listing` at the `nth` `point` reached, then writes
    // `text` to `file`.
    let run_at = |point: Point, nth: usize, listing: Listing, file: PathBuf, text: &'static str| {
        let result = Rc::new(RefCell::new(None));
        let hook = {
            let result = result.clone();
            let mut listing = Some(listing);
            let mut reached = 0;
            move |at| {
                if at != point {
                    return;
                }
                reached += 1;
                if reached == nth
                    && let Some(listing) = listing.take()
                {
                    *result.borrow_mut() = Some(listing.run());
                    fs::write(&file, text).unwrap();
                }
            }
        };
        (hook, result)
    };

    // A refresh of listed `d` running after `apply` looked up `a.txt`, and
    // `d/two.txt` made then, which `apply` looks up next: the refresh is
    // older than that, and doesn't take `two.txt` away.
    let d = list_owing(&mut tree, &root, "d");
    let refresh = tree.listing(d).unwrap();
    let (hook, early) = run_at(Point::Stat, 1, refresh, root.join("d/two.txt"), "");
    with_hook(hook, || {
        tree.apply(&changes(&root, &["a.txt", "d/two.txt"]))
    });
    tree.fill(d, early.take().unwrap());
    assert!(tree.find("d/two.txt").is_some());
    assert!(tree.listing(d).is_some());

    // A first listing of `u` running then, and `u/new.txt` made then, whose
    // path `apply` drops next as `u` isn't listed: the listing is stored but
    // offered again.
    let u = id(&tree, "u");
    let first = tree.listing(u).unwrap();
    let (hook, early) = run_at(Point::Stat, 1, first, root.join("u/new.txt"), "");
    with_hook(hook, || {
        tree.apply(&changes(&root, &["a.txt", "u/new.txt"]))
    });
    tree.fill(u, early.take().unwrap());
    assert!(tree.children(u).unwrap().is_empty());
    let again = tree.listing(u).unwrap();
    tree.fill(u, again.run());
    assert!(tree.find("u/new.txt").is_some());

    // The same during a rescan, the listing of `v` running once the rescan
    // has opened the root: the rescan marks `v` after it.
    let v = id(&tree, "v");
    let first = tree.listing(v).unwrap();
    let (hook, early) = run_at(Point::Opened, 1, first, root.join("v/new.txt"), "");
    with_hook(hook, || tree.apply(&[FsChange::Rescan]));
    tree.fill(v, early.take().unwrap());
    assert!(tree.children(v).unwrap().is_empty());
    let again = tree.listing(v).unwrap();
    tree.fill(v, again.run());
    assert!(tree.find("v/new.txt").is_some());

    // A refresh of listed `y` running as a rescan's own listing of `y` has
    // opened it (in a tree where only the root comes before), and
    // `y/two.txt` made then: the rescan's listing reads it, and the refresh,
    // which overlapped it, is discarded, with `y` owing another.
    let (_other, other) = temp();
    make(&other, &["y/one.txt"]);
    let mut small = FileTree::open(&other, TreeOptions::default()).unwrap();
    let y = list_owing(&mut small, &other, "y");
    let refresh = small.listing(y).unwrap();
    let (hook, early) = run_at(Point::Opened, 2, refresh, other.join("y/two.txt"), "");
    with_hook(hook, || small.apply(&[FsChange::Rescan]));
    small.fill(y, early.take().unwrap());
    assert!(small.find("y/two.txt").is_some());
    assert!(small.listing(y).is_some());

    // A refresh of listed `w` running once `apply` has looked `w/x.tmp` up,
    // and a `.gitignore` made then that ignores it, which `apply` reads
    // next to classify it: the refresh doesn't bring `x.tmp` back.
    let w = list_owing(&mut tree, &root, "w");
    let refresh = tree.listing(w).unwrap();
    let rules = root.join("w/.gitignore");
    let (hook, early) = run_at(Point::Stat, 1, refresh, rules, "*.tmp\n");
    with_hook(hook, || tree.apply(&changes(&root, &["w/x.tmp"])));
    assert!(tree.find("w/x.tmp").is_none());
    tree.fill(w, early.take().unwrap());
    assert!(tree.find("w/x.tmp").is_none());
    assert!(tree.listing(w).is_some());
}

#[test]
fn directories_gone_for_a_moment_lose_nothing() {
    let (_dir, root) = temp();
    make(&root, &["a.txt", "p/d/old.txt"]);
    let open_all = |tree: &mut FileTree| {
        tree.expand(id(tree, "p"));
        tree.expand(id(tree, "p/d"));
    };

    // `p` moved away while a rescan lists `p/d`, and back after, with a
    // change in `p/d` meanwhile: `p/d` keeps its entries rather than going
    // for good, and `p`'s own event lists both again.
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    open_all(&mut tree);
    let away = {
        let p = root.join("p");
        let mut opened = 0;
        move |point| {
            // The root, then `p`, then `p/d`.
            if point == Point::Opened {
                opened += 1;
                if opened == 3 {
                    fs::rename(&p, aside(&p)).unwrap();
                }
            }
        }
    };
    with_hook(away, || tree.apply(&[FsChange::Rescan]));
    assert_eq!(layout(&tree), ["v p", "  v d", "    old.txt", "a.txt"]);
    make(&root, &["p.aside/d/late.txt"]);
    swap_back_dir(&root.join("p"));
    tree.apply(&changes(&root, &["p", "p.aside", "p.aside/d/late.txt"]));
    assert_eq!(
        layout(&tree),
        ["v p", "  v d", "    late.txt", "    old.txt", "a.txt"]
    );
    assert!(tree.listing(id(&tree, "p/d")).is_none());
    fs::remove_file(root.join("p/d/late.txt")).unwrap();

    // `apply` finding a directory (or one above it) gone when it opens it,
    // and back when it looks its name up: the changes in it that it
    // couldn't look up come from listing it again.
    for gone in ["p/d", "p"] {
        let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
        open_all(&mut tree);
        let new = format!("new_{}.txt", gone.len());
        make(&root, &[&format!("p/d/{new}")]);
        let away_and_back = {
            let path = root.join(gone);
            let mut moved = false;
            move |point| match point {
                // Looking up `a.txt`, which comes first.
                Point::Stat if !moved => {
                    moved = true;
                    fs::rename(&path, aside(&path)).unwrap();
                }
                Point::Unreached => swap_back_dir(&path),
                _ => {}
            }
        };
        let batch = changes(&root, &["a.txt", &format!("p/d/{new}")]);
        with_hook(away_and_back, || tree.apply(&batch));
        assert!(tree.find(format!("p/d/{new}")).is_some(), "{gone}");
    }
}

#[test]
fn directories_back_from_elsewhere_are_listed_again() {
    // Moved away, changed, and moved back between two looks: the changes
    // are reported under the other name, and the directory is back as
    // itself. Its own event lists it again, with everything loaded below.
    let (_dir, base) = temp();
    let root = base.join("up/proj");
    make(&root, &["a/f.txt", "p/d/old.txt", "p/x/"]);
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    for dir in ["a", "p", "p/d"] {
        tree.expand(id(&tree, dir));
    }
    // A listing of `p/x` that runs before the moves and is filled after.
    let x = id(&tree, "p/x");
    let early = tree.listing(x).unwrap().run();
    let away_and_back = |name: &str, change: &dyn Fn(&Path)| {
        let path = root.join(name);
        fs::rename(&path, aside(&path)).unwrap();
        change(&aside(&path));
        swap_back_dir(&path);
    };
    away_and_back("a", &|a| {
        make(a, &["new.txt"]);
        fs::remove_file(a.join("f.txt")).unwrap();
    });
    away_and_back("p", &|p| make(p, &["d/new.txt", "x/new.txt"]));
    let batch = changes(
        &root,
        &[
            "a.aside/new.txt",
            "a.aside/f.txt",
            "a.aside",
            "a",
            "p.aside/d/new.txt",
            "p.aside/x/new.txt",
            "p.aside",
            "p",
        ],
    );
    tree.apply(&batch);
    assert_eq!(
        layout(&tree),
        [
            "v a",
            "  new.txt",
            "v p",
            "  v d",
            "    new.txt",
            "    old.txt",
            "  > x"
        ]
    );
    // The early listing of `p/x` is stored, but offered again.
    tree.fill(x, early);
    assert!(tree.children(x).unwrap().is_empty());
    tree.expand(x);
    assert!(tree.find("p/x/new.txt").is_some());

    // The root too, reported only by its own path.
    fs::rename(&root, aside(&root)).unwrap();
    make(&aside(&root), &["p/d/newer.txt"]);
    swap_back_dir(&root);
    tree.apply(&[FsChange::Path(root.clone())]);
    assert!(tree.find("p/d/newer.txt").is_some());

    // A directory above the root, for which nothing is reported: the tree
    // stays as it was while the root can't be reached, and once `apply`
    // reaches it again, everything loaded is listed again.
    let up = base.join("up");
    fs::rename(&up, aside(&up)).unwrap();
    assert_eq!(tree.apply(&changes(&root, &["a/new.txt"])), []);
    assert!(tree.find("p/d/newer.txt").is_some());
    make(&aside(&up), &["proj/p/d/newest.txt"]);
    swap_back_dir(&up);
    tree.apply(&changes(&root, &["a/new.txt"]));
    assert!(tree.find("p/d/newest.txt").is_some());
}

#[test]
fn a_root_cut_off_or_replaced_above_it_is_found_again() {
    // Lists what is owed until nothing is, as a caller would.
    fn settle(tree: &mut FileTree) {
        for _ in 0..10 {
            let owed: Vec<_> = (std::iter::once(tree.root()))
                .chain(tree.rows(0..tree.row_count()).map(|row| row.id))
                .filter(|&id| tree.children(id).is_some() || id == tree.root())
                .filter_map(|id| Some((id, tree.listing(id)?)))
                .collect();
            if owed.is_empty() {
                return;
            }
            for (id, listing) in owed {
                tree.fill(id, listing.run());
            }
        }
    }
    let (_dir, base) = temp();
    let root = base.join("up/proj");
    let up = base.join("up");
    make(&root, &["a/k.txt", "a/u/", "b/x.txt", "v/", "w/"]);
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    tree.expand(id(&tree, "b"));

    // A result that saw the root cut off is news even when it is discarded:
    // here a refresh of `a`, overtaken by a change dropped under `a/u`.
    // Changes made meanwhile, unreported, are found once the root is back.
    let a = list_owing(&mut tree, &root, "a");
    let refresh = tree.listing(a).unwrap();
    make(&root, &["a/u/f.txt"]);
    fs::rename(&up, aside(&up)).unwrap();
    make(&aside(&up), &["proj/b/new.txt"]);
    let cut = refresh.run();
    assert_eq!(cut.lost, Some(Lost::Root));
    tree.apply(&changes(&root, &["a/u/f.txt"]));
    tree.fill(a, cut);
    swap_back_dir(&up);
    assert!(tree.listing(tree.root()).is_some());
    settle(&mut tree);
    assert!(tree.find("b/new.txt").is_some());

    // A listing of the root that ran before the root was seen cut off again,
    // filled after the listing that saw it, doesn't count as finding it: the
    // root is listed once more.
    let w = id(&tree, "w");
    fs::rename(&up, aside(&up)).unwrap();
    let cut = tree.listing(w).unwrap().run();
    tree.fill(w, cut);
    swap_back_dir(&up);
    let (first, second) = (
        tree.listing(tree.root()).unwrap(),
        tree.listing(tree.root()).unwrap(),
    );
    let early = first.run();
    fs::rename(&up, aside(&up)).unwrap();
    make(&aside(&up), &["proj/new.txt"]);
    let cut = second.run();
    assert_eq!(cut.lost, Some(Lost::Root));
    swap_back_dir(&up);
    tree.fill(tree.root(), cut);
    tree.fill(tree.root(), early);
    assert!(tree.listing(tree.root()).is_some());
    settle(&mut tree);
    assert!(tree.find("new.txt").is_some());

    // A directory not listed yet, whose listing read it before the root was
    // cut off, filled after the root was found again: stored, but offered
    // again, as it may have changed meanwhile.
    let v = id(&tree, "v");
    let before = tree.listing(v).unwrap().run();
    fs::rename(&up, aside(&up)).unwrap();
    make(&aside(&up), &["proj/v/new.txt"]);
    let cut = tree.listing(w).unwrap().run();
    tree.fill(w, cut);
    swap_back_dir(&up);
    let found = tree.listing(tree.root()).unwrap().run();
    tree.fill(tree.root(), found);
    tree.fill(v, before);
    let again = tree.listing(v).unwrap();
    tree.fill(v, again.run());
    assert!(tree.find("v/new.txt").is_some());

    // The root replaced above it, by a directory holding another project:
    // a listing below finds the root isn't the one it was, and the root is
    // offered, to be listed as the new one.
    make(&base, &["new/proj/other.txt"]);
    fs::rename(&up, base.join("up.old")).unwrap();
    fs::rename(base.join("new"), &up).unwrap();
    let u = id(&tree, "a/u");
    let cut = tree.listing(u).unwrap().run();
    assert_eq!(cut.lost, Some(Lost::Root));
    tree.fill(u, cut);
    assert!(tree.listing(tree.root()).is_some());
    settle(&mut tree);
    assert_eq!(layout(&tree), ["other.txt"]);

    // The root's parent deleted: FSEvents reports the root's own path with
    // the rest, and the tree drops everything rather than keep a ghost.
    make(&root, &["d/y.txt"]);
    tree.apply(&changes(&root, &["d"]));
    tree.expand(id(&tree, "d"));
    fs::remove_dir_all(&up).unwrap();
    tree.apply(&changes(&root, &["d/y.txt", "d", "other.txt", ""]));
    assert_eq!(tree.row_count(), 0);
    let gone = tree.listing(tree.root()).unwrap().run();
    assert!(gone.result().is_err());
}

#[test]
fn apply_replaces_directories_with_new_identity() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "left/l.txt",
            "right/r.txt",
            "same/inner/i.txt",
            "re/x.txt",
            "base/",
        ],
    );
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    for dir in ["left", "right", "same", "same/inner", "re", "base"] {
        tree.expand(id(&tree, dir));
    }

    // Two directories swapped through a temporary name.
    fs::rename(root.join("left"), root.join("tmp")).unwrap();
    fs::rename(root.join("right"), root.join("left")).unwrap();
    fs::rename(root.join("tmp"), root.join("right")).unwrap();
    // Deleted and made again with other contents.
    fs::remove_dir_all(root.join("same")).unwrap();
    make(&root, &["same/other.txt"]);
    let loaded = tree.stats().loaded_entries;
    assert_eq!(
        tree.apply(&changes(&root, &["left", "tmp", "right", "same"])),
        [tree.root()]
    );
    // l.txt, r.txt, inner and i.txt went with the old nodes.
    assert_eq!(tree.stats().loaded_entries, loaded - 4);
    assert_eq!(
        layout(&tree),
        ["v base", "> left", "v re", "  x.txt", "> right", "> same"]
    );
    tree.expand(id(&tree, "left"));
    assert!(tree.find("left/r.txt").is_some());
    assert_eq!(tree.apply(&changes(&root, &["left", "right", "same"])), []);

    // A rescan notices a replacement too.
    fs::remove_dir_all(root.join("re")).unwrap();
    make(&root, &["re/y.txt"]);
    tree.apply(&[FsChange::Rescan]);
    assert!(tree.children(id(&tree, "re")).is_none());
    assert!(layout(&tree).contains(&"> re".to_string()));

    // A path below a name the tree doesn't have is dropped without IO; the
    // new directory's own path brings it in.
    make(&root, &["base/fresh/f.txt"]);
    let io = tree.io_calls;
    assert_eq!(tree.apply(&changes(&root, &["base/fresh/f.txt"])), []);
    assert_eq!(tree.io_calls, io);
    assert_eq!(
        tree.apply(&changes(&root, &["base/fresh"])),
        [id(&tree, "base")]
    );
    assert_eq!(layout(&tree)[..2], ["v base", "  > fresh"]);
}

#[test]
fn symlinks_put_in_place_of_directories_are_not_followed() {
    let (_outside_dir, outside) = temp();
    make(&outside, &["secret.txt", "deeper/x.txt"]);
    let (_dir, root) = temp();
    make(
        &root,
        &["unlisted/u.txt", "a/b/x.txt", "p/q/y.txt", "m/n/z.txt"],
    );
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    for dir in ["a", "a/b", "p", "p/q", "m", "m/n"] {
        tree.expand(id(&tree, dir));
    }
    let swap_for_link = |path: &str| {
        fs::rename(root.join(path), root.join(format!("{path}.gone"))).unwrap();
        symlink(&outside, root.join(path)).unwrap();
    };

    // A directory replaced by a link before its listing runs: the listing
    // isn't stored, and the node goes until the path's event says what's
    // there.
    let unlisted = id(&tree, "unlisted");
    let listing = tree.listing(unlisted).unwrap();
    swap_for_link("unlisted");
    tree.fill(unlisted, listing.run());
    assert!(tree.find("unlisted").is_none());
    tree.apply(&changes(&root, &["unlisted", "unlisted.gone"]));
    assert!(layout(&tree).contains(&"@ unlisted".to_string()));
    tree.expand(id(&tree, "unlisted"));
    assert!(tree.find("unlisted/secret.txt").is_none());

    // A listed directory, or one above it, replaced by a link, with only a
    // path below it reported: nothing outside is stat'ed, and the replaced
    // directory becomes the link.
    swap_for_link("a/b");
    swap_for_link("p");
    tree.apply(&changes(
        &root,
        &["a/b/secret.txt", "a/b/deeper", "p/q/y.txt"],
    ));
    for leaked in ["a/b/secret.txt", "a/b/deeper", "p/q", "p/secret.txt"] {
        assert!(tree.find(leaked).is_none(), "{leaked}");
    }

    // A path below a name the tree lacks, under a replaced directory, is
    // dropped without any IO, so nothing is read through the link.
    swap_for_link("m");
    let io = tree.io_calls;
    assert_eq!(tree.apply(&changes(&root, &["m/n/deeper/x.txt"])), []);
    assert_eq!(tree.io_calls, io);
    tree.apply(&changes(&root, &["m/n/z.txt"]));
    assert_eq!(
        layout(&tree),
        [
            "v a",
            "  @ b",
            "> unlisted.gone",
            "@ m",
            "@ p",
            "@ unlisted"
        ]
    );
}

#[test]
fn the_root_recovers_from_failure_and_replacement() {
    let (_dir, parent) = temp();
    let root = parent.join("proj");
    make(&root, &["one.txt", "sub/two.txt"]);
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    tree.expand(id(&tree, "sub"));

    // Replaced by another directory: a path event re-lists it.
    fs::rename(&root, parent.join("proj.old")).unwrap();
    make(&root, &["new.txt", "sub/three.txt"]);
    tree.apply(&changes(&root, &["new.txt"]));
    assert_eq!(layout(&tree), ["> sub", "new.txt"]);
    // And through a rescan.
    fs::rename(&root, parent.join("proj.older")).unwrap();
    make(&root, &["newer.txt"]);
    tree.apply(&[FsChange::Rescan]);
    assert_eq!(layout(&tree), ["newer.txt"]);

    // With only the root's own path reported: replaced, deleted, restored.
    fs::rename(&root, parent.join("proj.newer")).unwrap();
    make(&root, &["only.txt"]);
    assert_eq!(tree.apply(&changes(&root, &[""])), [tree.root()]);
    assert_eq!(layout(&tree), ["only.txt"]);
    fs::rename(&root, parent.join("proj.only")).unwrap();
    assert_eq!(tree.apply(&changes(&root, &[""])), [tree.root()]);
    assert_eq!(tree.row_count(), 0);
    assert!(tree.listing(tree.root()).is_some());
    fs::rename(parent.join("proj.newer"), &root).unwrap();
    assert_eq!(tree.apply(&[FsChange::Path(root.clone())]), [tree.root()]);
    assert_eq!(layout(&tree), ["newer.txt"]);

    // A rescan that can't read the root, then events once it can again.
    fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(&root).is_ok() {
        // Root reads everything.
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    tree.apply(&[FsChange::Rescan]);
    assert_eq!(tree.row_count(), 0);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(tree.apply(&changes(&root, &["newer.txt", "x/y"])), []);
    let listing = tree.listing(tree.root()).unwrap();
    tree.fill(tree.root(), listing.run());
    assert_eq!(layout(&tree), ["newer.txt"]);
    tree.apply(&[FsChange::Rescan]);
    assert_eq!(layout(&tree), ["newer.txt"]);
}

/// macOS only: its `/Users` is a firmlink.
#[cfg(target_os = "macos")]
#[test]
fn firmlinked_directories_keep_their_identity() {
    // `/Users`'s `readdir` inode is the covered directory's, not what `stat`
    // reports, as at a mount point. Skipped where that isn't so.
    let users = Path::new("/Users");
    let covered = fs::read_dir("/")
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.file_name() == "Users")
        .map(|entry| std::os::unix::fs::DirEntryExt::ino(&entry));
    let Ok(metadata) = fs::symlink_metadata(users) else {
        return;
    };
    if covered.is_none_or(|ino| ino == std::os::unix::fs::MetadataExt::ino(&metadata)) {
        return;
    }
    // The tree's identities agree with `stat`.
    let stat = listing::stat_at(&fs::File::open("/").unwrap(), OsStr::new("Users")).unwrap();
    use std::os::unix::fs::MetadataExt;
    assert_eq!(stat.id, (metadata.dev(), metadata.ino()));

    let mut tree = FileTree::open("/", TreeOptions::default()).unwrap();
    let users_id = id(&tree, "Users");
    tree.expand(users_id);
    let row = tree.row(users_id).unwrap();
    assert!(row.expanded && row.error.is_none());
    assert!(!tree.children(users_id).unwrap().is_empty());
    assert_eq!(tree.apply(&changes(Path::new("/"), &["Users"])), []);
    tree.apply(&[FsChange::Rescan]);
    assert_eq!(tree.find("Users"), Some(users_id));
    assert!(tree.children(users_id).is_some());
}

#[test]
fn ignore_rules_follow_replaced_directories() {
    let (_dir, root) = temp();
    make(&root, &["pkg/.gitignore", "pkg/keep.txt"]);
    fs::write(root.join("pkg/.gitignore"), "*.tmp\n").unwrap();
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    tree.expand(id(&tree, "pkg"));
    make(&root, &["pkg/a.tmp", "pkg/a.out"]);
    tree.apply(&changes(&root, &["pkg/a.tmp", "pkg/a.out"]));
    assert_eq!(
        layout(&tree),
        ["v pkg", "  .gitignore", "  a.out", "  keep.txt"]
    );

    // The same path, a new directory with other rules.
    let recreate = |rule: &str| {
        fs::remove_dir_all(root.join("pkg")).unwrap();
        make(&root, &["pkg/.gitignore"]);
        fs::write(root.join("pkg/.gitignore"), rule).unwrap();
    };
    recreate("*.out\n");
    tree.apply(&changes(&root, &["pkg"]));
    tree.expand(id(&tree, "pkg"));
    make(&root, &["pkg/b.tmp", "pkg/b.out"]);
    tree.apply(&changes(&root, &["pkg/b.tmp", "pkg/b.out"]));
    assert_eq!(layout(&tree), ["v pkg", "  .gitignore", "  b.tmp"]);

    // And after a rescan.
    recreate("*.tmp\n");
    tree.apply(&[FsChange::Rescan]);
    tree.expand(id(&tree, "pkg"));
    make(&root, &["pkg/c.tmp", "pkg/c.out"]);
    tree.apply(&changes(&root, &["pkg/c.tmp", "pkg/c.out"]));
    assert_eq!(layout(&tree), ["v pkg", "  .gitignore", "  c.out"]);

    // The root replaced by a directory holding only files, with other
    // rules, after its rules were read for an event, and found through a
    // child's event.
    let (_dir, parent) = temp();
    let root = parent.join("proj");
    make(&root, &[".gitignore", "a.txt"]);
    fs::write(root.join(".gitignore"), "*.tmp\n").unwrap();
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    make(&root, &["b.tmp", "b.out"]);
    tree.apply(&changes(&root, &["b.tmp", "b.out"]));
    assert_eq!(layout(&tree), [".gitignore", "a.txt", "b.out"]);
    fs::rename(&root, parent.join("proj.old")).unwrap();
    make(&root, &[".gitignore", "c.txt"]);
    fs::write(root.join(".gitignore"), "*.out\n").unwrap();
    tree.apply(&changes(&root, &["c.txt"]));
    assert_eq!(layout(&tree), [".gitignore", "c.txt"]);
    make(&root, &["d.tmp", "d.out"]);
    tree.apply(&changes(&root, &["d.tmp", "d.out"]));
    assert_eq!(layout(&tree), [".gitignore", "c.txt", "d.tmp"]);
}

/// macOS only: elsewhere `apply` can't learn an entry's on-disk spelling.
#[cfg(target_os = "macos")]
#[test]
fn case_only_renames_reconcile() {
    let (_dir, root) = temp();
    make(&root, &["Probe"]);
    if !root.join("probe").exists() {
        // A case-sensitive file system: the names are different files.
        return;
    }
    fs::remove_file(root.join("Probe")).unwrap();
    make(&root, &["Foo", "Baz", "bar", ".GIT/HEAD", ".gitignore"]);
    fs::write(root.join(".gitignore"), "foo\nbaz\n").unwrap();
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    assert_eq!(layout(&tree), ["> .GIT", ".gitignore", "bar", "Baz", "Foo"]);

    // Into an ignored name, into `.git`, and plainly.
    fs::rename(root.join("Foo"), root.join("foo")).unwrap();
    fs::rename(root.join(".GIT"), root.join(".git")).unwrap();
    fs::rename(root.join("bar"), root.join("Bar")).unwrap();
    let batch = changes(&root, &["Foo", "foo", ".GIT", ".git", "bar", "Bar"]);
    assert_eq!(tree.apply(&batch), [tree.root()]);
    assert_eq!(layout(&tree), [".gitignore", "Bar", "Baz"]);
    let version = tree.version();
    assert_eq!(tree.apply(&batch), []);
    assert_eq!(tree.version(), version);

    // The two names of a rename in separate batches, either way round, and
    // a spelling that was never on disk.
    fs::rename(root.join("Baz"), root.join("baz")).unwrap();
    assert_eq!(tree.apply(&changes(&root, &["Baz"])), [tree.root()]);
    assert_eq!(layout(&tree), [".gitignore", "Bar"]);
    for path in ["baz", "Baz", "BAZ", "FOO", "bar"] {
        assert_eq!(tree.apply(&changes(&root, &[path])), [], "{path}");
    }
    assert_eq!(tree.version(), version + 1);
    fs::rename(root.join("Bar"), root.join("BAR")).unwrap();
    assert_eq!(tree.apply(&changes(&root, &["BAR"])), [tree.root()]);
    assert_eq!(tree.apply(&changes(&root, &["Bar"])), []);
    assert_eq!(layout(&tree), [".gitignore", "BAR"]);

    // Directories renamed into an ignored name and into `.git`, with only a
    // path below them reported.
    fs::remove_dir_all(root.join(".git")).unwrap();
    make(&root, &["Dir/child.txt", ".Git/HEAD"]);
    fs::write(root.join(".gitignore"), "foo\nbaz\ndir/\n").unwrap();
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    tree.expand(id(&tree, "Dir"));
    assert_eq!(
        layout(&tree),
        ["> .Git", "v Dir", "  child.txt", ".gitignore", "BAR"]
    );
    fs::rename(root.join("Dir"), root.join("dir")).unwrap();
    fs::rename(root.join(".Git"), root.join(".git")).unwrap();
    let batch = changes(&root, &["dir/child.txt", ".git/HEAD"]);
    assert_eq!(tree.apply(&batch), [tree.root()]);
    assert_eq!(layout(&tree), [".gitignore", "BAR"]);
    assert_eq!(tree.apply(&batch), []);

    // Loaded directories renamed so, with only a path below their old
    // spelling reported: that spelling still opens them, but their entries
    // in the root have the new one.
    fs::remove_dir_all(root.join(".git")).unwrap();
    fs::remove_dir_all(root.join("dir")).unwrap();
    make(&root, &["Pkg/child.txt", ".GIT/HEAD"]);
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    tree.expand(id(&tree, "Pkg"));
    tree.expand(id(&tree, ".GIT"));
    assert_eq!(
        layout(&tree),
        [
            "v .GIT",
            "  HEAD",
            "v Pkg",
            "  child.txt",
            ".gitignore",
            "BAR"
        ]
    );
    fs::rename(root.join("Pkg"), root.join("pkg")).unwrap();
    fs::rename(root.join(".GIT"), root.join(".git")).unwrap();
    let batch = changes(&root, &["Pkg/child.txt", ".GIT/HEAD"]);
    assert_eq!(tree.apply(&batch), [tree.root()]);
    assert_eq!(layout(&tree), ["> pkg", ".gitignore", "BAR"]);
    assert_eq!(tree.apply(&batch), []);

    // Renamed so and back, with a change in between, reported under the
    // other spelling: the directory's own path lists it again.
    tree.expand(id(&tree, "pkg"));
    fs::rename(root.join("pkg"), root.join("PKG")).unwrap();
    make(&root, &["PKG/new.txt"]);
    fs::rename(root.join("PKG"), root.join("pkg")).unwrap();
    tree.apply(&changes(&root, &["pkg", "PKG", "PKG/new.txt"]));
    assert_eq!(
        layout(&tree),
        ["v pkg", "  child.txt", "  new.txt", ".gitignore", "BAR"]
    );
}

/// A listing's entries, `(ignored)` marked.
fn entries(listed: &Listed) -> Vec<String> {
    let entries = listed.result().unwrap().iter();
    entries
        .map(|entry| match entry.ignored {
            true => format!("{} (ignored)", entry.name.to_string_lossy()),
            false => entry.name.to_string_lossy().into_owned(),
        })
        .collect()
}

/// A hook that swaps directory `path` for a link to `target` once a listing
/// has opened and checked its directory, and back before it checks the path
/// again.
fn swap_while_reading(path: &Path, target: &Path) -> impl FnMut(Point) + 'static {
    let (path, target) = (path.to_path_buf(), target.to_path_buf());
    move |point| match point {
        Point::Opened => swap_for_link(&path, &target),
        Point::Read => swap_back(&path),
        Point::Stat | Point::Unreached => {}
    }
}

#[test]
fn listings_read_the_directory_they_checked() {
    // What a path swapped for a symlink could show if anything were read by
    // path: other entries, and other rules.
    let (_outside_dir, outside) = temp();
    make(
        &outside,
        &[
            "d/.gitignore",
            "d/secret.txt",
            "d/kept.log",
            "proj/secret.txt",
        ],
    );
    fs::write(outside.join("d/.gitignore"), "keep.txt\n!*.log\n").unwrap();
    for show_ignored in [false, true] {
        let (_dir, base) = temp();
        let root = base.join("p/proj");
        make(
            &root,
            &[
                ".gitignore",
                "a/d/keep.txt",
                "a/d/x.log",
                "a/e/keep.txt",
                "a/f/keep.txt",
                "a/g/keep.txt",
            ],
        );
        fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        let mut tree = FileTree::open(&root, TreeOptions { show_ignored }).unwrap();
        tree.expand(id(&tree, "a"));
        let mut kept = vec!["keep.txt"];
        if show_ignored {
            kept.push("x.log (ignored)");
        }

        // A directory above it swapped for a link, after the listing checked
        // its directory and until it checks the path again: what it reads
        // is still the project's directory, with the project's rules.
        let listing = tree.listing(id(&tree, "a/d")).unwrap();
        let listed = with_hook(swap_while_reading(&root.join("a"), &outside), || {
            listing.run()
        });
        assert_eq!(listed.lost, None);
        assert_eq!(entries(&listed), kept, "show ignored: {show_ignored}");

        // The directory itself swapped for good: the result says so, and
        // nothing is read through the link.
        let e = id(&tree, "a/e");
        let listing = tree.listing(e).unwrap();
        let swap = {
            let (path, target) = (root.join("a/e"), outside.join("d"));
            move |point| {
                if point == Point::Opened {
                    swap_for_link(&path, &target);
                }
            }
        };
        let listed = with_hook(swap, || listing.run());
        assert_eq!(listed.lost, Some(Lost::Itself));
        assert!(listed.result().is_err());
        tree.fill(e, listed);
        assert!(tree.find("a/e").is_none());
        // Or for another directory, before the listing runs or once it has
        // checked the directory.
        let listing = tree.listing(id(&tree, "a/f")).unwrap();
        swap_for_dir(&root.join("a/f"));
        assert_eq!(listing.run().lost, Some(Lost::Itself));
        let listing = tree.listing(id(&tree, "a/g")).unwrap();
        let path = root.join("a/g");
        let swap = move |point| {
            if point == Point::Opened {
                swap_for_dir(&path);
            }
        };
        assert_eq!(with_hook(swap, || listing.run()).lost, Some(Lost::Itself));

        // A directory above the root swapped for a link: nothing is listed
        // through it. No events come for what is above the root, so the
        // tree stays as it was, and offers the root for listing.
        let rows = layout(&tree);
        let p = base.join("p");
        swap_for_link(&p, &outside);
        assert_eq!(tree.apply(&changes(&root, &["a"])), []);
        assert_eq!(layout(&tree), rows);
        assert!(tree.listing(tree.root()).is_some());
        swap_back(&p);
        // And swapped while that listing runs, as above. Once it is stored,
        // what is loaded is offered again: changes made meanwhile went
        // unreported.
        let listing = tree.listing(tree.root()).unwrap();
        let listed = with_hook(swap_while_reading(&p, &outside), || listing.run());
        assert_eq!(entries(&listed), ["a", ".gitignore"]);
        tree.fill(tree.root(), listed);
        assert_eq!(layout(&tree), rows);
        assert!(tree.listing(tree.root()).is_none());
        assert!(tree.listing(id(&tree, "a")).is_some());
    }
}

#[test]
fn apply_reads_rules_through_the_directory_it_checked() {
    // Other rules, and an entry `apply` will look for.
    let (_outside_dir, outside) = temp();
    make(
        &outside,
        &["d/.gitignore", "d/new.txt", "d/new.log", "d/leak.txt"],
    );
    fs::write(outside.join("d/.gitignore"), "*.txt\n!*.log\n").unwrap();
    for show_ignored in [false, true] {
        let (_dir, root) = temp();
        make(&root, &["a/d/.gitignore", "a/d/old.txt"]);
        fs::write(root.join("a/d/.gitignore"), "*.log\n").unwrap();
        let mut tree = FileTree::open(&root, TreeOptions { show_ignored }).unwrap();
        tree.expand(id(&tree, "a"));
        tree.expand(id(&tree, "a/d"));
        make(&root, &["a/d/new.txt", "a/d/new.log"]);

        // `a` swapped for a link once `apply` has looked up the first entry
        // and before it reads any rules: they, and the other entries, are
        // read in the directories it opened.
        let swap = {
            let (a, outside) = (root.join("a"), outside.clone());
            let mut swapped = false;
            move |point| {
                if point == Point::Stat && !swapped {
                    swapped = true;
                    swap_for_link(&a, &outside);
                }
            }
        };
        let batch = changes(&root, &["a/d/new.txt", "a/d/new.log", "a/d/leak.txt"]);
        with_hook(swap, || tree.apply(&batch));
        let mut expected = vec![
            "v a",
            "  v d",
            "    .gitignore",
            "    new.txt",
            "    old.txt",
        ];
        if show_ignored {
            expected.insert(3, "    new.log (ignored)");
        }
        assert_eq!(layout(&tree), expected, "show ignored: {show_ignored}");

        // The link's own event makes it the link.
        tree.apply(&changes(&root, &["a", "a.aside"]));
        assert_eq!(layout(&tree), ["> a.aside", "@ a"]);
    }
}

/// What the `ignore` crate's walker, configured as the listing's rules are
/// described, finds in `dir`: the entries kept, and those it filters. Inside
/// a repository it takes git's rules from that repository only, and outside
/// any it takes `.gitignore` files too.
fn walk(dir: &Path) -> (Vec<String>, Vec<String>) {
    let in_repository = dir
        .ancestors()
        .any(|dir| dir.join(".git").exists() || dir.join(".jj").exists());
    let names = |filtered: bool| {
        let mut builder = ignore::WalkBuilder::new(dir);
        builder.max_depth(Some(1)).follow_links(false);
        if filtered {
            // Without git's global excludes, as unit tests run the tree.
            builder
                .parents(true)
                .hidden(false)
                .require_git(in_repository)
                .git_global(false);
        } else {
            builder.standard_filters(false);
        }
        let mut names: Vec<String> = builder
            .build()
            .map(Result::unwrap)
            .filter(|entry| entry.depth() == 1 && entry.file_name() != ".git")
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let kept = names(true);
    let filtered: Vec<String> = names(false)
        .into_iter()
        .filter(|name| !kept.contains(name))
        .collect();
    (kept, filtered)
}

/// Checks every listed directory of `tree` that isn't ignored against
/// [`walk`].
fn check_against_walker(tree: &FileTree) {
    let mut dirs = vec![tree.root()];
    while let Some(dir) = dirs.pop() {
        let (mut kept, mut ignored) = (Vec::new(), Vec::new());
        for &child in tree.children(dir).unwrap() {
            let row = tree.row(child).unwrap();
            let name = row.name.to_string_lossy().into_owned();
            if row.ignored {
                ignored.push(name);
            } else {
                if tree.children(child).is_some() {
                    dirs.push(child);
                }
                kept.push(name);
            }
        }
        kept.sort();
        ignored.sort();
        let path = tree.path(dir).unwrap();
        let (walked, filtered) = walk(&path);
        assert_eq!(kept, walked, "{}", path.display());
        if tree.options().show_ignored {
            assert_eq!(ignored, filtered, "{}", path.display());
        }
    }
}

/// Opens `root` with each of `expand` expanded and checks it against the
/// walker; then makes `created` and checks again once `apply` has
/// classified them.
fn check_listings_and_apply(root: &Path, show_ignored: bool, expand: &[&str], created: &[&str]) {
    let mut tree = FileTree::open(root, TreeOptions { show_ignored }).unwrap();
    for dir in expand {
        tree.expand(id(&tree, dir));
    }
    check_against_walker(&tree);
    make(root, created);
    let batch: Vec<_> = created
        .iter()
        .map(|path| FsChange::Path(root.join(path.trim_end_matches('/'))))
        .collect();
    tree.apply(&batch);
    check_against_walker(&tree);
}

#[test]
fn listings_and_apply_filter_as_the_ignore_walker_does() {
    for show_ignored in [false, true] {
        let (_dir, root) = temp();
        make(
            &root,
            &[
                ".env",
                ".git/HEAD",
                ".git/info/exclude",
                ".gitignore",
                ".ignore",
                "a/build",
                "anchored.txt",
                "app.log",
                "build/out.o",
                "docs/one/draft.md",
                "docs/two/final.md",
                "excluded.txt",
                "keep.gen",
                "keep.log",
                "only_dir/",
                "secret",
                "secret_ok",
                "sub/.gitignore",
                "sub/anchored.txt",
                "sub/debug.log",
                "sub/deep/note.tmp",
                "sub/local/x",
                "target/",
                "x.gen",
            ],
        );
        symlink("target", root.join("link")).unwrap();
        fs::write(
            root.join(".gitignore"),
            "*.log\n!keep.log\n*.gen\nbuild/\n/anchored.txt\nsub/deep/*.tmp\n\
             docs/**/draft.md\nlink/\nonly_dir\n",
        )
        .unwrap();
        fs::write(root.join(".ignore"), "secret*\n!secret_ok\n!keep.gen\n").unwrap();
        fs::write(root.join("sub/.gitignore"), "!*.log\nlocal/\n").unwrap();
        fs::write(root.join(".git/info/exclude"), "excluded.txt\n").unwrap();

        let expand = ["a", "docs", "docs/one", "docs/two", "sub", "sub/deep"];
        // And entries created since, as `apply` classifies them.
        let created = [
            "new.log",
            "new.gen",
            "secret2",
            "anchored2.txt",
            "fresh/",
            "docs/one/build/",
            "docs/two/draft.md",
            "sub/new.log",
            "sub/deep/new.tmp",
            "sub/deep/new.txt",
        ];
        check_listings_and_apply(&root, show_ignored, &expand, &created);
    }
}

#[test]
fn git_global_excludes_rank_below_the_projects_ignore_files() {
    let (_dir, root) = temp();
    make(&root, &[".ignore", ".gitignore", ".git/info/exclude"]);
    fs::write(root.join(".ignore"), "!a.log\n").unwrap();
    fs::write(root.join(".gitignore"), "*.log\n!b.tmp\n").unwrap();
    fs::write(root.join(".git/info/exclude"), "c.txt\n!e.tmp\n").unwrap();
    let mut global = ignore::gitignore::GitignoreBuilder::new(&root);
    global.add_line(None, "*.tmp").unwrap();
    let global = global.build().unwrap();
    let dirs = listing::open_dirs(&root, listing::Access::Read).unwrap();
    let rules = listing::Rules::chain(&dirs, &root).unwrap();
    let ignored = |name: &str| rules.ignores(&global, &root.join(name), false);
    assert!(!ignored("a.log"), ".ignore over .gitignore");
    assert!(ignored("x.log"));
    assert!(!ignored("b.tmp"), ".gitignore over the global excludes");
    assert!(!ignored("e.tmp"), "git's exclude over the global excludes");
    assert!(ignored("c.txt"));
    assert!(ignored("y.tmp"));
    assert!(!ignored("z.txt"));
}

#[test]
fn case_variants_cost_io_only_where_case_is_ignored() {
    let (_dir, root) = temp();
    make(&root, &["Build/x.o", ".gitignore", "Probe"]);
    fs::write(root.join(".gitignore"), "build/\n").unwrap();
    let ignores_case = root.join("probe").exists();
    fs::remove_file(root.join("Probe")).unwrap();
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    assert_eq!(tree.ignores_case, ignores_case);
    // Where case tells names apart, `build/` is an ignored directory beside
    // `Build/`, and paths below it are dropped without IO like any other.
    tree.ignores_case = false;
    let (stats, io) = (tree.stats(), tree.io_calls);
    assert_eq!(tree.apply(&changes(&root, &["build/a.o", "build/b.o"])), []);
    assert_eq!((tree.stats(), tree.io_calls), (stats, io));
}

#[test]
fn ignore_files_are_not_followed_or_waited_on() {
    let (_outside_dir, outside) = temp();
    make(&outside, &["rules"]);
    fs::write(outside.join("rules"), "*.txt\n").unwrap();
    let (_dir, root) = temp();
    make(&root, &["a.txt", "b.log"]);
    // A `.gitignore` linking out of the project is skipped, as git skips one,
    // and a FIFO `.ignore` is skipped without blocking on it.
    symlink(outside.join("rules"), root.join(".gitignore")).unwrap();
    let mkfifo = std::process::Command::new("mkfifo")
        .arg(root.join(".ignore"))
        .status()
        .unwrap();
    assert!(mkfifo.success());
    let tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    assert_eq!(layout(&tree), ["@ .gitignore", ".ignore", "a.txt", "b.log"]);
}

#[test]
fn ignore_rules_stop_at_the_nearest_repository() {
    for show_ignored in [false, true] {
        // A project that is a repository of its own, inside another one
        // whose `.gitignore` and `info/exclude` don't apply in it, though its
        // `.ignore` does. It holds a nested repository and a linked worktree
        // of a third one, whose `.git` file leads to that one's
        // `info/exclude`.
        let (_dir, base) = temp();
        let root = base.join("proj");
        make(
            &base,
            &[
                ".git/HEAD",
                ".git/info/exclude",
                ".gitignore",
                ".ignore",
                "main/.git/HEAD",
                "main/.git/info/exclude",
                "main/.git/worktrees/wt/commondir",
                "proj/.git/HEAD",
                "proj/.gitignore",
                "proj/keep.txt",
                "proj/notes.md",
                "proj/outer.rs",
                "proj/app.log",
                "proj/plain/a.txt",
                "proj/plain/b.log",
                "proj/inner/.git/HEAD",
                "proj/inner/.git/info/exclude",
                "proj/inner/app.log",
                "proj/inner/keep.txt",
                "proj/inner/mine.rs",
                "proj/inner/x.md",
                "proj/inner/deep/y.log",
                "proj/wt/app.log",
                "proj/wt/linked.rs",
                "proj/wt/other.txt",
                "main/.git/worktrees/wt2/commondir",
                "proj/wt2/kept.rs",
                "proj/wt2/linked.rs",
            ],
        );
        fs::write(base.join(".gitignore"), "*.txt\n").unwrap();
        fs::write(base.join(".ignore"), "*.md\n").unwrap();
        fs::write(base.join(".git/info/exclude"), "outer.rs\n").unwrap();
        fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        fs::write(root.join("inner/.git/info/exclude"), "mine.rs\n").unwrap();
        fs::write(base.join("main/.git/info/exclude"), "linked.rs\n").unwrap();
        fs::write(base.join("main/.git/worktrees/wt/commondir"), "../..\n").unwrap();
        let gitdir = base.join("main/.git/worktrees/wt");
        fs::write(
            root.join("wt/.git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        // Another, with `..` in both absolute paths.
        let gitdir = base.join("main/../main/.git/worktrees/wt2");
        let common = base.join("main/.git/worktrees/wt2/../..");
        fs::write(
            root.join("wt2/.git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        fs::write(
            base.join("main/.git/worktrees/wt2/commondir"),
            format!("{}\n", common.display()),
        )
        .unwrap();
        let expand = ["plain", "inner", "inner/deep", "wt", "wt2"];
        let created = [
            "new.txt",
            "new.log",
            "new.md",
            "plain/new.log",
            "inner/new.log",
            "inner/deep/new.log",
            "wt/new.log",
            "wt/new.txt",
        ];
        check_listings_and_apply(&root, show_ignored, &expand, &created);
        let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
        for dir in expand {
            tree.expand(id(&tree, dir));
        }
        assert_eq!(
            layout(&tree),
            [
                "v inner",
                "  v deep",
                "    new.log",
                "    y.log",
                "  app.log",
                "  keep.txt",
                "  new.log",
                "v plain",
                "  a.txt",
                "v wt",
                "  app.log",
                "  new.log",
                "  new.txt",
                "  other.txt",
                "v wt2",
                "  kept.rs",
                ".gitignore",
                "keep.txt",
                "new.txt",
                "outer.rs",
            ]
        );

        // Outside any repository `.gitignore` files apply, but not in a
        // repository below.
        let (_dir, root) = temp();
        make(
            &root,
            &[".gitignore", "a.log", "nested/.git/HEAD", "nested/b.log"],
        );
        fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        check_listings_and_apply(&root, show_ignored, &["nested"], &["c.log", "nested/c.log"]);
    }
}

#[test]
fn retry_markers_keep_the_directories_above_from_dropping_them() {
    use std::{cell::RefCell, rc::Rc};

    let (_dir, base) = temp();
    let root = base.join("up/proj");
    make(&root, &["p/c/old.txt", "v/", "w/"]);
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();

    // Two overlapping refreshes of `p/c`, the inner one stored first: the
    // outer one is discarded, and `c` owes a refresh. A refresh of `p` that
    // started before then, while `c` was away for a moment, is older than
    // that, and mustn't drop `c` with what it owes.
    let p = list_owing(&mut tree, &root, "p");
    let c = list_owing(&mut tree, &root, "p/c");
    let inner = Rc::new(RefCell::new(None));
    let hook = {
        let (inner, mut listing) = (inner.clone(), tree.listing(c));
        move |point| {
            if point == Point::Read
                && let Some(listing) = listing.take()
            {
                *inner.borrow_mut() = Some(listing.run());
            }
        }
    };
    let outer = tree.listing(c).unwrap();
    let outer = with_hook(hook, || outer.run());
    tree.fill(c, inner.take().unwrap());
    assert!(tree.listing(c).is_none());
    let refresh = tree.listing(p).unwrap();
    let away = root.join("p/c");
    fs::rename(&away, aside(&away)).unwrap();
    let stale = refresh.run();
    swap_back_dir(&away);
    tree.fill(c, outer);
    assert!(tree.listing(c).is_some());
    tree.fill(p, stale);
    assert_eq!(tree.find("p/c"), Some(c));
    assert!(tree.listing(c).is_some() && tree.listing(p).is_some());
    for dir in [p, c] {
        let listing = tree.listing(dir).unwrap();
        tree.fill(dir, listing.run());
    }
    assert!(tree.find("p/c/old.txt").is_some() && tree.find("p/c/late.txt").is_some());
    assert!(tree.listing(p).is_none() && tree.listing(c).is_none());

    // A refresh of `p` run while `c` was away, filled after a listing of
    // another directory found the root cut off, and before the root is
    // found again: changes may have gone unreported since, `c`'s return
    // among them, so the refresh is older than that.
    {
        let (_dir, base) = temp();
        let root = base.join("up/proj");
        make(&root, &["p/c/old.txt", "w/"]);
        let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
        let p = list_owing(&mut tree, &root, "p");
        let c = id(&tree, "p/c");
        tree.expand(c);
        let refresh = tree.listing(p).unwrap();
        let away = root.join("p/c");
        fs::rename(&away, aside(&away)).unwrap();
        let stale = refresh.run();
        swap_back_dir(&away);
        let up = base.join("up");
        fs::rename(&up, aside(&up)).unwrap();
        let w = id(&tree, "w");
        let cut = tree.listing(w).unwrap().run();
        assert_eq!(cut.lost, Some(Lost::Root));
        tree.fill(w, cut);
        swap_back_dir(&up);
        tree.fill(p, stale);
        assert_eq!(tree.find("p/c"), Some(c));
        assert!(tree.find("p/c/old.txt").is_some());
        assert!(tree.listing(p).is_some());
    }

    // A refresh of `p` run while `c` was away, filled after a rescan that
    // the root, cut off, cut short: that rescan was news for everything.
    {
        let (_dir, base) = temp();
        let root = base.join("up/proj");
        make(&root, &["p/c/old.txt"]);
        let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
        let p = list_owing(&mut tree, &root, "p");
        let c = id(&tree, "p/c");
        tree.expand(c);
        let refresh = tree.listing(p).unwrap();
        let away = root.join("p/c");
        fs::rename(&away, aside(&away)).unwrap();
        let stale = refresh.run();
        swap_back_dir(&away);
        let up = base.join("up");
        fs::rename(&up, aside(&up)).unwrap();
        tree.apply(&[FsChange::Rescan]);
        swap_back_dir(&up);
        tree.fill(p, stale);
        assert_eq!(tree.find("p/c"), Some(c));
        assert!(tree.find("p/c/old.txt").is_some());
    }

    // The root found again by one listing, and another, started after that
    // one but before it was stored, that saw `v` away for a moment: the
    // recovery marks `v` as owing a listing, and the root as newer than the
    // second listing, which mustn't drop `v`.
    let (v, w) = (id(&tree, "v"), id(&tree, "w"));
    let up = base.join("up");
    fs::rename(&up, aside(&up)).unwrap();
    let cut = tree.listing(w).unwrap().run();
    assert_eq!(cut.lost, Some(Lost::Root));
    tree.fill(w, cut);
    swap_back_dir(&up);
    let (first, second) = (
        tree.listing(tree.root()).unwrap(),
        tree.listing(tree.root()).unwrap(),
    );
    let found = first.run();
    let away = root.join("v");
    fs::rename(&away, aside(&away)).unwrap();
    let stale = second.run();
    swap_back_dir(&away);
    tree.fill(tree.root(), found);
    tree.fill(tree.root(), stale);
    assert_eq!(tree.find("v"), Some(v));
    assert!(tree.listing(v).is_some());
    assert!(tree.listing(tree.root()).is_some());
    let listing = tree.listing(tree.root()).unwrap();
    tree.fill(tree.root(), listing.run());
    assert_eq!(tree.find("v"), Some(v));
    assert!(tree.listing(tree.root()).is_none());

    // A listing of the root that ran before the root was cut off again
    // (while `v`, loaded, was away for a moment), filled after: it can't
    // be the one that finds the root again, and doesn't drop `v`.
    tree.expand(v);
    let w = list_owing(&mut tree, &root, "w");
    fs::rename(&up, aside(&up)).unwrap();
    let cut = tree.listing(w).unwrap().run();
    tree.fill(w, cut);
    swap_back_dir(&up);
    let early = tree.listing(tree.root()).unwrap();
    let away = root.join("v");
    fs::rename(&away, aside(&away)).unwrap();
    let early = early.run();
    swap_back_dir(&away);
    fs::rename(&up, aside(&up)).unwrap();
    let cut = tree.listing(w).unwrap().run();
    assert_eq!(cut.lost, Some(Lost::Root));
    tree.fill(w, cut);
    swap_back_dir(&up);
    tree.fill(tree.root(), early);
    assert_eq!(tree.find("v"), Some(v));
    assert!(tree.listing(tree.root()).is_some());
}

#[test]
fn a_rescan_with_the_roots_own_path_drops_a_deleted_project() {
    // FSEvents reports the root's own path when a directory above it is
    // deleted, perhaps with a rescan in the same batch: the tree drops
    // everything rather than keep ghosts, and rescans keep it so.
    let (_dir, base) = temp();
    let root = base.join("up/proj");
    make(&root, &["d/y.txt", "x.txt"]);
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    tree.expand(id(&tree, "d"));
    fs::remove_dir_all(base.join("up")).unwrap();
    tree.apply(&[FsChange::Path(root.clone()), FsChange::Rescan]);
    assert_eq!(tree.row_count(), 0);
    assert_eq!(tree.stats().loaded_entries, 0);
    tree.apply(&[FsChange::Rescan]);
    let listing = tree.listing(tree.root()).unwrap();
    tree.fill(tree.root(), listing.run());
    assert_eq!(tree.row_count(), 0);
}

#[test]
fn listings_made_before_their_ignore_flag_changed_are_listed_again() {
    // A listing of an unlisted directory, made before a rescan changed
    // whether the directory is ignored, and run after it: it classified the
    // entries by the old flag, so it isn't stored, and the directory is
    // offered again. Both ways: `a` becomes ignored, `b` stops being.
    let (_dir, root) = temp();
    make(&root, &[".gitignore", "a/f.txt", "b/g.txt"]);
    fs::write(root.join(".gitignore"), "b/\n").unwrap();
    let mut tree = FileTree::open(&root, TreeOptions { show_ignored: true }).unwrap();
    let (a, b) = (id(&tree, "a"), id(&tree, "b"));
    let (early_a, early_b) = (tree.listing(a).unwrap(), tree.listing(b).unwrap());
    fs::write(root.join(".gitignore"), "a/\n").unwrap();
    tree.apply(&[FsChange::Rescan]);
    assert!(tree.row(a).unwrap().ignored && !tree.row(b).unwrap().ignored);
    for (dir, early) in [(a, early_a), (b, early_b)] {
        tree.fill(dir, early.run());
        assert!(tree.children(dir).is_none());
        let listing = tree.listing(dir).unwrap();
        tree.fill(dir, listing.run());
        assert!(tree.listing(dir).is_none());
    }
    assert!(tree.row(id(&tree, "a/f.txt")).unwrap().ignored);
    assert!(!tree.row(id(&tree, "b/g.txt")).unwrap().ignored);

    // A listed directory whose flag a stored listing of its parent changes:
    // its children were classified by the old flag, so it is offered again.
    make(&root, &["p/.gitignore", "p/c/f.txt"]);
    tree.apply(&changes(&root, &["p"]));
    let p = list_owing(&mut tree, &root, "p");
    let c = id(&tree, "p/c");
    tree.expand(c);
    fs::write(root.join("p/.gitignore"), "c/\n").unwrap();
    tree.apply(&changes(&root, &["p/.gitignore"]));
    let listing = tree.listing(p).unwrap();
    tree.fill(p, listing.run());
    assert!(tree.row(c).unwrap().ignored);
    let listing = tree.listing(c).unwrap();
    tree.fill(c, listing.run());
    assert!(tree.row(id(&tree, "p/c/f.txt")).unwrap().ignored);
    assert!(tree.listing(c).is_none());
}
