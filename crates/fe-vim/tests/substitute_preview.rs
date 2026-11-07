//! 'inccommand': [`fe_vim::VimState::substitute_preview`] gives what the
//! `:s` an open `:` line would run replaces while it's typed, matched as
//! submitting it would match, and changes nothing.

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use fe_vim::{
    HeadlessVim, SUBSTITUTE_PREVIEW_BYTES, SUBSTITUTE_PREVIEW_CAP, SubstitutePreview,
    search::set_time_limit, set_search_preview_limit,
};

const TEXT: &str = "one two\ntwo one\nOne Two\nthree\nfour one\nfive\n";

static LIMITS: Mutex<()> = Mutex::new(());

/// Keeps the previews from giving up on a loaded machine's clock: raises
/// both the preview's limit and the search's (which caps it). The limits are
/// process-wide and one test lowers the search's to see it bite, so each
/// test holds the guard for its whole run, which serializes them.
fn unhurried() -> MutexGuard<'static, ()> {
    let guard = LIMITS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    set_search_preview_limit(Some(Duration::from_secs(60)));
    set_time_limit(Duration::from_secs(60));
    guard
}

/// `text` with each of `preview`'s matches replaced, checking they come in
/// order and apart.
fn applied(text: &str, preview: &SubstitutePreview) -> String {
    let mut out = String::new();
    let mut at = 0;
    for (range, new) in &preview.matches {
        assert!(range.start >= at, "{:?} overlaps or is out of order", range);
        out.push_str(&text[at..range.start]);
        out.push_str(new);
        at = range.end;
    }
    out.push_str(&text[at..]);
    out
}

/// After `setup` keys, opens the line with `open` and previews it with
/// `typed` after its prefill, then submits it: the preview applied to the
/// text, and the text the `:s` left.
fn differential(
    text: &str,
    cursor: (u32, usize),
    setup: &str,
    open: &str,
    typed: &str,
) -> (String, String) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(setup).unwrap();
    vim.feed(open).unwrap();
    let line = format!("{}{typed}", vim.state().command_line_prefill());
    let before = vim.text();
    let preview = vim
        .state()
        .substitute_preview(vim.model(), vim.registers(), &line)
        .unwrap_or_else(|| panic!("no preview of {line:?}"));
    assert!(!preview.truncated, "{line:?}");
    vim.feed(&format!("{typed}<CR>")).unwrap();
    assert!(!vim.state().failed(), "{line:?} failed");
    (applied(&before, &preview), vim.text())
}

#[test]
fn substitute_preview_matches_the_real_substitute() {
    let _limits = unhurried();
    let same = |cursor: (u32, usize), setup: &str, open: &str, typed: &str| {
        let (previewed, ran) = differential(TEXT, cursor, setup, open, typed);
        assert_eq!(previewed, ran, "{setup:?} {open:?} {typed:?}");
        assert_ne!(ran, TEXT, "{typed:?} changed nothing");
    };
    // The cursor's line, every line, a visual range, a relative range, a
    // count, a pattern's address.
    same((1, 0), "", ":", "s/o/0/");
    same((0, 0), "", ":", "%s/o/0/");
    same((0, 0), "", ":", "%s/o/0/g");
    same((1, 0), "", "Vj:", "s/o/0/g");
    same((0, 0), "", ":", ".,+3s/o/0/g");
    same((0, 0), "", ":", "s/o/0/g 3");
    same((0, 0), "", ":", "/four/,$s/o/0/g");
    same((0, 0), "", ":", "2;+1s/o/0/g");
    // Flags: `i` and `I`, against 'ignorecase'.
    same((0, 0), "", ":", "%s/one/1/gi");
    same((0, 0), ":set ignorecase<CR>", ":", "%s/one/1/g");
    same((0, 0), ":set ignorecase<CR>", ":", "%s/one/1/gI");
    // Repeats: `:&&` keeps the last flags, `:&` drops them, `:~` takes
    // the last search's pattern.
    same((0, 0), ":s/o/0/g<CR>", ":", "%&&");
    same((0, 0), ":s/o/0/g<CR>", ":", "%&");
    same((0, 0), ":s/o/0/g<CR>/t<CR>", ":", "%~");
    same((0, 0), "", ":", "%s/o/&&/g");
    // The replacement's specials.
    same((0, 0), "", ":", r"%s/t\(w\)o/[&|\1]/g");
    same((0, 0), "", ":", r"%s/\w\+/\u&/g");
    same((0, 0), "", ":", r"%s/\w\+/\U&\E!/g");
    same((0, 0), ":s/one/ONE/<CR>", ":", "%s/two/~~/g");
    same((0, 0), "", ":", r"%s/ /\r/g");
    // Patterns over rows: joining rows, and the joined row matched on.
    same((0, 0), "", ":", r"%s/o\n/_/g");
    same((0, 0), "", ":", r"%s/e\n\(f\)/\1/g");
    same((0, 0), "", ":", r"%s/\n//g");
    same((0, 0), "", ":", r"1,3s/\n//");
    same((4, 0), "", ":", r"s/\n//");

    // `\n` in a replacement is refused by both.
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed(":").unwrap();
    let preview = vim
        .state()
        .substitute_preview(vim.model(), vim.registers(), r"%s/o/\n/");
    assert_eq!(preview, None);
    vim.feed(r"%s/o/\n/<CR>").unwrap();
    assert!(vim.state().failed());
    assert_eq!(vim.text(), TEXT);
}

