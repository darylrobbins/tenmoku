use std::sync::{Arc, atomic::AtomicBool};

use super::*;
use crate::fixture;

/// Walks `root` into a new index and finishes it.
fn walked(root: &Path) -> FinderIndex {
    let mut index = FinderIndex::new(root);
    assert!(FinderIndex::walk(
        root,
        Arc::new(AtomicBool::new(false)),
        |batch| index.extend(batch)
    ));
    index.finish();
    index
}

/// Every indexed path, in path order.
fn all(index: &mut FinderIndex) -> Vec<String> {
    index
        .query("", usize::MAX)
        .into_iter()
        .map(|found| slashed(found.path))
        .collect()
}

fn found(index: &mut FinderIndex, text: &str) -> Vec<String> {
    index
        .query(text, usize::MAX)
        .into_iter()
        .map(|found| slashed(found.path))
        .collect()
}

/// Checks that `results` are in rank order and each one's indices spell
/// `text` in its path, ignoring case.
fn check_highlights(text: &str, results: &[FinderMatch]) {
    for pair in results.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let (a_len, b_len) = (a.path.chars().count(), b.path.chars().count());
        assert!(
            (b.score, a_len, &a.path) < (a.score, b_len, &b.path),
            "{a:?} should rank above {b:?}"
        );
    }
    for result in results {
        assert!(
            result.indices.windows(2).all(|pair| pair[0] < pair[1]),
            "{result:?}"
        );
        let chars: Vec<char> = result.path.chars().collect();
        let spelled: Vec<char> = result.indices.iter().map(|&ix| chars[ix]).collect();
        // Either case of a letter, as one char or several (`ς` and `Σ` are
        // the same uppercased, `Σ` and `σ` lowercased).
        let same = |a: char, b: char| {
            a == b || a.to_lowercase().eq(b.to_lowercase()) || a.to_uppercase().eq(b.to_uppercase())
        };
        assert!(
            spelled.len() == text.chars().count()
                && spelled.iter().zip(text.chars()).all(|(&a, b)| same(a, b)),
            "{result:?} should spell {text}"
        );
    }
}

#[test]
fn walk_indexes_every_unignored_file() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "README.md",
            ".env",
            "src/main.rs",
            "src/deep/er/mod.rs",
            "src/deep/er/notes.log",
            "src/vendor/.gitignore",
            "src/vendor/kept.rs",
            "src/vendor/generated.rs",
            "build/out.o",
            "scratch/a.txt",
            "debug.log",
            "empty/",
            "nested/empty/",
            ".git/HEAD",
            ".git/objects/ab/cdef",
            "src/.git/config",
        ],
    );
    fs::write(root.join(".gitignore"), "*.log\nbuild/\n").unwrap();
    fs::write(root.join(".ignore"), "scratch/\n").unwrap();
    fs::write(root.join("src/vendor/.gitignore"), "generated.rs\n").unwrap();

    let mut index = walked(&root);
    assert_eq!(
        all(&mut index),
        [
            ".env",
            ".gitignore",
            ".ignore",
            "README.md",
            "src/deep/er/mod.rs",
            // `src` is a repository of its own, where the root's `.gitignore`
            // doesn't apply, as in git.
            "src/deep/er/notes.log",
            "src/main.rs",
            "src/vendor/.gitignore",
            "src/vendor/kept.rs",
        ]
    );
    assert_eq!(index.progress(), (9, true));
}

#[test]
fn results_are_ranked_limited_and_highlighted() {
    let paths = [
        "src/main.rs",
        "src/maintenance.rs",
        "docs/manual.md",
        "tests/my_app_in.rs",
        "a/m/a/i/n.txt",
        "docs/résumé.md",
        "naïve/ñandú.txt",
        "Makefile",
    ];
    let mut index = FinderIndex::from_paths(paths);
    for text in ["main", "MAIN", "rsm", "ñandú", "ndtxt", "m"] {
        let every = index.query(text, usize::MAX);
        assert!(!every.is_empty(), "{text} matches something");
        check_highlights(text, &every);
        let limited = index.query(text, 2);
        assert_eq!(limited.len(), every.len().min(2));
        assert_eq!(limited, every[..limited.len()]);
    }
    assert_eq!(found(&mut index, "rsm"), ["docs/résumé.md"]);
    // Chars, not bytes: `r`, `s` and `m` of "résumé", then `m` of "md".
    assert_eq!(index.query("rsmm", 1)[0].indices, [5, 7, 9, 12]);
    assert_eq!(index.query("ñandú", 1)[0].indices, [6, 7, 8, 9, 10]);
    assert!(index.query("main", 0).is_empty());

    // Equal scores: the shorter path first, then path order.
    let mut index = FinderIndex::from_paths(["bb/x.rs", "a/x.rs", "b/x.rs"]);
    let results = index.query("x", 3);
    assert_eq!(results[0].score, results[2].score);
    let order: Vec<_> = results.into_iter().map(|found| found.path).collect();
    assert_eq!(order, ["a/x.rs", "b/x.rs", "bb/x.rs"]);
}

