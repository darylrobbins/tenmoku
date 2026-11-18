//! The What's New marker: which version's notes this machine has seen.

use std::fs;

use fe_config::whats_new::{MARKER_FILE, WhatsNew, decide, parse_version, record};
use tempfile::TempDir;

fn marker(state: &TempDir) -> String {
    fs::read_to_string(state.path().join(MARKER_FILE)).unwrap()
}

#[test]
fn missing_marker_shows() {
    let state = TempDir::new().unwrap();
    assert_eq!(decide(state.path(), "0.2.3"), WhatsNew::Show);
    // Nor a state directory.
    let gone = state.path().join("gone");
    assert_eq!(decide(&gone, "0.2.3"), WhatsNew::Show);
    assert!(!gone.exists());
}

#[test]
fn unreadable_marker_shows() {
    let cases = [
        "this is [not toml",
        "",
        "seen = 3\n",
        "other = \"0.2.2\"\n",
        "seen = \"0.2\"\n",
        "seen = \"v0.2.2\"\n",
        "seen = \"0.02.2\"\n",
    ];
    for text in cases {
        let state = TempDir::new().unwrap();
        fs::write(state.path().join(MARKER_FILE), text).unwrap();
        assert_eq!(decide(state.path(), "0.2.3"), WhatsNew::Show, "{text:?}");
        assert_eq!(marker(&state), text, "decide writes nothing");
    }
    // One that isn't a file.
    let state = TempDir::new().unwrap();
    fs::create_dir(state.path().join(MARKER_FILE)).unwrap();
    assert_eq!(decide(state.path(), "0.2.3"), WhatsNew::Show);
}

#[test]
fn older_marker_shows() {
    for seen in ["0.2.2", "0.1.9", "0.0.0", "0.2.0"] {
        let state = TempDir::new().unwrap();
        record(state.path(), seen).unwrap();
        assert_eq!(decide(state.path(), "0.2.3"), WhatsNew::Show, "{seen}");
    }
    // As numbers, not text.
    let state = TempDir::new().unwrap();
    record(state.path(), "0.2.9").unwrap();
    assert_eq!(decide(state.path(), "0.2.10"), WhatsNew::Show);
}

#[test]
fn equal_or_newer_marker_is_seen() {
    for seen in ["0.2.3", "0.2.4", "0.3.0", "1.0.0", "0.2.10"] {
        let state = TempDir::new().unwrap();
        record(state.path(), seen).unwrap();
        let before = marker(&state);
        assert_eq!(decide(state.path(), "0.2.3"), WhatsNew::Seen, "{seen}");
        assert_eq!(marker(&state), before);
    }
}

#[test]
fn versions_parse_as_update_check_parses_them() {
    assert_eq!(parse_version("0.2.3"), Some((0, 2, 3)));
    assert_eq!(parse_version("10.0.12"), Some((10, 0, 12)));
    for text in [
        "", "0.2", "0.2.3.4", "v0.2.3", "0.2.03", "0.-2.3", "0.2.x", "0..3",
    ] {
        assert_eq!(parse_version(text), None, "{text:?}");
    }
}

#[test]
fn record_writes_atomically_and_replaces() {
    let dir = TempDir::new().unwrap();
    // It makes the state directory.
    let state = dir.path().join("state");
    record(&state, "0.2.2").unwrap();
    assert_eq!(
        fs::read_to_string(state.join(MARKER_FILE)).unwrap(),
        "seen = \"0.2.2\"\n"
    );
    record(&state, "0.2.3").unwrap();
    assert_eq!(
        fs::read_to_string(state.join(MARKER_FILE)).unwrap(),
        "seen = \"0.2.3\"\n"
    );
    assert_eq!(decide(&state, "0.2.3"), WhatsNew::Seen);
    // Nothing but the marker is left behind: no temp file.
    let names: Vec<_> = fs::read_dir(&state)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, [MARKER_FILE]);
}

#[cfg(unix)]
#[test]
fn record_does_not_follow_a_planted_temp_file() {
    // A file already at a temp name is skipped, not written through.
    let state = TempDir::new().unwrap();
    let victim = state.path().join("victim");
    fs::write(&victim, "keep").unwrap();
    for n in 0..4 {
        let tmp = format!("{MARKER_FILE}.{}.{n}.tmp", std::process::id());
        std::os::unix::fs::symlink(&victim, state.path().join(tmp)).unwrap();
    }
    record(state.path(), "0.2.3").unwrap();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "keep");
    assert_eq!(marker(&state), "seen = \"0.2.3\"\n");
}

#[cfg(unix)]
#[test]
fn record_fails_in_a_read_only_state_dir() {
    use std::os::unix::fs::PermissionsExt as _;

    let state = TempDir::new().unwrap();
    record(state.path(), "0.2.2").unwrap();
    fs::set_permissions(state.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let result = record(state.path(), "0.2.3");
    fs::set_permissions(state.path(), fs::Permissions::from_mode(0o755)).unwrap();
    // Root ignores the mode; anyone else can't write.
    if result.is_err() {
        assert_eq!(marker(&state), "seen = \"0.2.2\"\n");
    }
}
