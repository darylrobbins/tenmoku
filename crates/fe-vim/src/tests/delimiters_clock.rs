//! Tests of what the clock, the pattern names and the passes over the text
//! cost, which read thread-locals of `src/delimiters.rs`.
use std::{collections::BTreeSet, ops::Range};

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

/// What `keys` on `text` (caret at `column`) leave, with every clock on the
/// thread running out at its look `look` (or not): the text, the unnamed
/// register, each look taken (its phase and line of `delimiters.rs`), and
/// whether a clock ran out.
fn run(
    text: &str,
    column: usize,
    keys: &str,
    look: Option<usize>,
) -> (String, String, Vec<(&'static str, u32)>, bool) {
    LOOKS.set(look);
    LOOKED.set(0);
    EXPIRED.set(false);
    MOST_STEPS.set(0);
    SINCE.set(0);
    PHASE.set("");
    LATE.set(0);
    LOG.with_borrow_mut(Vec::clear);
    RECORD.set(true);
    let (got, _, register) = after(text, (0, column), keys);
    RECORD.set(false);
    LOOKS.set(None);
    let log = LOG.with_borrow_mut(std::mem::take);
    (got, register, log, EXPIRED.get())
}

/// Runs `keys` on `text` (caret at `column`), which take `taken`, once to
/// see each look at the clock the command takes, then again with the clock
/// running out at each of those looks in turn: each time the object fails
/// with the text unchanged and the register `failed` (what `keys` take
/// without it), and no pattern is compiled once it's out. In every run, no
/// pass takes more than [`WORK`] steps between two looks (each pass counts
/// the steps it really takes). The lines of `delimiters.rs` looked from.
fn expire_at_every_look(
    text: &str,
    column: usize,
    keys: &str,
    taken: &str,
    failed: &str,
) -> BTreeSet<u32> {
    let bound = |look: Option<usize>| {
        assert!(
            MOST_STEPS.get() <= WORK,
            "{keys} at look {look:?}: {} steps between looks",
            MOST_STEPS.get()
        );
        assert_eq!(
            LATE.get(),
            0,
            "{keys} at look {look:?}: compiled past the clock"
        );
    };
    let (_, register, log, expired) = run(text, column, keys, None);
    assert_eq!((register.as_str(), expired), (taken, false), "{keys}");
    bound(None);
    for (look, &(phase, line)) in log.iter().enumerate() {
        let (got, register, _, expired) = run(text, column, keys, Some(look));
        assert!(expired, "{keys}: look {look} of {}", log.len());
        assert_eq!(
            (got.as_str(), register.as_str()),
            (text, failed),
            "{keys} out of time at look {look} ({phase}, line {line})"
        );
        bound(Some(look));
    }
    log.iter().map(|&(_, line)| line).collect()
}

/// The lines of `delimiters.rs` that look at a command's clock: a call that
/// spends, checks, meters or compiles, outside a `#[track_caller]` helper
/// (whose looks are its caller's line) and the tests.
fn look_sites() -> BTreeSet<u32> {
    const CALLS: [&str; 5] = [".spend(", ".check()", ".ahead(", "metered(", "compile("];
    let source = include_str!("../delimiters.rs");
    let mut sites = BTreeSet::new();
    let (mut caller, mut skip) = (false, None::<usize>);
    for (ix, line) in source.lines().enumerate() {
        let code = line.trim_start();
        let indent = line.len() - code.len();
        if code == "mod clock_tests;" {
            break;
        }
        if let Some(depth) = skip {
            skip = (indent != depth || code != "}").then_some(depth);
            continue;
        }
        if code == "#[track_caller]" {
            caller = true;
            continue;
        }
        if caller && code.starts_with("fn ") {
            (caller, skip) = (false, Some(indent));
            continue;
        }
        if !code.starts_with("//")
            && !code.contains("fn ")
            && CALLS.iter().any(|call| code.contains(call))
        {
            sites.insert(ix as u32 + 1);
        }
    }
    sites
}

/// Commands that take every look `delimiters.rs` has: the text, the
/// caret's column, the keys, what they take, and what they take once the
/// object fails.
type Command = (String, usize, &'static str, String, &'static str);

fn commands() -> Vec<Command> {
    // `dit` from column 0, taking `x`.
    let dit = |text: String| (text, 0, "dit", "x".to_string(), "");
    // Names read and compared: ASCII and not (a run longer than a segment
    // without ASCII), composed (paired by a pass of their own), the same
    // but for case (long ones too, read side by side), a grapheme of many
    // marks matched by the same marks in another order, and the table of
    // places a name can start (looking at every 256 tags too). (nvim pairs
    // no name with a char of more than 6 composing chars, which fe-vim
    // doesn't follow yet: the graphemes of many marks here are for what
    // comparing them takes.)
    let long = "b".repeat(5_000);
    let wide = "é".repeat(3_000);
    let composed = "e\u{301}".repeat(2_000);
    // 600 marks, 1,800 bytes: comparing two takes about what reading them
    // does.
    let nonspacing = Regex::new(r"^\p{Mn}$").unwrap();
    let marks: String = (0x800..0x1_0000)
        .filter_map(char::from_u32)
        .filter(|ch| nonspacing.is_match(ch.encode_utf8(&mut [0; 4])))
        .take(600)
        .collect();
    assert_eq!(format!("e{marks}").graphemes(true).count(), 1);
    let backwards: String = marks.chars().rev().collect();
    let run = "<".repeat(300);
    // The pass back over unnested tags, a count passing over openings, and
    // a visual selection.
    let many = "<a->".repeat(20);
    let open = "<q>".repeat(20);
    let mut commands = vec![
        dit(format!("<{long}>x</{long}>")),
        dit(format!("<{wide}>x</{wide}>")),
        dit(format!("<{composed}>x</{composed}>")),
        dit("<Ab c>x</aB>".into()),
        dit(format!("<{}>x</{}>", "A".repeat(5_000), "a".repeat(5_000))),
        dit(format!(
            "<{}>x</{}>",
            format!("e{marks}").repeat(4),
            format!("e{backwards}").repeat(4),
        )),
        (format!("<a>x{run}></a>"), 3, "dit", format!("x{run}>"), ""),
        (format!("{many}x</a->"), many.len(), "yit", "x".into(), ""),
        (
            format!("<a>{open}x</a>"),
            3 + open.len(),
            "10yit",
            format!("{open}x"),
            "",
        ),
        ("<a>xy</a>".into(), 3, "vity", "xy".into(), "x"),
    ];
    // Pattern names, prepared and paired (one nesting), and expensive ones
    // to prepare: long, with class atoms, and with joined graphemes in
    // collections, with a `-` last, and with ranges of private-use chars
    // taking a few of a window of them, then all of one, or held by
    // collections of more than one base (so no char of them stands in),
    // and joined graphemes of many marks, satisfying clusters of them.
    let many = "<a.b>".repeat(10);
    let name = format!("a.{}", "b".repeat(300));
    commands.extend([
        (
            format!("<b>{many}x</axb></b>"),
            3 + many.len(),
            "yit",
            "x".into(),
            "",
        ),
        (
            "<a.b>y<axb>x</axb></axb>".into(),
            5,
            "dit",
            "y<axb>x</axb>".into(),
            "",
        ),
        dit(format!("<{name}>x</axb{}>", "b".repeat(299))),
        dit("<a\u{200d}.\\i\\I>x</a\u{200d}baa>".into()),
        dit("<a\u{200d}[a-][^b]>x</a\u{200d}a\u{200d}a\u{200d}>".into()),
        dit("<a\u{200d}[^\\U000f0000-\\U000f0002]>x</a\u{200d}a\u{200d}>".into()),
        dit("<a\u{200d}[^\\U000f0000-\\U000f07ff]>x</a\u{200d}a\u{200d}>".into()),
        dit(format!(
            "<e{marks}\u{200d}e{few}\u{200d}.>x</e{backwards}\u{200d}e{marks}\u{200d}b> {}",
            format!("e{marks}\u{200d}").repeat(4),
            few = marks.chars().take(100).collect::<String>(),
        )),
        dit(format!(
            "<a\u{200d}b\u{200d}[a{range}][b{range}]>x</a\u{200d}b\u{200d}ab>",
            range = "\\U000f0000-\\U000f07ff",
        )),
    ]);
    commands
}

/// [`expire_at_every_look`] over `commands[range]` (each a test of its own,
/// so they run side by side).
fn expire_at_every_look_of(range: Range<usize>) {
    for (text, column, keys, taken, failed) in &commands()[range] {
        expire_at_every_look(text, *column, keys, taken, failed);
    }
}

#[test]
fn expiring_at_any_look_reading_names_fails_the_object() {
    expire_at_every_look_of(0..6);
}

#[test]
fn expiring_at_any_look_reading_tags_fails_the_object() {
    expire_at_every_look_of(6..10);
}

#[test]
fn expiring_at_any_look_pairing_a_pattern_fails_the_object() {
    expire_at_every_look_of(10..14);
}

#[test]
fn expiring_at_any_look_preparing_a_pattern_fails_the_object() {
    expire_at_every_look_of(14..18);
}

#[test]
fn expiring_at_any_look_choosing_stand_ins_fails_the_object() {
    expire_at_every_look_of(18..19);
}

#[test]
fn every_look_at_the_clock_is_taken_and_compiling_looks_first() {
    // The tests expiring at each look cover them all.
    assert_eq!(commands().len(), 19);
    let mut taken = BTreeSet::new();
    for (text, column, keys, _, _) in commands() {
        let (_, _, log, _) = run(&text, column, keys, None);
        taken.extend(log.iter().map(|&(_, line)| line));
    }
    let sites = look_sites();
    assert!(sites.len() > 40, "{sites:?}");
    let missed: Vec<u32> = sites.difference(&taken).copied().collect();
    assert_eq!(missed, Vec::<u32>::new(), "looks no command takes");
    // Every pattern is compiled by `compile`, which looks first.
    let source = include_str!("../delimiters.rs");
    let code = &source[..source.find("mod clock_tests;").unwrap()];
    assert_eq!(code.matches("Pattern::parse(").count(), 1);
}

#[test]
fn a_name_that_spells_out_to_too_much_fails_in_bounded_room() {
    // 3,000 `\k` atoms are collections of a KB or so each: past what a
    // name may spell out to, which fails the object before it's read.
    let name = "\\k".repeat(3_000);
    let text = format!("<{name}>x</{}>", "a".repeat(3_000));
    ALLOCATED.set(0);
    assert_eq!(after(&text, (0, 0), "dit").2, "");
    let allocated = ALLOCATED.get();
    assert!(allocated < 2 * text.len(), "{allocated} elements");
    // One that spells out to just too much, as a few atoms.
    let name = format!("a\u{200d}{}", "\\k".repeat(4));
    let text = format!("<{name}>x</a\u{200d}aaaa>");
    assert_eq!(after(&text, (0, 0), "dit").2, "");
    let name = format!("a\u{200d}{}", "\\k".repeat(2));
    let text = format!("<{name}>x</a\u{200d}aa>");
    assert_eq!(after(&text, (0, 0), "dit").2, "x");
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
    let marks = "\u{301}".repeat(SEGMENT / 2 - 2);
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