#[test]
fn file_name_matches_rank_first() {
    let mut index = FinderIndex::from_paths([
        "m/a/i/n",
        "my/app/in/n.rs",
        "src/main.rs",
        "crates/fe-editor/src/lib.rs",
        "crates/fe-project/src/lib.rs",
        "lib.rs/other.rs",
        "lib/rs/thing.txt",
        "src/lib.rs.bak",
        "src/my_lib.rs",
        "src/liblib.rs",
        "crates/fe-editor/src/view.rs",
        "crates/fe-gpui/src/view.rs",
    ]);

    let main = found(&mut index, "main");
    let pos = |name: &str| main.iter().position(|path| path == name).unwrap();
    assert!(pos("src/main.rs") < pos("m/a/i/n"), "{main:?}");
    assert!(pos("src/main.rs") < pos("my/app/in/n.rs"), "{main:?}");

    let lib = found(&mut index, "lib.rs");
    let ends = lib.iter().filter(|path| path.ends_with("/lib.rs")).count();
    assert_eq!(ends, 2);
    assert!(
        lib[..ends].iter().all(|path| path.ends_with("/lib.rs")),
        "{lib:?}"
    );
    assert!(lib.len() > ends, "other paths match too: {lib:?}");

    assert_eq!(
        found(&mut index, "fe-ed/view"),
        ["crates/fe-editor/src/view.rs"]
    );
}

#[test]
fn empty_query_lists_paths_in_order() {
    let mut index = FinderIndex::from_paths(["src/b.rs", "Cargo.toml", "src/a.rs", "README.md"]);
    let results = index.query("", 3);
    let paths: Vec<_> = results.iter().map(|found| found.path.as_str()).collect();
    assert_eq!(paths, ["Cargo.toml", "README.md", "src/a.rs"]);
    assert!(results.iter().all(|found| found.indices.is_empty()));
}

#[test]
fn queries_see_paths_indexed_so_far() {
    let mut index = FinderIndex::new("/nonexistent");
    index.extend(["src/alpha.rs", "src/beta.rs"].map(PathBuf::from));
    assert_eq!(index.progress(), (2, false));
    assert_eq!(found(&mut index, "rs"), ["src/beta.rs", "src/alpha.rs"]);
    assert!(found(&mut index, "gamma").is_empty());

    index.extend(["src/gamma.rs", "src/beta.rs"].map(PathBuf::from));
    index.finish();
    assert_eq!(index.progress(), (3, true));
    assert_eq!(found(&mut index, "gamma"), ["src/gamma.rs"]);
    assert_eq!(found(&mut index, "alpha"), ["src/alpha.rs"]);
}

#[test]
fn a_cancelled_walk_stops() {
    let (_dir, root) = temp();
    make(&root, &["a.txt", "src/b.rs"]);
    let mut index = FinderIndex::new(&root);
    let mut batches = 0;
    let finished = FinderIndex::walk(&root, Arc::new(AtomicBool::new(true)), |batch| {
        batches += 1;
        index.extend(batch);
    });
    assert!(!finished);
    assert_eq!(batches, 0);
    assert_eq!(index.progress(), (0, false));
}

