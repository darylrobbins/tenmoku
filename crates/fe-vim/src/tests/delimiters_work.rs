//! Tests of what the clock, the pattern names and the passes over the text
//! cost, which read thread-locals of `src/delimiters.rs`.
use super::*;
use crate::HeadlessVim;

/// The text, caret and unnamed register after `keys` on `text` with the
/// caret at `cursor`.
fn after(text: &str, cursor: (u32, usize), keys: &str) -> (String, (u32, usize), String) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    let register = vim
        .registers()
        .get('"')
        .map(|register| register.text.clone())
        .unwrap_or_default();
    (vim.text(), vim.cursor(), register)
}

/// Runs `keys` on `text` (caret at `column`) once to see the phases of the
/// command and the looks each takes, then again with the command clock
/// running out at the first, a middle and the last look of each phase named
/// in `phases` (all must occur): each time the object fails with nothing
/// changed. In every run, no pass has taken more than a few stretches of
/// steps ([`WORK`]) between two looks (each pass counts the steps it
/// really takes).
fn expire_in_every_phase(text: &str, column: usize, keys: &str, taken: &str, phases: &[&str]) {
    let run = |look: Option<usize>| {
        LOOKS.set(look);
        LOOKED.set(0);
        EXPIRED.set(false);
        MOST_STEPS.set(0);
        SINCE.set(0);
        PHASE.set("");
        LOG.with_borrow_mut(Vec::clear);
        RECORD.set(true);
        let result = after(text, (0, column), keys);
        RECORD.set(false);
        LOOKS.set(None);
        result
    };
    let bound = |keys: &str, look: Option<usize>| {
        assert!(
            MOST_STEPS.get() <= WORK,
            "{keys} at look {look:?}: {} steps between looks",
            MOST_STEPS.get()
        );
    };
    let (_, _, register) = run(None);
    assert_eq!(register, taken, "{keys}");
    bound(keys, None);
    let log = LOG.with_borrow(Clone::clone);
    for &phase in phases {
        let at: Vec<usize> = (0..log.len()).filter(|&ix| log[ix] == phase).collect();
        assert!(!at.is_empty(), "{keys}: no look in phase {phase}: {log:?}");
        for look in [at[0], at[at.len() / 2], at[at.len() - 1]] {
            let (got, _, register) = run(Some(look));
            assert!(EXPIRED.get(), "{keys}: look {look} of {}", log.len());
            assert_eq!(
                (got.as_str(), register.as_str()),
                (text, ""),
                "{keys} in {phase} at look {look}"
            );
            bound(keys, Some(look));
        }
    }
}

#[test]
fn every_pass_looks_at_the_clock() {
    // Names read and compared: ASCII and not, composed (paired by a pass of
    // their own), and the table of places a name can start.
    let long = "b".repeat(20_000);
    expire_in_every_phase(
        &format!("<{long}>x</{long}>"),
        long.len() + 2,
        "dit",
        "x",
        &["gather", "row", "scan", "pair", "inner"],
    );
    let wide = "é".repeat(10_000);
    expire_in_every_phase(
        &format!("<{wide}>x</{wide}>"),
        wide.len() + 2,
        "dit",
        "x",
        &["scan", "pair"],
    );
    let composed = "e\u{301}".repeat(6_000);
    expire_in_every_phase(
        &format!("<{composed}>x</{composed}>"),
        composed.len() + 2,
        "dit",
        "x",
        &["scan", "partner", "inner"],
    );
    let run = "<".repeat(20_000);
    expire_in_every_phase(
        &format!("<a>x{run}></a>"),
        3,
        "dit",
        &format!("x{run}>"),
        &["scan"],
    );
    // The pass back over unnested tags, and a count passing over openings.
    let many = "<a->".repeat(4_000);
    expire_in_every_phase(&format!("{many}x</a->"), many.len(), "yit", "x", &["pair"]);
    let open = "<q>".repeat(4_000);
    expire_in_every_phase(
        &format!("<a>{open}x</a>"),
        3 + open.len(),
        "1000yit",
        &format!("{open}x"),
        &["count"],
    );
    // A pattern name, prepared and paired, and an expensive one to prepare.
    let many = "<a.b>".repeat(300);
    expire_in_every_phase(
        &format!("<b>{many}x</axb></b>"),
        3 + many.len(),
        "yit",
        "x",
        &["prepare", "heads", "pairing"],
    );
    let name = format!("a.{}", "b".repeat(300));
    expire_in_every_phase(
        &format!("<{name}>x</axb{}>", "b".repeat(299)),
        0,
        "dit",
        "x",
        &["prepare", "heads", "pairing"],
    );
    expire_in_every_phase(
        "<a\u{200d}.\\k>x</a\u{200d}ba>",
        0,
        "dit",
        "x",
        &["prepare", "heads", "pairing"],
    );
}

