//! fe-vim's insert, replace and command-line remaps against recorded Neovim:
//! `tests/corpus/remaps_insert.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes. Each case's `mapping` lines run in
//! nvim and, read by fe-vim's vimrc parser, here.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn insert_remaps_match_neovim() {
    let recorded = load("remaps_insert").unwrap();
    assert!(
        recorded.len() >= 12,
        "remaps_insert: {} cases",
        recorded.len()
    );
    for name in [
        "remap_insert_esc",
        "remap_insert_expansion",
        "remap_insert_imap_chain",
        "remap_insert_inoremap_no_chain",
        "remap_insert_failed_prefix",
        "remap_replace_mode",
        "remap_nmap_then_imap",
        "remap_insert_normal_cmd",
        "remap_insert_normal_bang_cmd",
        "remap_cmdline_cr",
        "remap_bang_map_and_unmap",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "remaps_insert: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "remaps_insert: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