#[test]
fn apply_updates_without_rewalking() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "src/old.rs",
            "src/before.rs",
            "src/kept.rs",
            "gone/a.rs",
            "gone/deep/b.rs",
            "build/stale.o",
        ],
    );
    fs::write(root.join(".gitignore"), "build/\n*.log\n/fresh/skip.rs\n").unwrap();
    let mut index = walked(&root);
    let walks = index.stats().walks;

    // The new directories hold files the root's `.gitignore` ignores, which
    // their walks leave out.
    make(
        &root,
        &[
            "src/new.rs",
            "fresh/one.rs",
            "fresh/skip.rs",
            "fresh/sub/two.rs",
            "fresh/sub/skip.rs",
            "fresh/sub/three.log",
            "other/three.rs",
        ],
    );
    fs::remove_file(root.join("src/old.rs")).unwrap();
    fs::rename(root.join("src/before.rs"), root.join("src/after.rs")).unwrap();
    fs::remove_dir_all(root.join("gone")).unwrap();
    index.apply(&changes(
        &root,
        &[
            "src/new.rs",
            "src/old.rs",
            "src/before.rs",
            "src/after.rs",
            "gone",
            "gone/a.rs",
            "fresh",
            "fresh/one.rs",
            "fresh/sub",
            "fresh/sub/two.rs",
            "other",
            "other/three.rs",
        ],
    ));
    assert_eq!(
        all(&mut index),
        [
            ".gitignore",
            "fresh/one.rs",
            "fresh/sub/skip.rs",
            "fresh/sub/two.rs",
            "other/three.rs",
            "src/after.rs",
            "src/kept.rs",
            "src/new.rs",
        ]
    );
    // One walk each for `fresh` (with `fresh/sub`, which it covers) and
    // `other`; none for files or removals.
    assert_eq!(index.stats().walks, walks + 2);

    // Ignored paths: created, changed, removed, and a new ignored directory.
    let before = all(&mut index);
    make(
        &root,
        &["build/new.o", "debug.log", "src/trace.log", "logs/"],
    );
    fs::write(root.join(".gitignore"), "build/\n*.log\nlogs/\n").unwrap();
    make(&root, &["logs/today.txt"]);
    fs::remove_file(root.join("build/stale.o")).unwrap();
    index.apply(&changes(
        &root,
        &[
            "build/new.o",
            "build/stale.o",
            "build",
            "debug.log",
            "src/trace.log",
            "logs",
            "logs/today.txt",
            ".git/index",
        ],
    ));
    assert_eq!(all(&mut index), before);
    assert_eq!(index.stats().walks, walks + 2);
}

/// The only finder test that generates the 100k-file fixture (see
/// `large.rs`).
#[test]
fn a_100k_file_index_bounds_query_work() {
    let (_dir, root) = temp();
    fixture::generate(&root, 100, 1_000).unwrap();

    let mut index = walked(&root);
    // The fixture's files and its `.gitignore`; the ignored directory is
    // left out.
    let (count, complete) = index.progress();
    assert!(complete);
    assert_eq!(count, 100_000 + 1);
    let files = found(&mut index, "file_");
    assert_eq!(files.len(), 100_000);
    assert!(files.iter().all(|path| path.starts_with("dir_")));

    let computed = index.stats().indices_computed;
    let results = index.query("d42f7", 50);
    assert_eq!(results.len(), 50);
    check_highlights("d42f7", &results);
    assert!(index.stats().indices_computed - computed <= 50);
}