#[test]
fn a_name_that_spells_out_to_too_much_fails_in_bounded_room() {
    // 3,000 `\k` atoms are collections of a few KB each: past what a name
    // may spell out to, which fails the object before the pattern is built.
    let name = "\\k".repeat(3_000);
    let text = format!("<{name}>x</{}>", "a".repeat(3_000));
    ALLOCATED.set(0);
    assert_eq!(after(&text, (0, 0), "dit").2, "");
    let allocated = ALLOCATED.get();
    assert!(allocated < 2 * SPELLED, "{allocated} elements");
}

#[test]
fn pattern_pairing_tests_only_real_candidates() {
    // The `<b>`s can't start a tag named `a.b`: one search of the row finds
    // where `<a.b` and `</a.b` match, and only `</axb>` is tried.
    let many = "<b>".repeat(20_000);
    let text = format!("<a.b>{many}x</axb>");
    SPANS.set(0);
    assert_eq!(after(&text, (0, 0), "dit").2, format!("{many}x"));
    assert!(SPANS.get() <= 4, "{} spans", SPANS.get());
    // Across rows too, and with the `<b>`s on rows of their own.
    let text = format!("<a.b>\n{}x\n</axb>", "<b>\n".repeat(5_000));
    SPANS.set(0);
    assert_eq!(
        after(&text, (0, 0), "dit").2,
        format!("\n{}x\n", "<b>\n".repeat(5_000))
    );
    assert!(SPANS.get() <= 4);
}

#[test]
fn a_pattern_name_is_compiled_once_for_all_its_tags() {
    // A count of 150 pairs 150 openings of one name, each against the
    // closing tags after it.
    let many = "<a.b>".repeat(60);
    let text = format!("<b>{many}x</axb></b>");
    COMPILED.set(0);
    let got = after(&text, (0, 3 + many.len()), "40yit");
    assert!(!got.2.is_empty());
    assert_eq!(COMPILED.get(), 1);
}

#[test]
fn preparing_a_long_pattern_name_allocates_in_proportion() {
    let name = format!("a.{}", "b".repeat(400));
    let text = format!("<{name}>x</axb{}>", "b".repeat(399));
    ALLOCATED.set(0);
    assert_eq!(after(&text, (0, 0), "dit").2, "x");
    let allocated = ALLOCATED.get();
    assert!(allocated < 2048 + 12 * name.len(), "{allocated} elements");
}

#[test]
fn names_are_compared_by_all_their_composing_chars() {
    // A char with a stretch of marks and then another: not the same name
    // as the same with a different last one.
    let marks = "\u{301}".repeat(WORK / 2);
    let text = format!("<e{marks}\u{300}>x</e{marks}\u{302}>");
    assert_eq!(after(&text, (0, 0), "dit").2, "");
    let text = format!("<e{marks}\u{300}>x</e{marks}\u{300}>");
    assert_eq!(after(&text, (0, 0), "dit").2, "x");
}

#[test]
fn a_grapheme_too_long_to_segment_in_a_step_fails_the_object() {
    // A base with 20,000 marks, in a name (plain and pattern), and in a row
    // the pairing reads: not read in unbounded steps, so the object fails.
    let marks = "\u{301}".repeat(20_000);
    for text in [
        format!("<e{marks}>x</e{marks}>"),
        format!("<e{marks}.>x</e{marks}x>"),
        format!("<a.b>x{marks}</axb>"),
    ] {
        assert_eq!(after(&text, (0, 0), "dit").2, "", "{}", text.len());
    }
}
