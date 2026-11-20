//! The file walk the search is built on, reached through the public API.

use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_project::{
    PathFilter, SearchOptions, SearchQuery, WalkControl, search_filtered, walk_files,
};

fn write(root: &Path, rel: &str, text: &[u8]) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn slash(rel: &Path) -> String {
    rel.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn walked(root: &Path, threads: usize, filter: &PathFilter) -> BTreeSet<String> {
    let seen = Mutex::new(BTreeSet::new());
    let summary = walk_files(
        root,
        threads,
        filter,
        Arc::new(AtomicBool::new(false)),
        |file| {
            seen.lock().unwrap().insert(slash(&file.rel));
            WalkControl::Continue
        },
    );
    assert!(!summary.cancelled);
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    seen.into_inner().unwrap()
}

fn tree() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, ".gitignore", b"ignored.txt\nbuild/\n");
    write(root, "a.txt", b"needle\n");
    write(root, "src/b.rs", b"needle\n");
    write(root, "src/deep/c.rs", b"needle\n");
    write(root, "ignored.txt", b"needle\n");
    write(root, "build/out.txt", b"needle\n");
    write(root, ".hidden", b"needle\n");
    write(root, "bin.dat", b"needle\0\x01\x02");
    write(root, "empty.txt", b"");
    tmp
}

#[test]
fn walk_files_visits_the_same_files_as_search_does_for_a_match_all_query() {
    let tmp = tree();
    let root = tmp.path();
    for threads in [1, 4] {
        let mut searched = BTreeSet::new();
        let summary = search_filtered(
            root,
            &SearchQuery {
                pattern: "".into(),
                regex: true,
                case_sensitive: true,
                whole_word: false,
            },
            &SearchOptions {
                threads,
                ..SearchOptions::default()
            },
            &PathFilter::default(),
            Arc::new(AtomicBool::new(false)),
            |batch| searched.extend(batch.into_iter().map(|file| file.rel)),
        )
        .unwrap();
        let walked = walked(root, threads, &PathFilter::default());
        // The search counts every file it opens, binary ones included.
        assert_eq!(walked.len(), summary.files_searched, "{walked:?}");
        let found: BTreeSet<String> = searched
            .iter()
            .map(|rel| rel.replace(std::path::MAIN_SEPARATOR, "/"))
            .collect();
        let expected: BTreeSet<String> = walked
            .iter()
            .filter(|rel| *rel != "bin.dat" && *rel != "empty.txt")
            .cloned()
            .collect();
        assert_eq!(found, expected);
        assert!(walked.contains("a.txt") && walked.contains("src/deep/c.rs"));
        assert!(walked.contains("bin.dat") && walked.contains(".gitignore"));
        assert!(!walked.contains("ignored.txt") && !walked.contains("build/out.txt"));
        // Hidden files are shown, as the tree shows them.
        assert!(walked.contains(".hidden"));
    }
}

#[test]
fn walk_files_honours_the_path_filter_and_prunes_excluded_directories() {
    let tmp = tree();
    let root = tmp.path();
    let only_rs = PathFilter::new(root, &["*.rs"], &[]).unwrap();
    assert_eq!(
        walked(root, 1, &only_rs),
        BTreeSet::from(["src/b.rs".to_string(), "src/deep/c.rs".to_string()])
    );
    let no_deep = PathFilter::new(root, &[], &["deep/"]).unwrap();
    let seen = walked(root, 4, &no_deep);
    assert!(seen.contains("src/b.rs") && !seen.contains("src/deep/c.rs"));
    let no_src = PathFilter::new(root, &[], &["src"]).unwrap();
    let seen = walked(root, 1, &no_src);
    assert!(seen.contains("a.txt"));
    assert!(!seen.iter().any(|rel| rel.starts_with("src/")), "{seen:?}");
}

#[test]
fn walk_files_stops_when_cancelled_and_reports_cancelled() {
    let tmp = tree();
    let root = tmp.path();
    // Cancelled before the walk: nothing is visited.
    let cancel = Arc::new(AtomicBool::new(true));
    let summary = walk_files(root, 1, &PathFilter::default(), cancel, |_| {
        panic!("visited after cancel")
    });
    assert!(summary.cancelled);
    // Cancelled from the first visit: no file after it.
    let cancel = Arc::new(AtomicBool::new(false));
    let count = Mutex::new(0);
    let summary = walk_files(root, 1, &PathFilter::default(), cancel.clone(), |_| {
        *count.lock().unwrap() += 1;
        cancel.store(true, Ordering::Relaxed);
        WalkControl::Continue
    });
    assert!(summary.cancelled);
    assert_eq!(*count.lock().unwrap(), 1);
    // Stop ends the walk without being a cancel.
    let count = Mutex::new(0);
    let summary = walk_files(
        root,
        1,
        &PathFilter::default(),
        Arc::new(AtomicBool::new(false)),
        |_| {
            *count.lock().unwrap() += 1;
            WalkControl::Stop
        },
    );
    assert!(!summary.cancelled);
    assert_eq!(*count.lock().unwrap(), 1);
}

#[cfg(unix)]
#[test]
fn walk_files_opens_beneath_the_root_and_skips_a_symlink_swapped_in() {
    use std::io::Read;

    let tmp = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(tmp.path()).unwrap();
    let root = base.join("project");
    write(&root, "sub/a.txt", b"inside a\n");
    write(&root, "sub/b.txt", b"inside b\n");
    write(&base, "outside/b.txt", b"OUTSIDE\n");
    let texts = Mutex::new(Vec::new());
    let summary = walk_files(
        &root,
        1,
        &PathFilter::default(),
        Arc::new(AtomicBool::new(false)),
        |mut file| {
            let mut text = String::new();
            file.file.read_to_string(&mut text).unwrap();
            if slash(&file.rel) == "sub/a.txt" {
                // The walk has listed `sub`: swap it for a link out.
                fs::rename(root.join("sub"), root.join("sub-real")).unwrap();
                std::os::unix::fs::symlink(base.join("outside"), root.join("sub")).unwrap();
            }
            texts.lock().unwrap().push(text);
            WalkControl::Continue
        },
    );
    let texts = texts.into_inner().unwrap();
    assert_eq!(texts, ["inside a\n"], "{summary:?}");
    assert!(!summary.cancelled);
    assert_eq!(summary.errors.len(), 1, "{summary:?}");
}
