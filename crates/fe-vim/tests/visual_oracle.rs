//! fe-vim's visual modes against recorded Neovim:
//! `tests/corpus/visual.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use std::process::Command;

use support::{check, load};

#[test]
fn visual_matches_neovim() {
    let recorded = load("visual").unwrap();
    assert!(recorded.len() >= 40, "visual: {} cases", recorded.len());
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "visual: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

/// Rewrites `src/lower.txt` and `src/upper.txt`, what nvim's visual `u` and
/// `U` make of every code point they change, which fe-vim's `u` and `U` over
/// a selection read:
/// `cargo nextest run -p fe-vim --run-ignored only -E 'test(=record_lower_upper_tables_with_neovim)'`.
#[test]
#[ignore = "needs nvim 0.12.4; rewrites src/lower.txt and src/upper.txt"]
fn record_lower_upper_tables_with_neovim() {
    let version = Command::new("nvim")
        .arg("--version")
        .output()
        .expect("run nvim");
    let version = String::from_utf8_lossy(&version.stdout);
    assert_eq!(
        version.lines().next().unwrap_or_default(),
        format!("NVIM v{}", support::NVIM),
        "the oracle is pinned to nvim {}",
        support::NVIM
    );
    let dir = support::crate_dir();
    let status = Command::new("nvim")
        .args(["--clean", "--headless", "-l"])
        .arg(dir.join("tests/support/lower_upper.lua"))
        .arg(dir.join("src/lower.txt"))
        .arg(dir.join("src/upper.txt"))
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("run nvim");
    assert!(status.success(), "nvim exited with {status}");
}