#[test]
fn apply_handles_replacements_rescans_and_building() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "src/a.rs",
            "was_file",
            "was_dir/x.rs",
            "Case.rs",
            "target.rs",
            "gone/a.rs",
            "gone/b.rs",
        ],
    );
    let mut index = walked(&root);
    let walks = index.stats().walks;

    // Another directory in place of an indexed one, reported by its own
    // path only: walked, and what was indexed under the old one dropped.
    fs::remove_dir_all(root.join("src")).unwrap();
    make(&root, &["src/new.rs"]);
    // A directory removed, reported by one of its files only.
    fs::remove_dir_all(root.join("gone")).unwrap();
    index.apply(&changes(&root, &["src", "gone/a.rs"]));
    assert_eq!(index.stats().walks, walks + 1);
    assert_eq!(
        all(&mut index),
        [
            "Case.rs",
            "src/new.rs",
            "target.rs",
            "was_dir/x.rs",
            "was_file"
        ]
    );

    fs::remove_file(root.join("was_file")).unwrap();
    make(&root, &["was_file/inner.rs"]);
    fs::remove_dir_all(root.join("was_dir")).unwrap();
    make(&root, &["was_dir"]);
    fs::rename(root.join("Case.rs"), root.join("case.rs")).unwrap();
    // Windows makes file symlinks only with a privilege: there, `link.rs`
    // is missing and `src/new.rs` gone, which index the same.
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("target.rs"), root.join("link.rs")).unwrap();
    fs::remove_file(root.join("src/new.rs")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("target.rs"), root.join("src/new.rs")).unwrap();
    index.apply(&changes(
        &root,
        &[
            "was_file",
            "was_dir",
            "Case.rs",
            "case.rs",
            "link.rs",
            "src/new.rs",
        ],
    ));
    assert_eq!(
        all(&mut index),
        ["case.rs", "target.rs", "was_dir", "was_file/inner.rs"]
    );
    assert_eq!(index.stats().walks, walks + 2);

    // A rescan finds what no event reported.
    make(&root, &["unreported.rs"]);
    index.apply(&[FsChange::Rescan]);
    assert!(all(&mut index).contains(&"unreported.rs".to_owned()));
    assert_eq!(index.stats().walks, walks + 3);

    // While building, a batch read before a change can arrive after it;
    // `finish` applies the change again.
    let mut index = FinderIndex::new(&root);
    index.extend(["target.rs", "was_file/inner.rs"].map(|rel| PathBuf::from(native(rel))));
    fs::remove_file(root.join("target.rs")).unwrap();
    fs::remove_dir_all(root.join("was_file")).unwrap();
    make(&root, &["was_file/other.rs"]);
    index.apply(&changes(&root, &["target.rs", "was_file"]));
    index.extend(["target.rs", "was_file/inner.rs"].map(|rel| PathBuf::from(native(rel))));
    assert_eq!(found(&mut index, "target"), ["target.rs"]);
    index.finish();
    assert!(found(&mut index, "target").is_empty());
    assert_eq!(
        found(&mut index, &native("was_file/")),
        ["was_file/other.rs"]
    );
}

#[test]
fn a_root_removed_empties_the_index_until_it_is_back() {
    let (dir, root) = temp();
    let root = root.join("project");
    make(&root, &["a.rs"]);
    let mut index = walked(&root);
    let walks = index.stats().walks;

    fs::remove_dir_all(&root).unwrap();
    index.apply(&changes(&root, &["a.rs"]));
    assert_eq!(index.progress(), (0, true));

    make(&root, &["b.rs", "c.rs"]);
    index.apply(&changes(&root, &["b.rs"]));
    assert_eq!(all(&mut index), ["b.rs", "c.rs"]);
    assert_eq!(index.stats().walks, walks + 1);

    // Another directory in its place is walked too.
    fs::rename(&root, dir.path().join("old")).unwrap();
    make(&root, &["d.rs"]);
    index.apply(&changes(&root, &["d.rs"]));
    assert_eq!(all(&mut index), ["d.rs"]);
    assert_eq!(index.stats().walks, walks + 2);
}

#[test]
fn highlights_ignore_case_as_matching_does() {
    // Letters whose cases don't map back and forth: final sigma `ς`
    // uppercases to `Σ`, which lowercases to `σ`; micro `µ` uppercases to
    // Greek `Μ`.
    let mut index = FinderIndex::from_paths(["src/Σ.rs", "Σ/r.rs", "src/Μ.rs", "Μ/r.rs"]);
    for (text, paths) in [
        ("ς", ["src/Σ.rs", "Σ/r.rs"]),
        ("ς/r", ["Σ/r.rs", ""]),
        ("µ", ["src/Μ.rs", "Μ/r.rs"]),
        ("µ/r", ["Μ/r.rs", ""]),
    ] {
        let results = index.query(text, 4);
        let found: Vec<_> = results.iter().map(|found| found.path.as_str()).collect();
        let want: Vec<_> = paths.into_iter().filter(|path| !path.is_empty()).collect();
        assert_eq!(found, want, "{text}");
        check_highlights(text, &results);
    }
    // Spread over a long path, where frizbee's alignment is cut short and the
    // highlights are found again.
    let long = format!("a{}Σ.rs", "x".repeat(80));
    let mut index = FinderIndex::from_paths([long.as_str()]);
    let results = index.query("aς", 1);
    assert_eq!(results.len(), 1);
    check_highlights("aς", &results);
}

