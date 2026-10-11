//! A counted replace session whose copies pair regional indicators with
//! what's before them on the row, as Neovim 0.12.4 does: a `<BS>` in a copy
//! takes out the grapheme that the indicators before it put its characters
//! in, but only what the copy typed of it. Through fe-vim's public API
//! alone, as `tests/host.rs` drives it.

use std::time::{Duration, Instant};

use fe_vim::HeadlessVim;

/// `buffer` with the caret at its start, once `keys` are fed.
fn fed(buffer: &str, keys: &str) -> String {
    let mut vim = HeadlessVim::new(buffer, (0, 0));
    vim.feed(keys).unwrap();
    vim.text()
}

#[test]
fn counted_replace_pairs_regional_indicators_after_a_line_break() {
    // The session leaves a lone 🇨 before the caret, which the next copy's
    // 🇦 pairs with: its `<BS>` takes 🇧 alone.
    let keys = "R🇦<CR><BS>🇧<BS>🇨<Esc>";
    assert_eq!(fed("abcdefghijk", &format!("2{keys}")), "🇨🇦🇨efghijk");
    // A counted dot-repeat on a fresh row, with the session's count and
    // with a count of its own.
    let row = "abcdefghijk";
    assert_eq!(
        fed(&format!("{row}\n{row}"), &format!("2{keys}j0.")),
        "🇨🇦🇨efghijk\n🇨🇦🇨efghijk"
    );
    assert_eq!(
        fed(&format!("{row}\n{row}"), &format!("{keys}j02.")),
        "🇨cdefghijk\n🇨🇦🇨efghijk"
    );
}

#[test]
fn counted_replace_pairs_regional_indicators_with_an_empty_tail() {
    // The copy's 🇦 pairs with the session's last 🇦, so its 🇧🇨 pair and
    // the `<BS>` after the line break takes both.
    let keys = "R🇦🇧🇨<CR><BS><BS>🇦<Esc>";
    assert_eq!(
        fed("abcdefghijklmno", &format!("2{keys}")),
        "🇦🇧🇦🇦🇦ghijklmno"
    );
    let row = "abcdefghijklmno";
    assert_eq!(
        fed(&format!("{row}\n{row}"), &format!("2{keys}j0.")),
        "🇦🇧🇦🇦🇦ghijklmno\n🇦🇧🇦🇦🇦ghijklmno"
    );
    assert_eq!(
        fed(&format!("{row}\n{row}"), &format!("{keys}j02.")),
        "🇦🇧🇦defghijklmno\n🇦🇧🇦🇦🇦ghijklmno"
    );
}

#[test]
fn counted_replace_of_regional_indicators_stays_bounded() {
    // Every count this large writes as much as a put may insert, which
    // takes a while in a debug build and longer under load: each session
    // is timed against copies typed without a `<BS>` to replay, writing as
    // much, just before it. Replaying the count one copy at a time would
    // take many times longer.
    let count = u32::MAX;
    let timed = |keys: &str| {
        let started = Instant::now();
        let text = fed("abcdefghijklmno", keys);
        (started.elapsed(), text)
    };
    for (keys, starts) in [
        ("R🇦<CR><BS>🇧<BS>🇨<Esc>", "🇨🇦🇨🇦🇨"),
        ("R🇦🇧🇨<CR><BS><BS>🇦<Esc>", "🇦🇧🇦🇦🇦🇦🇦"),
        // Each copy leaves one indicator, paired by the next copy's.
        ("R🇦<CR><BS><Esc>", "🇦🇦🇦🇦🇦"),
    ] {
        let (plain, _) = timed(&format!("{count}R🇦🇧<Esc>"));
        for keys in [format!("{count}{keys}"), format!("{keys}0{count}.")] {
            let (took, text) = timed(&keys);
            assert!(
                took < (plain * 4 + Duration::from_secs(1)).min(Duration::from_secs(60)),
                "{keys}: {took:?}, {plain:?} without a `<BS>`"
            );
            assert!(text.starts_with(starts), "{keys}");
        }
    }
}
