//! `<CR>` in fe-vim's virtual replace mode (`gR`) against recorded Neovim:
//! `tests/corpus/virtual_replace_breaks.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn virtual_replace_breaks_match_neovim() {
    let recorded = load("virtual_replace_breaks").unwrap();
    assert!(
        recorded.len() >= 10,
        "virtual_replace_breaks: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "rest_of_row",
        "last_row_adds",
        "last_row_adds_twice",
        "next_row_indent",
        "bs_over_break",
        "count",
        "dot",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "virtual_replace_breaks: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "virtual_replace_breaks: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

/// The largest counts stay within the 64 MiB a put may insert, counted in
/// the bytes typed tabs expand to, and break rows in a time bounded against
/// the same count of `R`'s copies run beside it, so load can't cross it.
#[test]
fn huge_virtual_counts_stay_bounded() {
    use std::time::{Duration, Instant};

    use fe_vim::HeadlessVim;

    let run = |keys: &str| {
        let started = Instant::now();
        let mut vim = HeadlessVim::new("abcdefghijklmno\npq", (0, 0));
        vim.feed(keys).unwrap();
        (started.elapsed(), vim.text())
    };
    let (plain, _) = run("4294967295Rx<Esc>");
    for keys in ["4294967295gR<Tab><Esc>", "4294967295gRx<CR>y<Esc>"] {
        let (took, text) = run(keys);
        assert!(text.len() <= 65 << 20, "{keys}: {} bytes", text.len());
        assert!(text.len() > 1 << 20, "{keys}: {} bytes", text.len());
        assert!(
            took < plain * 20 + Duration::from_secs(5),
            "{keys}: {took:?}, {plain:?} for R"
        );
    }
}
