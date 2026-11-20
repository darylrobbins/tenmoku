//! A live watcher's report is dropped when a symlinked report folder's
//! target, or a folder a `coverage.report` glob matched, is removed or
//! replaced: those removals name folders the search no longer resolves.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use fe_project::FsChange;
use fe_workbench::coverage::CoverageSource;

fn lcov(file: &str) -> String {
    format!("SF:{file}\nDA:1,1\nend_of_record\n")
}

/// A source with a live watcher, loaded.
fn settle(source: &mut CoverageSource) {
    let start = Instant::now();
    loop {
        source.poll(Instant::now());
        if !source.is_loading() {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "load timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    (dir, root)
}

fn changed(source: &mut CoverageSource, path: &Path) {
    source.fs_changed(&[FsChange::Path(path.to_owned())]);
    settle(source);
}

/// Writes a report for `file` whose modification time is `secs` after the
/// epoch, so which is newest doesn't hang on the clock.
fn report(dir: &Path, file: &str, secs: u64) {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join("lcov.info");
    fs::write(&path, lcov(file)).unwrap();
    let time = std::time::UNIX_EPOCH + Duration::from_secs(secs);
    let file = fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(time).unwrap();
}

/// Whether the loaded report covers a file named `name`.
fn covers(source: &CoverageSource, name: &str) -> bool {
    let files = &source.report().expect("a report is loaded").files;
    files.keys().any(|path| path.ends_with(name))
}

#[cfg(unix)]
#[test]
fn default_discovery_drops_a_report_when_a_symlinked_folders_target_is_removed_or_replaced() {
    let (_dir, root) = project();
    let first = root.join("elsewhere-a");
    report(&first, "a.rs", 1_000);
    std::os::unix::fs::symlink(&first, root.join("coverage")).unwrap();

    let mut source = CoverageSource::new(&root, None);
    source.set_watching(true);
    settle(&mut source);
    assert_eq!(
        source.report_path(),
        Some(root.join("coverage/lcov.info").as_path())
    );
    assert!(covers(&source, "a.rs"));

    fs::remove_dir_all(&first).unwrap();
    changed(&mut source, &first);
    assert_eq!(source.report_path(), None);

    let second = root.join("elsewhere-b");
    report(&second, "b.rs", 2_000);
    fs::remove_file(root.join("coverage")).unwrap();
    std::os::unix::fs::symlink(&second, root.join("coverage")).unwrap();
    changed(&mut source, &root.join("coverage"));
    assert_eq!(
        source.report_path(),
        Some(root.join("coverage/lcov.info").as_path())
    );
    assert!(covers(&source, "b.rs"));
}

/// A symlink repointed at a report with the same modification time and
/// length: the spelled path is as it was, and only the content differs.
#[cfg(unix)]
#[test]
fn a_repointed_symlink_with_an_equal_stamp_loads_the_new_report() {
    let (_dir, root) = project();
    let (first, second) = (root.join("elsewhere-a"), root.join("elsewhere-b"));
    report(&first, "a.rs", 1_000);
    report(&second, "b.rs", 1_000);
    std::os::unix::fs::symlink(&first, root.join("coverage")).unwrap();

    let mut source = CoverageSource::new(&root, None);
    source.set_watching(true);
    settle(&mut source);
    assert!(covers(&source, "a.rs"));

    fs::remove_file(root.join("coverage")).unwrap();
    std::os::unix::fs::symlink(&second, root.join("coverage")).unwrap();
    changed(&mut source, &root.join("coverage"));
    assert_eq!(
        source.report_path(),
        Some(root.join("coverage/lcov.info").as_path())
    );
    assert!(covers(&source, "b.rs"));
    assert!(!covers(&source, "a.rs"));
}

#[test]
fn glob_discovery_drops_a_report_when_a_wildcard_folder_is_removed_or_replaced() {
    let (_dir, root) = project();
    let (one, two) = (root.join("runs/one"), root.join("runs/two"));
    report(&one, "a.rs", 1_000);
    report(&two, "b.rs", 2_000);

    let mut source = CoverageSource::new(&root, Some(PathBuf::from("runs/*/lcov.info")));
    source.set_watching(true);
    settle(&mut source);
    assert_eq!(source.report_path(), Some(two.join("lcov.info").as_path()));

    // The newest goes: the older one is picked, not none.
    fs::remove_dir_all(&two).unwrap();
    changed(&mut source, &two);
    assert_eq!(source.report_path(), Some(one.join("lcov.info").as_path()));
    assert!(covers(&source, "a.rs"));

    // Separately, a new folder with a newer report wins over the older.
    let three = root.join("runs/three");
    report(&three, "c.rs", 3_000);
    changed(&mut source, &three);
    assert_eq!(
        source.report_path(),
        Some(three.join("lcov.info").as_path())
    );
    assert!(covers(&source, "c.rs"));

    // The last folders go: no report.
    fs::remove_dir_all(&three).unwrap();
    changed(&mut source, &three);
    fs::remove_dir_all(&one).unwrap();
    changed(&mut source, &one);
    assert_eq!(source.report_path(), None);

    // A wildcard's match that is a symlink: its target is what a watcher
    // names when it goes.
    #[cfg(unix)]
    {
        let (_dir, root) = project();
        let (target, other) = (root.join("store-a"), root.join("store-b"));
        report(&target, "a.rs", 2_000);
        report(&other, "b.rs", 1_000);
        fs::create_dir(root.join("runs")).unwrap();
        std::os::unix::fs::symlink(&target, root.join("runs/link")).unwrap();
        std::os::unix::fs::symlink(&other, root.join("runs/spare")).unwrap();

        let mut source = CoverageSource::new(&root, Some(PathBuf::from("runs/*/lcov.info")));
        source.set_watching(true);
        settle(&mut source);
        assert_eq!(
            source.report_path(),
            Some(root.join("runs/link/lcov.info").as_path())
        );

        // The newest's target goes: the older one is picked, not none.
        fs::remove_dir_all(&target).unwrap();
        changed(&mut source, &target);
        assert_eq!(
            source.report_path(),
            Some(root.join("runs/spare/lcov.info").as_path())
        );
        assert!(covers(&source, "b.rs"));

        // A newer report behind a replaced link wins.
        let newer = root.join("store-c");
        report(&newer, "c.rs", 3_000);
        fs::remove_file(root.join("runs/link")).unwrap();
        std::os::unix::fs::symlink(&newer, root.join("runs/link2")).unwrap();
        changed(&mut source, &root.join("runs/link2"));
        assert_eq!(
            source.report_path(),
            Some(root.join("runs/link2/lcov.info").as_path())
        );
        assert!(covers(&source, "c.rs"));

        // And with every target gone: no report.
        fs::remove_dir_all(&newer).unwrap();
        changed(&mut source, &newer);
        fs::remove_dir_all(&other).unwrap();
        changed(&mut source, &other);
        assert_eq!(source.report_path(), None);
    }
}