#[test]
fn substitute_preview_has_no_side_effects() {
    let _limits = unhurried();
    let mut vim = HeadlessVim::new(TEXT, (1, 2));
    // A last search, a last substitute, history, marks and jumps to keep.
    vim.feed("ma/two<CR>:s/o/0/<CR>u3Gmb:").unwrap();
    let text = vim.text();
    let registers = vim.registers().clone();
    let state = format!("{:?}", vim.state());
    let cursor = vim.cursor();
    for line in [
        "%s/e/E/g",
        "/four/;+1s/o/0/g",
        r"%s/\(o\)\n/\1\r\r/g",
        "%&&",
        "'a,'bs/t/T/",
    ] {
        let preview = vim
            .state()
            .substitute_preview(vim.model(), vim.registers(), line);
        assert!(preview.is_some(), "{line:?}");
        assert_eq!(vim.text(), text, "{line:?}");
        assert!(
            *vim.registers() == registers,
            "{line:?} changed the registers"
        );
        assert_eq!(format!("{:?}", vim.state()), state, "{line:?}");
        assert_eq!(vim.cursor(), cursor, "{line:?}");
    }

    // Nothing to preview.
    let none = |vim: &HeadlessVim, line: &str| {
        vim.state()
            .substitute_preview(vim.model(), vim.registers(), line)
    };
    for line in [
        // Still being typed.
        "",
        "%",
        "%s",
        "%subst",
        "%s/o",
        r"%s/o\/",
        // An empty pattern would be the last search's.
        "%s//x/",
        // Asking, counting.
        "%s/o/0/c",
        "%s/o/0/gn",
        // Invalid, or matching nothing.
        r"%s/o\(/0/",
        r"%s/o/\=1/",
        "%s/zzz/0/",
        "99s/o/0/",
        "/nowhere/s/o/0/",
        "%s/o/0/ x",
        // Not a substitute, or more than one command.
        "w",
        "%d",
        "g/o/s//0/",
        "s/o/0/|s/t/T/",
    ] {
        assert_eq!(none(&vim, line), None, "{line:?}");
    }
    assert_eq!(vim.text(), text);
    assert!(*vim.registers() == registers);

    // Only a `:` line previews.
    let mut search = HeadlessVim::new(TEXT, (0, 0));
    search.feed("/").unwrap();
    assert_eq!(none(&search, "s/o/0/"), None);
    let mut normal = HeadlessVim::new(TEXT, (0, 0));
    normal.feed("").unwrap();
    assert_eq!(none(&normal, "s/o/0/"), None);

    // 'inccommand' off.
    vim.registers_mut().set_inccommand(false);
    assert!(!vim.registers().inccommand());
    assert_eq!(none(&vim, "%s/o/0/g"), None);
    vim.registers_mut().set_inccommand(true);
    assert!(none(&vim, "%s/o/0/g").is_some());
}

