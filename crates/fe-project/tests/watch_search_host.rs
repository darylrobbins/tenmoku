//! fe-host's `WatchHost` and `SearchHost` through their local impls
//! (`LocalWatch`, `LocalSearch`): the same changes, matches and file list
//! as calling fe-project's watcher, search and finder directly.

use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::AtomicBool, mpsc},
    time::{Duration, Instant},
};

use fe_host::{FilesSummary, SearchFailed, SearchFilter, SearchHost, WatchHost};
use fe_project::{
    FileMatches, FinderIndex, FsChange, LocalSearch, LocalWatch, PathFilter, ProjectWatcher,
    SearchError, SearchOptions, SearchQuery, SearchSummary, WalkControl, search, search_excluding,
    search_filtered, walk_files,
};

/// An upper bound only, for a loaded machine: each wait ends as soon as
/// what it waits for arrives.
const TIMEOUT: Duration = Duration::from_secs(10);

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A small project: ignored, hidden and nested files, matches in most.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", "ignored.txt\nbuild/\n");
    write(root, "a.txt", "needle\nhay\nneedle needle\n");
    write(root, "src/b.rs", "fn needle() {}\n");
    write(root, "src/deep/c.rs", "// needle\n");
    write(root, "src/d.rs", "nothing here\n");
    write(root, "docs/e.md", "a needle\n");
    write(root, "ignored.txt", "needle\n");
    write(root, "build/out.txt", "needle\n");
    dir
}

fn one_thread() -> SearchOptions {
    SearchOptions {
        threads: 1,
        ..SearchOptions::default()
    }
}

/// Every batch a direct search's sink got, and its summary.
fn direct(
    run: impl FnOnce(
        Arc<AtomicBool>,
        &mut dyn FnMut(Vec<FileMatches>),
    ) -> Result<SearchSummary, SearchError>,
) -> (Vec<Vec<FileMatches>>, SearchSummary) {
    let mut batches = Vec::new();
    let summary = run(Arc::default(), &mut |batch| batches.push(batch)).unwrap();
    (batches, summary)
}

/// Every batch a host's search streamed, and its summary.
fn hosted(
    host: &dyn SearchHost,
    root: &Path,
    query: &SearchQuery,
    filter: &SearchFilter,
) -> (Vec<Vec<FileMatches>>, SearchSummary) {
    let mut stream = host.search(root, query, &one_thread(), filter);
    let batches: Vec<_> = stream.by_ref().collect();
    let summary = stream.finish().expect("an end value").unwrap();
    (batches, summary)
}

#[test]
fn a_hosted_search_streams_what_search_hands_its_sink() {
    let dir = project();
    let query = SearchQuery::literal("needle");
    let expected = direct(|cancel, sink| search(dir.path(), &query, &one_thread(), cancel, sink));
    assert!(!expected.0.is_empty());
    let got = hosted(&LocalSearch, dir.path(), &query, &SearchFilter::default());
    assert_eq!(got, expected);
}

#[test]
fn a_hosted_search_with_globs_matches_search_filtered() {
    let dir = project();
    let query = SearchQuery::literal("needle");
    let (include, exclude) = (["*.rs", "*.md"], ["src/deep/"]);
    let filter = PathFilter::new(dir.path(), &include, &exclude).unwrap();
    let expected = direct(|cancel, sink| {
        search_filtered(dir.path(), &query, &one_thread(), &filter, cancel, sink)
    });
    let rels: Vec<_> = (expected.0.iter().flatten())
        .map(|file| file.rel.as_str())
        .collect();
    assert_eq!(rels, ["docs/e.md", "src/b.rs"]);
    let globs = SearchFilter::globs(&include, &exclude);
    let got = hosted(&LocalSearch, dir.path(), &query, &globs);
    assert_eq!(got, expected);
}

#[test]
fn a_hosted_search_excluding_a_file_matches_search_excluding() {
    let dir = project();
    let query = SearchQuery::literal("needle");
    let excluded = Path::new("a.txt");
    let expected = direct(|cancel, sink| {
        search_excluding(dir.path(), &query, &one_thread(), excluded, cancel, sink)
    });
    assert!(expected.0.iter().flatten().all(|file| file.rel != "a.txt"));
    let filter = SearchFilter::excluding(excluded);
    let got = hosted(&LocalSearch, dir.path(), &query, &filter);
    assert_eq!(got, expected);
}

