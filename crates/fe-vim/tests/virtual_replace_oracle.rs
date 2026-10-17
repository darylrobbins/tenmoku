//! fe-vim's virtual replace mode (`gR`) against recorded Neovim:
//! `tests/corpus/virtual_replace.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn virtual_replace_matches_neovim() {
    let recorded = load("virtual_replace").unwrap();
    assert!(
        recorded.len() >= 16,
        "virtual_replace: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "tab_kept",
        "tab_consumed",
        "typed_tab",
        "over_wide",
        "past_end",
        "bs_puts_back_tab",
        "bs_puts_back",
        "count",
        "tab_dot_other_column",
        "count_bs_wide",
        "tab_bs",
        "count_bs_combining",
        "dot",
        "undo",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "virtual_replace: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "virtual_replace: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

/// The largest count a put may insert, typed in a virtual session, keeps
/// no history per character: it takes about as long as the same count of
/// `R`'s copies, not many times more.
#[test]
fn a_huge_virtual_count_stays_bounded() {
    use std::time::{Duration, Instant};

    use fe_vim::HeadlessVim;

    let run = |keys: &str| {
        let started = Instant::now();
        let mut vim = HeadlessVim::new("abcdefghijklmno", (0, 0));
        vim.feed(keys).unwrap();
        (started.elapsed(), vim.text())
    };
    let (plain, _) = run("4294967295Rx<Esc>");
    let keys = "4294967295gRx<BS>y<Esc>";
    let (took, text) = run(keys);
    assert!(text.starts_with("yyyy"), "{keys}");
    assert!(
        took < (plain * 20 + Duration::from_secs(5)).min(Duration::from_secs(90)),
        "{keys}: {took:?}, {plain:?} for R"
    );
}