#[test]
fn substitute_preview_is_capped_and_timed() {
    let _limits = unhurried();
    // Past the cap, the first matches and `truncated`.
    let text = "ab ".repeat(SUBSTITUTE_PREVIEW_CAP + 20);
    let mut vim = HeadlessVim::new(&text, (0, 0));
    vim.feed(":").unwrap();
    let preview = vim
        .state()
        .substitute_preview(vim.model(), vim.registers(), "s/b/c/g")
        .unwrap();
    assert!(preview.truncated);
    assert_eq!(preview.matches.len(), SUBSTITUTE_PREVIEW_CAP);
    assert_eq!(preview.matches[0], (1..2, "c".to_string()));
    let exact = "ab ".repeat(SUBSTITUTE_PREVIEW_CAP);
    let mut vim = HeadlessVim::new(&exact, (0, 0));
    vim.feed(":").unwrap();
    let preview = vim
        .state()
        .substitute_preview(vim.model(), vim.registers(), "s/b/c/g")
        .unwrap();
    assert!(!preview.truncated);
    assert_eq!(preview.matches.len(), SUBSTITUTE_PREVIEW_CAP);

    // A fast pattern whose replacement grows each match a hundredfold stops
    // at the bytes: a 100 kB match once, or 2 kB ones on 50 rows once they
    // add up (well under the match cap), never writing out the whole of it.
    let amps = "&".repeat(100);
    for (text, line) in [
        ("a".repeat(100_000), format!("s/a*/{amps}/")),
        (
            format!("{}\n", "x".repeat(2_000)).repeat(50),
            format!(r"%s/x\+/{amps}/"),
        ),
    ] {
        let mut vim = HeadlessVim::new(&text, (0, 0));
        vim.feed(":").unwrap();
        let started = Instant::now();
        let preview = vim
            .state()
            .substitute_preview(vim.model(), vim.registers(), &line);
        let took = started.elapsed();
        assert!(took < Duration::from_secs(5), "{took:?}");
        let written: usize = (preview.iter())
            .flat_map(|preview| &preview.matches)
            .map(|(_, new)| new.len())
            .sum();
        assert!(written <= SUBSTITUTE_PREVIEW_BYTES, "{written}");
        assert!(
            preview.as_ref().is_none_or(|preview| preview.truncated),
            "{line:.12}"
        );
    }
    // Pieces that write nothing (case operators, empty groups) cost as
    // much: a few matches take all there is.
    let text = "a\n".repeat(600);
    for template in [r"\u".repeat(300_000), r"\1".repeat(300_000)] {
        let mut vim = HeadlessVim::new(&text, (0, 0));
        vim.feed(":").unwrap();
        let line = format!(r"%s/\(x\)\?a/{template}/");
        let started = Instant::now();
        let preview = vim
            .state()
            .substitute_preview(vim.model(), vim.registers(), &line)
            .unwrap();
        let took = started.elapsed();
        assert!(took < Duration::from_secs(5), "{took:?}");
        assert!(preview.truncated);
        assert!(preview.matches.len() < 10, "{}", preview.matches.len());
    }
    // A `~` standing for a replacement doubled 20 times (2 MiB) is past the
    // bytes before it's built: no preview, though the :s would run it.
    let mut vim = HeadlessVim::new("a\n", (0, 0));
    vim.feed(":s/zzz/ab/e<CR>").unwrap();
    for _ in 0..20 {
        vim.feed(":s/zzz/~~/e<CR>").unwrap();
    }
    vim.feed(":").unwrap();
    let started = Instant::now();
    for line in ["s/a/~/", "s/a/~~~~~~~~~~/"] {
        let preview = vim
            .state()
            .substitute_preview(vim.model(), vim.registers(), line);
        assert_eq!(preview, None, "{line}");
    }
    assert!(started.elapsed() < Duration::from_secs(5));
    vim.feed("s/a/~/<CR>").unwrap();
    assert_eq!(vim.text().len(), 2 << 20 | 1);

    // Under the bytes, all of it.
    let text = "abc\n".repeat(100);
    let mut vim = HeadlessVim::new(&text, (0, 0));
    vim.feed(":").unwrap();
    let preview = vim
        .state()
        .substitute_preview(vim.model(), vim.registers(), &format!("%s/abc/{amps}/"))
        .unwrap();
    assert!(!preview.truncated);
    assert_eq!(preview.matches.len(), 100);

    // A slow pattern gives up at the time limit (here the search's, the
    // preview's own lifted): each `a` scans to the row's end for a `z`,
    // which would take minutes. The limit is lowered for this one search
    // (the guard keeps the other tests out) and the bound below is far past
    // it.
    set_time_limit(Duration::from_secs(2));
    let text = "a".repeat(200_000);
    let mut vim = HeadlessVim::new(&text, (0, 0));
    vim.feed(":").unwrap();
    let started = Instant::now();
    let preview = vim
        .state()
        .substitute_preview(vim.model(), vim.registers(), r"s/a.*z\|a/b/g");
    let took = started.elapsed();
    assert!(
        preview.as_ref().is_none_or(|preview| preview.truncated),
        "{:?}",
        preview.map(|preview| preview.matches.len())
    );
    assert!(took < Duration::from_secs(20), "{took:?}");
}
