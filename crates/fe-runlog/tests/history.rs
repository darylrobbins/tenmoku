//! Test history: windows, flakiness, persistence. Every test passes its own
//! directory under the target's tmp dir; nothing reads `$HOME`.

use std::path::{Path, PathBuf};

use fe_runlog::{Flaky, History, Outcome, TestId};

use Outcome::{Failed, Passed};

fn dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("history")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn t(name: &str) -> TestId {
    TestId::new("bin", name)
}

#[test]
fn last8_keeps_the_newest_eight_oldest_first() {
    let mut h = History::in_memory();
    for i in 0..10 {
        h.record(&[(t("a"), if i == 0 { Failed } else { Passed })])
            .unwrap();
    }
    assert_eq!(h.last8(&t("a")), vec![Passed; 8]);
    h.record(&[(t("a"), Failed), (t("a"), Failed)]).unwrap();
    let w = h.last8(&t("a"));
    assert_eq!(w.len(), 8);
    assert_eq!(w[7], Failed);
    assert_eq!(w[6], Passed, "a test listed twice in one run counts once");
    assert!(h.last8(&t("nope")).is_empty());
}

#[test]
fn flaky_needs_a_pass_and_a_fail_in_the_window() {
    let mut h = History::in_memory();
    h.record(&[(t("p"), Passed), (t("f"), Failed), (t("m"), Failed)])
        .unwrap();
    h.record(&[(t("p"), Passed), (t("f"), Failed), (t("m"), Passed)])
        .unwrap();
    assert_eq!(h.flaky(&t("p")), None);
    assert_eq!(h.flaky(&t("f")), None);
    assert_eq!(h.flaky(&t("m")), Some(Flaky { failed: 1, of: 2 }));
    assert_eq!(h.flaky(&t("unknown")), None);
    // The fail ages out of the window.
    for _ in 0..8 {
        h.record(&[(t("m"), Passed)]).unwrap();
    }
    assert_eq!(h.flaky(&t("m")), None);
}

#[test]
fn a_test_missing_from_a_run_keeps_its_window() {
    let mut h = History::in_memory();
    h.record(&[(t("a"), Passed), (t("b"), Failed)]).unwrap();
    h.record(&[(t("a"), Passed)]).unwrap();
    assert_eq!(h.last8(&t("b")), vec![Failed]);
    assert_eq!(h.last8(&t("a")), vec![Passed, Passed]);
}

#[test]
fn save_is_atomic_and_load_round_trips() {
    let state = dir("roundtrip");
    let root = Path::new("/work/project");
    let mut h = History::load(&state, root);
    assert!(h.is_empty());
    h.record(&[(t("a"), Passed), (t("b"), Failed)]).unwrap();
    h.record(&[(t("a"), Failed)]).unwrap();

    let files: Vec<_> = std::fs::read_dir(state.join("test-history"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files.len(), 1, "no temp file left behind: {files:?}");
    assert!(files[0].ends_with(".json"));

    let again = History::load(&state, root);
    assert_eq!(again.last8(&t("a")), vec![Passed, Failed]);
    assert_eq!(again.last8(&t("b")), vec![Failed]);
    assert_eq!(again.flaky(&t("a")), Some(Flaky { failed: 1, of: 2 }));

    let other = History::load(&state, Path::new("/work/other"));
    assert!(other.is_empty(), "each root has its own file");
}

#[test]
fn a_corrupt_file_loads_empty() {
    let state = dir("corrupt");
    let root = Path::new("/r");
    let mut h = History::load(&state, root);
    h.record(&[(t("a"), Passed)]).unwrap();
    let file = std::fs::read_dir(state.join("test-history"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&file, "{not json").unwrap();
    let mut h = History::load(&state, root);
    assert!(h.is_empty());
    h.record(&[(t("b"), Passed)]).unwrap();
    assert_eq!(History::load(&state, root).last8(&t("b")), vec![Passed]);
}

#[test]
fn the_cap_of_20000_drops_the_least_recently_run() {
    let mut h = History::in_memory();
    let first: Vec<_> = (0..20_000).map(|i| (t(&format!("t{i}")), Passed)).collect();
    h.record(&first).unwrap();
    // t0 runs again, so t1 is now the least recently run.
    h.record(&[(t("t0"), Passed), (t("new"), Failed)]).unwrap();
    assert_eq!(h.len(), 20_000);
    assert!(h.last8(&t("t1")).is_empty());
    assert_eq!(h.last8(&t("t0")).len(), 2);
    assert_eq!(h.last8(&t("new")), vec![Failed]);
    assert_eq!(h.last8(&t("t2")).len(), 1);
}

fn only_file(state: &Path) -> PathBuf {
    std::fs::read_dir(state.join("test-history"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
}

#[test]
fn an_oversized_or_inconsistent_file_loads_empty() {
    let state = dir("invalid");
    let root = Path::new("/r");
    History::load(&state, root)
        .record(&[(t("a"), Passed)])
        .unwrap();
    let file = only_file(&state);
    let tests: Vec<String> = (0..20_001)
        .map(|i| format!(r#"{{"binary":"b","name":"t{i}","results":"P","seq":1}}"#))
        .collect();
    std::fs::write(
        &file,
        format!(r#"{{"seq":1,"tests":[{}]}}"#, tests.join(",")),
    )
    .unwrap();
    assert!(History::load(&state, root).is_empty());
    let max = u64::MAX;
    std::fs::write(&file, format!(r#"{{"seq":{max},"tests":[]}}"#)).unwrap();
    assert!(History::load(&state, root).is_empty());
    std::fs::write(
        &file,
        r#"{"seq":1,"tests":[{"binary":"b","name":"a","results":"P","seq":9}]}"#,
    )
    .unwrap();
    assert!(History::load(&state, root).is_empty());
}

#[cfg(unix)]
#[test]
fn non_utf8_roots_have_their_own_files() {
    use std::os::unix::ffi::OsStrExt;
    let state = dir("non-utf8");
    let a = Path::new(std::ffi::OsStr::from_bytes(b"/r\xff"));
    let b = Path::new(std::ffi::OsStr::from_bytes(b"/r\xfe"));
    History::load(&state, a)
        .record(&[(t("a"), Passed)])
        .unwrap();
    assert!(History::load(&state, b).is_empty());
}

#[test]
fn concurrent_saves_leave_a_valid_file() {
    let state = dir("concurrent");
    let root = Path::new("/r");
    let threads: Vec<_> = (0..8)
        .map(|n| {
            let state = state.clone();
            std::thread::spawn(move || {
                let mut h = History::load(&state, Path::new("/r"));
                for _ in 0..20 {
                    h.record(&[(t(&format!("t{n}")), Passed)]).unwrap();
                }
            })
        })
        .collect();
    for th in threads {
        th.join().unwrap();
    }
    assert!(!History::load(&state, root).is_empty());
    assert_eq!(
        std::fs::read_dir(state.join("test-history"))
            .unwrap()
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn a_planted_symlink_is_never_followed() {
    let state = dir("symlink");
    let root = Path::new("/r");
    let victim = state.join("victim");
    std::fs::write(&victim, "keep").unwrap();
    let mut h = History::load(&state, root);
    h.record(&[(t("a"), Passed)]).unwrap();
    let file = only_file(&state);
    std::os::unix::fs::symlink(&victim, file.with_extension("json.tmp")).unwrap();
    h.record(&[(t("a"), Failed)]).unwrap();
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
}
