//! fe-vim's remaps against recorded Neovim: `tests/corpus/remaps.toml` and
//! its expectations, which the ignored recorder in `tests/oracle.rs`
//! writes. Each case's `mapping` lines run in nvim and, read by fe-vim's
//! vimrc parser, here (docs/decisions/vim-remaps.md §8).

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn remaps_match_neovim() {
    let recorded = load("remaps").unwrap();
    assert!(recorded.len() >= 24, "remaps: {} cases", recorded.len());
    // What the card asks the corpus to cover, by case name.
    for name in [
        "remap_Y",
        "remap_H",
        "remap_count_H",
        "remap_dH_onoremap",
        "remap_dH_nnoremap_only",
        "remap_visual_x",
        "remap_dot",
        "remap_macro_record_play",
        "remap_recursive",
        "remap_noremap",
        "remap_self_prefix",
        "remap_self_prefix_more",
        "remap_failed_prefix_suffix_mapped",
        "remap_cycle_e223",
        "remap_leader",
        "remap_normal",
        "remap_normal_bang",
        "remap_unmap",
        "remap_ex_line_in_to",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "remaps: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "remaps: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
