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
    // Each session is timed against the same count of copies typed without
    // a `<BS>` to replay. The count is large enough that a per-character
    // history, or replaying the count one copy at a time, takes many times
    // longer, and small enough to be cheap in a debug build; the cap on
    // the largest count is `counted_replace_of_regional_indicators_caps_at_max_put`.
    // Load comes and goes mid-run, so while a session is over the bound it
    // is timed again, up to ROUNDS times in all, against the baseline,
    // which is measured once.
    const ROUNDS: usize = 3;
    let count = 1u32 << 18;
    let timed = |keys: &str| {
        let started = Instant::now();
        let text = fed("abcdefghijklmno", keys);
        (started.elapsed(), text)
    };
    let baseline = format!("{count}R🇦🇧<Esc>");
    let plain = timed(&baseline).0;
    for (keys, starts) in [
        ("R🇦<CR><BS>🇧<BS>🇨<Esc>", "🇨🇦🇨🇦🇨"),
        ("R🇦🇧🇨<CR><BS><BS>🇦<Esc>", "🇦🇧🇦🇦🇦🇦🇦"),
        // Each copy leaves one indicator, paired by the next copy's.
        ("R🇦<CR><BS><Esc>", "🇦🇦🇦🇦🇦"),
    ] {
        for keys in [format!("{count}{keys}"), format!("{keys}0{count}.")] {
            let bound =
                |plain: Duration| (plain * 4 + Duration::from_secs(1)).min(Duration::from_secs(60));
            let mut took = Duration::MAX;
            for _ in 0..ROUNDS {
                let (t, text) = timed(&keys);
                took = took.min(t);
                assert!(text.starts_with(starts), "{keys}");
                if took < bound(plain) {
                    break;
                }
            }
            assert!(
                took < bound(plain),
                "{keys}: {took:?}, {plain:?} without a `<BS>`"
            );
        }
    }
}

/// A count above what a put may insert is clamped to it: the largest count
/// writes no more than 64 MiB, and still writes more than a mid-size one.
/// No clock; the timing is `..._stays_bounded`'s.
#[test]
fn counted_replace_of_regional_indicators_caps_at_max_put() {
    let text = fed(
        "abcdefghijklmno",
        &format!("{}R🇦<CR><BS>🇧<BS>🇨<Esc>", u32::MAX),
    );
    assert!(text.len() <= 65 << 20, "{} bytes", text.len());
    assert!(text.len() > 1 << 20, "{} bytes", text.len());
}
