//! The command history behind the live prompt's suggestion.

use fe_terminal::history::{CAP, History};

#[test]
fn newest_match_by_prefix() {
    let history = History::new();
    history.record("git status");
    history.record("git stash");
    history.record("ls");
    assert_eq!(history.newest_match("git st").as_deref(), Some("git stash"));
    assert_eq!(history.remainder("git sta").as_deref(), Some("sh"));
    history.record("git stage .");
    assert_eq!(history.remainder("git sta").as_deref(), Some("ge ."));
    // Longer is required, and nothing typed suggests nothing.
    assert_eq!(history.newest_match("ls"), None);
    assert_eq!(history.newest_match(""), None);
    assert_eq!(history.newest_match("cargo"), None);
}

#[test]
fn duplicates_collapse() {
    let history = History::new();
    history.record("a one");
    history.record("b");
    history.record("a one");
    assert_eq!(history.lines(), ["a one", "b"]);
}

#[test]
fn lines_starting_with_a_space_are_skipped() {
    let history = History::new();
    history.record(" secret --token x");
    history.record("");
    history.record("   ");
    history.record("one\ntwo");
    assert!(history.lines().is_empty());
}

#[test]
fn the_cap_of_2000_drops_the_oldest() {
    let history = History::new();
    for n in 0..CAP + 5 {
        history.record(&format!("cmd {n}"));
    }
    let lines = history.lines();
    assert_eq!(CAP, 2000);
    assert_eq!(lines.len(), 2000);
    assert_eq!(lines[0], format!("cmd {}", CAP + 4));
    assert_eq!(lines[CAP - 1], "cmd 5");
}

#[test]
fn nothing_is_written_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let before = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let history = History::new();
    history.record("echo hunter2");
    std::env::set_current_dir(before).unwrap();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    // The type holds nothing but its lines: it has no path to write to.
    assert_eq!(history.lines(), ["echo hunter2"]);
}
