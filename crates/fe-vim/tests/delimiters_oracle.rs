//! fe-vim's delimiter objects `i[ a[ i< a< i` a` it at`, against recorded
//! Neovim: `tests/corpus/delimiters.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn delimiters_matches_neovim() {
    // The oracle checks results, not the time limit: a tag-name pattern runs on a
    // wall-clock deadline, and one that passes it (a stalled thread under load)
    // leaves the buffer untouched, which reads as a missing text object.
    fe_vim::search::set_time_limit(std::time::Duration::from_secs(60));
    let recorded = load("delimiters").unwrap();
    assert!(recorded.len() >= 40, "delimiters: {} cases", recorded.len());
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "delimiters: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