#[test]
fn a_hosted_search_with_an_invalid_pattern_ends_with_searchs_error() {
    let dir = project();
    let query = SearchQuery {
        regex: true,
        ..SearchQuery::literal("(")
    };
    let Err(SearchError::InvalidPattern(expected)) =
        search(dir.path(), &query, &one_thread(), Arc::default(), |_| {})
    else {
        panic!("the pattern parsed");
    };
    let host: &dyn SearchHost = &LocalSearch;
    let stream = host.search(dir.path(), &query, &one_thread(), &SearchFilter::default());
    assert!(stream.recv().is_none());
    match stream.finish() {
        Some(Err(SearchFailed::InvalidPattern(message))) => assert_eq!(message, expected),
        other => panic!("ended with {other:?}"),
    }
}

#[test]
fn a_hosted_search_with_an_invalid_glob_ends_with_path_filters_error() {
    let dir = project();
    let expected = PathFilter::new(dir.path(), &["src/[ab"], &[]).unwrap_err();
    let host: &dyn SearchHost = &LocalSearch;
    let filter = SearchFilter::globs(&["src/[ab"], &[]);
    let query = SearchQuery::literal("needle");
    let stream = host.search(dir.path(), &query, &one_thread(), &filter);
    match stream.finish() {
        Some(Err(SearchFailed::InvalidGlob { glob, message })) => {
            assert_eq!((glob, message), (expected.glob, expected.message));
        }
        other => panic!("ended with {other:?}"),
    }
}

#[test]
fn dropping_a_hosted_search_cancels_it() {
    let dir = tempfile::tempdir().unwrap();
    // Batches of 64 files on one thread: the walk can't get past the few
    // batches the stream holds before the drop.
    let total = 400;
    for index in 0..total {
        write(dir.path(), &format!("f{index:03}.txt"), "needle\n");
    }
    let host: &dyn SearchHost = &LocalSearch;
    let query = SearchQuery::literal("needle");
    let stream = host.search(dir.path(), &query, &one_thread(), &SearchFilter::default());
    let ended = stream.ended();
    assert!(stream.recv().is_some());
    drop(stream);
    let summary = ended.wait().expect("an end value").unwrap();
    assert!(summary.cancelled);
    assert!(summary.files_searched < total, "{summary:?}");
}

/// Every path a direct finder walk sent, in order.
fn finder_walk(root: &Path) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    assert!(FinderIndex::walk(root, Arc::default(), |batch| {
        paths.extend(batch)
    }));
    paths
}

/// Every path a host's file list sent, checking none came twice.
fn hosted_files(root: &Path, filter: &SearchFilter) -> BTreeSet<PathBuf> {
    let host: &dyn SearchHost = &LocalSearch;
    let mut stream = host.files(root, filter);
    let mut paths = BTreeSet::new();
    for batch in stream.by_ref() {
        assert!(!batch.is_empty());
        for path in batch {
            assert!(paths.insert(path));
        }
    }
    let end = stream.finish().expect("an end value").unwrap();
    assert_eq!(end, FilesSummary { complete: true });
    paths
}

#[test]
fn a_hosted_file_list_is_the_finders() {
    let dir = project();
    let expected = finder_walk(dir.path());
    assert!(expected.contains(Path::new("src/deep/c.rs")));
    assert!(!expected.contains(Path::new("ignored.txt")));
    assert_eq!(hosted_files(dir.path(), &SearchFilter::default()), expected);
}

#[test]
fn a_hosted_file_list_with_globs_takes_what_the_search_walk_takes() {
    let dir = project();
    let (include, exclude) = (["*.rs", "*.md"], ["src/deep/"]);
    let filter = PathFilter::new(dir.path(), &include, &exclude).unwrap();
    let expected = search_walked(dir.path(), &filter);
    assert_eq!(
        expected,
        BTreeSet::from(["docs/e.md", "src/b.rs", "src/d.rs"].map(PathBuf::from))
    );
    let globs = SearchFilter::globs(&include, &exclude);
    assert_eq!(hosted_files(dir.path(), &globs), expected);
}

/// The files the search walk takes with `filter`.
fn search_walked(root: &Path, filter: &PathFilter) -> BTreeSet<PathBuf> {
    let walked = Mutex::new(BTreeSet::new());
    walk_files(root, 1, filter, Arc::default(), |file| {
        walked.lock().unwrap().insert(file.rel);
        WalkControl::Continue
    });
    walked.into_inner().unwrap()
}

