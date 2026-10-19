//! A yank records its ranges and a new serial for a host to flash; a delete
//! or change leaves the last yank alone.

#![allow(clippy::single_range_in_vec_init)]

use fe_vim::HeadlessVim;

fn last(vim: &HeadlessVim) -> Option<(u64, Vec<std::ops::Range<usize>>)> {
    vim.state()
        .last_yank()
        .map(|(serial, ranges)| (serial, ranges.to_vec()))
}

/// Feeds `keys` to a vim on `buffer` at `at`, and returns the last yank.
fn after(buffer: &str, at: (u32, usize), keys: &str) -> Option<(u64, Vec<std::ops::Range<usize>>)> {
    let mut vim = HeadlessVim::new(buffer, at);
    vim.feed(keys).unwrap();
    last(&vim)
}

#[test]
fn yank_ranges_are_recorded() {
    let text = "one two\nthree four\nfive";
    assert_eq!(after(text, (0, 4), "yiw"), Some((1, vec![4..7])));
    assert_eq!(after(text, (1, 0), "yy"), Some((1, vec![8..19])));
    assert_eq!(after(text, (0, 0), "2yy"), Some((1, vec![0..19])));
    assert_eq!(after(text, (0, 4), "viwy"), Some((1, vec![4..7])));
    assert_eq!(after(text, (0, 4), "\"ayy"), Some((1, vec![0..8])));
    assert_eq!(
        after(text, (0, 1), "<C-v>jly"),
        Some((1, vec![1..3, 9..11]))
    );

    // A later yank takes the next serial; a delete or change keeps it.
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.feed("yiw").unwrap();
    vim.feed("w").unwrap();
    vim.feed("yiw").unwrap();
    assert_eq!(last(&vim), Some((2, vec![4..7])));
    for keys in ["dd", "x", "cwz<Esc>"] {
        vim.feed(keys).unwrap();
        assert_eq!(last(&vim).map(|(serial, _)| serial), Some(2), "{keys}");
    }
    let fresh = HeadlessVim::new(text, (0, 0));
    assert_eq!(last(&fresh), None);
}
