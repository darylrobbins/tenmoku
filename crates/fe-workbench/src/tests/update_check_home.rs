//! Check for Updates asks the repository's current home (T409): the old
//! name's API URL answers 301 and `fe-net` follows no redirects.

#[test]
fn the_update_check_asks_the_current_repository() {
    let source = include_str!("../update_check.rs");
    let line = |name: &str| {
        source
            .lines()
            .find(|line| line.starts_with(&format!("const {name}:")))
            .unwrap_or_else(|| panic!("no const {name}"))
            .to_owned()
    };
    assert!(
        line("LATEST_RELEASE")
            .contains("\"https://api.github.com/repos/tenmoku-editor/tenmoku/releases/latest\"")
    );
    assert!(
        line("RELEASE_PAGES").contains("\"https://github.com/tenmoku-editor/tenmoku/releases/\"")
    );
}