#[test]
fn a_hosted_file_list_keeps_an_excluded_directorys_negated_files_out() {
    let dir = project();
    write(dir.path(), "src/deep/keep.rs", "needle\n");
    write(dir.path(), "docs/old/notes/keep.md", "needle\n");
    // The walk never enters an excluded directory, so a negated glob for a
    // file in it brings nothing back.
    let exclude = [
        "src/deep/",
        "!src/deep/keep.rs",
        "docs/old/",
        "!docs/old/notes/keep.md",
    ];
    let filter = PathFilter::new(dir.path(), &[], &exclude).unwrap();
    let expected = search_walked(dir.path(), &filter);
    assert!(expected.contains(Path::new("src/b.rs")));
    assert!(!expected.contains(Path::new("src/deep/keep.rs")));
    assert!(!expected.contains(Path::new("docs/old/notes/keep.md")));
    let globs = SearchFilter::globs(&[], &exclude);
    assert_eq!(hosted_files(dir.path(), &globs), expected);
}

#[test]
fn a_hosted_file_list_leaves_the_excluded_file_out() {
    let dir = project();
    let mut expected = finder_walk(dir.path());
    assert!(expected.remove(Path::new("src/b.rs")));
    let filter = SearchFilter::excluding("src/b.rs");
    assert_eq!(hosted_files(dir.path(), &filter), expected);
}

/// Every change `next` delivers up to and including the batch that names
/// `last`: a watcher delivers a change after those made before it, so this
/// is everything before `last` too. `None` if `last` didn't come by the
/// deadline.
fn changes_through(
    last: &Path,
    mut next: impl FnMut(Duration) -> Option<Vec<FsChange>>,
) -> Option<HashSet<FsChange>> {
    let deadline = Instant::now() + TIMEOUT;
    let mut seen = HashSet::new();
    let wanted = FsChange::Path(last.to_path_buf());
    while !seen.contains(&wanted) {
        let left = deadline.checked_duration_since(Instant::now())?;
        seen.extend(next(left)?);
    }
    Some(seen)
}

#[test]
fn a_hosted_watch_delivers_the_watchers_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    write(&root, "src/main.rs", "fn main() {}\n");
    write(&root, "src/old.rs", "");
    write(&root, "src/gone.rs", "");
    let (tx, direct) = mpsc::channel();
    let _watcher = ProjectWatcher::new(&root, move |batch| {
        let _ = tx.send(batch);
    })
    .unwrap();
    let host: &dyn WatchHost = &LocalWatch;
    let hosted = host.watch(&root).unwrap();

    // A create, a rename and a removal, then one file written last, which
    // each watcher reports after the rest.
    fs::write(root.join("src/new.rs"), "").unwrap();
    fs::rename(root.join("src/old.rs"), root.join("src/moved.rs")).unwrap();
    fs::remove_file(root.join("src/gone.rs")).unwrap();
    let last = root.join("src/last.rs");
    fs::write(&last, "").unwrap();

    // Each watcher coalesces into batches as its own thread wakes, so the
    // batches can differ; the changes they hold don't.
    let from_watcher = changes_through(&last, |left| direct.recv_timeout(left).ok())
        .expect("the watcher saw the last file");
    let from_host = changes_through(&last, |left| hosted.recv_timeout(left).ok())
        .expect("the host saw the last file");
    assert_eq!(from_host, from_watcher);
    for rel in ["src/new.rs", "src/old.rs", "src/moved.rs", "src/gone.rs"] {
        let change = FsChange::Path(root.join(rel));
        assert!(from_host.contains(&change), "{change:?} in {from_host:?}");
    }
}

#[test]
fn a_hosted_watch_that_cant_start_fails_as_the_watcher_does() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    let direct = ProjectWatcher::new(&missing, |_| {});
    let host: &dyn WatchHost = &LocalWatch;
    let hosted = host.watch(&missing);
    assert_eq!(direct.is_err(), hosted.is_err());
    // inotify watches each directory itself, from the root down.
    #[cfg(target_os = "linux")]
    assert_eq!(
        hosted.err().map(|error| error.kind()),
        Some(std::io::ErrorKind::NotFound)
    );
}