#[test]
fn highlights_cover_matches_spread_over_long_paths() {
    let name = format!("a{}b.rs", "x".repeat(80));
    let dirs = format!("a{}/b.rs", "x".repeat(200));
    let mut index = FinderIndex::from_paths([name.as_str(), dirs.as_str()]);
    for text in ["ab", "a/b"] {
        let results = index.query(text, 2);
        assert!(!results.is_empty(), "{text}");
        check_highlights(text, &results);
    }
}

#[test]
#[cfg(unix)]
fn names_that_are_not_utf8_are_lossy_with_the_real_path_kept() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let real = Path::new(OsStr::from_bytes(b"src/caf\xe9.rs"));
    let mut index = FinderIndex::from_paths([real, Path::new("src/ÑANDÚ.rs")]);
    let results = index.query("caf", 1);
    assert_eq!(results[0].path, "src/caf\u{fffd}.rs");
    assert_eq!(results[0].relative, real);
    check_highlights("caf", &results);
    // Case is ignored for non-ASCII needles too.
    let results = index.query("ñandú", 1);
    assert_eq!(results[0].path, "src/ÑANDÚ.rs");
    check_highlights("ñandú", &results);

    // Path order goes by the paths as shown: `é` before `\u{fffd}`.
    let lossy = Path::new(OsStr::from_bytes(b"\x80/x.rs"));
    let mut index = FinderIndex::from_paths([lossy, Path::new("é/x.rs")]);
    assert_eq!(all(&mut index), ["é/x.rs", "\u{fffd}/x.rs"]);
    assert_eq!(found(&mut index, "x"), ["é/x.rs", "\u{fffd}/x.rs"]);
}

#[test]
fn walks_filter_as_the_tree_does_across_repositories() {
    // Outside any repository `.gitignore` files apply, but not in a
    // repository below; a submodule's `.git` file leads to its
    // `info/exclude`. The walk, `apply` and the tree agree.
    let (_dir, root) = temp();
    make(
        &root,
        &[
            ".gitignore",
            "a.txt",
            "b.rs",
            "nested/.git/HEAD",
            "nested/keep.txt",
            "nested/deep/also.txt",
            "outer/.git/HEAD",
            "outer/.git/modules/sub/HEAD",
            "outer/.git/modules/sub/info/exclude",
            "outer/c.txt",
            "outer/sub/x.rs",
            "outer/sub/y.rs",
        ],
    );
    fs::write(root.join(".gitignore"), "*.txt\n").unwrap();
    fs::write(root.join("outer/.git/modules/sub/info/exclude"), "x.rs\n").unwrap();
    fs::write(root.join("outer/sub/.git"), "gitdir: ../.git/modules/sub\n").unwrap();
    let expected = [
        ".gitignore",
        "b.rs",
        "nested/deep/also.txt",
        "nested/keep.txt",
        "outer/c.txt",
        "outer/sub/y.rs",
    ];
    let mut index = walked(&root);
    assert_eq!(all(&mut index), expected);

    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    for dir in ["nested", "nested/deep", "outer", "outer/sub"] {
        tree.expand(tree.find(dir).unwrap());
    }
    let mut files: Vec<String> = (0..tree.row_count())
        .filter_map(|ix| tree.rows(ix..ix + 1).next())
        .filter(|row| row.kind == EntryKind::File)
        .map(|row| {
            let path = tree.path(row.id).unwrap();
            slashed(
                path.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            )
        })
        .collect();
    files.sort();
    assert_eq!(files, expected);

    let mut index = FinderIndex::new(&root);
    index.finish();
    let paths: Vec<FsChange> = [".gitignore", "a.txt", "b.rs", "nested", "outer"]
        .iter()
        .map(|path| FsChange::Path(root.join(path)))
        .collect();
    index.apply(&paths);
    assert_eq!(all(&mut index), expected);
}

#[test]
fn walks_from_a_relative_root_filter_as_from_an_absolute_one() {
    let (_dir, root) = temp();
    make(
        &root,
        &[".gitignore", "a.rs", "b.log", "sub/c.rs", "sub/d.log"],
    );
    fs::write(root.join(".gitignore"), "*.log\n").unwrap();
    let expected = [".gitignore", "a.rs", "sub/c.rs"];
    assert_eq!(all(&mut walked(&root)), expected);
    // None where the temporary folder is on another Windows drive.
    if let Some(rel) = relative(&root) {
        assert_eq!(all(&mut walked(&rel)), expected);
    }
}
