//! Tests of the jump feature: `gs` and `gS` as a motion, the pure matcher
//! and labeler, and the labelled session.
use std::collections::HashSet;

use fe_buffer::{Origin, ResolvedSelection};
use fe_command::ContextFrame;
use fe_text::Text;

use super::{commit, invocation, run};
use crate::{
    HeadlessVim, JumpChars, JumpMotion, Motion, MoveArgs, OperateArgs, Operator, Step, Target,
    VimKey, VimState,
    jump::{
        JumpLabelState,
        label::{FULL, SAFE, label, targets},
    },
    parse_keys,
};

fn jump(chars: &str, forward: bool) -> Motion {
    let chars: Vec<char> = chars.chars().collect();
    Motion::Jump(JumpMotion {
        chars: JumpChars([chars[0], chars[1]]),
        forward,
    })
}

/// `keys` over `text` from `cursor`: the text and the caret.
fn feed(text: &str, cursor: (u32, usize), keys: &str) -> (String, (u32, usize)) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    (vim.text(), vim.cursor())
}

/// Where `keys` leave the caret, the text unchanged.
fn lands(text: &str, cursor: (u32, usize), keys: &str) -> (u32, usize) {
    let (after, caret) = feed(text, cursor, keys);
    assert_eq!(after, text, "{keys}");
    caret
}

#[test]
fn keys_commit_the_motion() {
    let moving = |keys: &str, motion, count, extend| {
        let committed = commit(keys);
        let want = invocation(
            "vim.move",
            MoveArgs {
                motion,
                extend,
                count,
            },
        );
        assert_eq!(committed.command, want.command, "{keys}");
        assert_eq!(committed.args, want.args, "{keys}");
    };
    moving("gsab", jump("ab", true), None, false);
    moving("gSab", jump("ab", false), None, false);
    moving("3gs  ", jump("  ", true), Some(3), false);

    let committed = commit("\"a2dgSxY");
    let want = OperateArgs {
        operator: Operator::Delete,
        target: Target::Motion(jump("xY", false)),
        count: Some(2),
        register: Some('a'),
    };
    assert_eq!(committed.command.as_str(), "vim.operate");
    assert_eq!(committed.args, serde_json::to_value(want).unwrap());

    // The motion's JSON, both ways.
    let json = serde_json::json!({"jump": {"chars": "ab", "forward": true}});
    assert_eq!(serde_json::to_value(jump("ab", true)).unwrap(), json);
    assert_eq!(
        serde_json::from_value::<Motion>(json).unwrap(),
        jump("ab", true)
    );
    for chars in ["a", "abc", ""] {
        let json = serde_json::json!({"jump": {"chars": chars, "forward": true}});
        assert!(serde_json::from_value::<Motion>(json).is_err(), "{chars:?}");
    }

    // `gs` waits; `<Esc>` or another key after `g` cancels as before.
    assert!(matches!(run("gs"), Step::Wait(_)));
    assert!(matches!(run("dgSa"), Step::Wait(_)));
    assert_eq!(run("gs<Esc>"), Step::Cancel);
    assert_eq!(run("gsa<Esc>"), Step::Cancel);
    assert_eq!(run("gZ"), Step::Cancel);
}

#[test]
fn pending_kind_is_jump_until_both_chars() {
    let stamp = |state: &VimState| {
        let frame = state.stamp(ContextFrame::new("editor"));
        frame.get("vim.pending_kind").map(str::to_string)
    };
    for prefix in ["", "d", "\"a3y", "v", "V", "<C-v>"] {
        let mut state = VimState::new();
        let keys = parse_keys(&format!("{prefix}gs")).unwrap();
        for key in keys {
            state.feed(key);
        }
        assert_eq!(stamp(&state).as_deref(), Some("jump"), "{prefix}");
        state.feed(VimKey::Char('a'));
        assert_eq!(stamp(&state).as_deref(), Some("jump"), "{prefix}");
        assert_eq!(state.feed(VimKey::Esc), Step::Cancel, "{prefix}");
        assert_eq!(stamp(&state), None, "{prefix}");
    }

    // `<Esc>` changes nothing, even in a visual mode.
    let mut vim = HeadlessVim::new("ab cd\nab", (0, 0));
    vim.feed("vlgsc<Esc>").unwrap();
    assert_eq!(vim.mode(), "visual");
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((0, 1), Some((0, 0))));
    let mut vim = HeadlessVim::new("ab cd\nab", (0, 0));
    vim.feed("dgsc<Esc>").unwrap();
    assert_eq!((vim.text(), vim.cursor()), ("ab cd\nab".into(), (0, 0)));
    assert_eq!(vim.mode(), "normal");
}

#[test]
fn nearest_match_in_either_direction_over_the_whole_text() {
    let text = "ab xab\ncd ab\nab";
    assert_eq!(lands(text, (0, 0), "gsab"), (0, 4));
    assert_eq!(lands(text, (0, 4), "gsab"), (1, 3));
    assert_eq!(lands(text, (1, 3), "gsab"), (2, 0));
    assert_eq!(lands(text, (2, 0), "gSab"), (1, 3));
    assert_eq!(lands(text, (1, 0), "gSab"), (0, 4));
    assert_eq!(lands(text, (0, 4), "gSab"), (0, 0));
    // The nth, counted.
    assert_eq!(lands(text, (0, 0), "2gsab"), (1, 3));
    assert_eq!(lands(text, (0, 0), "3gsab"), (2, 0));
    assert_eq!(lands(text, (2, 0), "3gSab"), (0, 0));
    // Not the caret's own match.
    assert_eq!(lands("ab ab", (0, 0), "gsab"), (0, 3));
    assert_eq!(lands("ab ab", (0, 3), "gSab"), (0, 0));
}

#[test]
fn matching_folds_case() {
    assert_eq!(lands("xx AB", (0, 0), "gsab"), (0, 3));
    assert_eq!(lands("xx aB", (0, 0), "gsAb"), (0, 3));
    assert_eq!(lands("Éa xé", (0, 4), "gSéA"), (0, 0));
    // The base char of a grapheme, as `f` matches.
    assert_eq!(lands("x e\u{301}z", (0, 0), "gsez"), (0, 2));
    // Every case variant, typed or in the text: `Σ σ ς`, `S s ſ`, `K k`
    // and the Kelvin sign.
    for (text, keys) in [
        ("a ςx", "gsΣx"),
        ("a Σx", "gsςx"),
        ("a σx", "gsςX"),
        ("a ſx", "gsSx"),
        ("a Sx", "gsſx"),
        ("a \u{212a}x", "gskx"),
        ("a kx", "gs\u{212a}x"),
    ] {
        assert_eq!(lands(text, (0, 0), keys), (0, 2), "{text} {keys}");
    }
    // Unicode's simple folds whose upper cases expand: `ΐ ΐ`, `ΰ ΰ`, `ﬅ ﬆ`.
    for (x, y) in [
        ('\u{390}', '\u{1fd3}'),
        ('\u{3b0}', '\u{1fe3}'),
        ('\u{fb05}', '\u{fb06}'),
    ] {
        for (typed, text) in [(x, y), (y, x)] {
            let text = format!("a {text}x");
            assert_eq!(
                lands(&text, (0, 0), &format!("gs{typed}x")),
                (0, 2),
                "{text}"
            );
        }
        // And they make one run.
        let text = format!("a {x}{y}{x} {y}{y}");
        let twice = format!("gs{x}{y}");
        assert_eq!(lands(&text, (0, 0), &twice), (0, 2), "{text}");
        let next = 2 + 2 * x.len_utf8() + y.len_utf8() + 1;
        assert_eq!(lands(&text, (0, 2), &twice), (0, next), "{text}");
    }
    // A char with no simple fold keeps its own: `ß`, and dotless `ı` isn't
    // `i` (nor `I`), nor dotted `İ`.
    assert_eq!(lands("ab ßx", (0, 0), "gsßx"), (0, 3));
    for (text, keys) in [
        ("ab ßx", "gssx"),
        ("a ıx", "gsix"),
        ("a ıx", "gsIx"),
        ("a ix", "gsıx"),
        ("a İx", "gsix"),
    ] {
        let (_, caret) = feed(text, (0, 0), keys);
        assert_eq!(caret, (0, 0), "{text} {keys}");
    }
    // The variants make one run.
    assert_eq!(lands("x ςσΣ ΣΣ", (0, 0), "gsσς"), (0, 2));
    assert_eq!(lands("x ςσΣ ΣΣ", (0, 2), "gsσς"), (0, 9));
}

#[test]
fn space_matches_a_blank_or_a_line_end() {
    // `{c} `: a space, a tab, or the row's last char.
    assert_eq!(lands("ab ac\tad", (0, 0), "gsc "), (0, 4));
    assert_eq!(lands("ab ac\tad", (0, 0), "gsb "), (0, 1));
    assert_eq!(lands("xd\nad", (0, 1), "gsd "), (1, 1));
    assert_eq!(lands("ab\tc", (0, 0), "gs c"), (0, 2));
    // `  `: an empty row, at its start, or a blank run's start.
    assert_eq!(lands("ab\n\ncd", (0, 0), "gs  "), (1, 0));
    assert_eq!(lands("ab\ncd\n", (0, 0), "gs  "), (2, 0));
    assert_eq!(lands("ab\n\ncd", (2, 1), "gS  "), (1, 0));
    assert_eq!(lands("a  \t b", (0, 0), "gs  "), (0, 1));
    // A line end is never the first char.
    let (_, caret) = feed("ab\ncd", (0, 0), "gs c");
    assert_eq!(caret, (0, 0));
}

#[test]
fn a_same_char_pair_matches_a_run_at_its_start() {
    assert_eq!(lands("x ===== ==", (0, 0), "gs=="), (0, 2));
    assert_eq!(lands("x ===== ==", (0, 2), "gs=="), (0, 8));
    assert_eq!(lands("x ===== ==", (0, 9), "gS=="), (0, 8));
    assert_eq!(lands("x ===== ==", (0, 8), "gS=="), (0, 2));
    assert_eq!(lands("aAaA b", (0, 5), "gSaa"), (0, 0));
}

#[test]
fn a_space_and_a_tab_are_a_distinct_pair() {
    // A tab can't be typed after `gs` (the key cancels), but the motion
    // can carry one.
    let all = |text: &str, chars, forward| {
        let rope = Text::new(text);
        let caret = if forward { 0 } else { text.len() };
        targets(&rope, 0..rope.row_count(), caret, chars, forward)
    };
    for forward in [true, false] {
        // Space then tab, wherever it is, a blank before it included.
        assert_eq!(all("x \t", [' ', '\t'], forward), [1]);
        assert_eq!(all("x  \ty", [' ', '\t'], forward), [2]);
        assert_eq!(all("x\t \t", [' ', '\t'], forward), [2]);
        // Tab then space.
        assert_eq!(all("x\t y", ['\t', ' '], forward), [1]);
        assert_eq!(all("x \t y", ['\t', ' '], forward), [2]);
        // The second char, a space, still takes a tab: both match.
        let want: &[usize] = if forward { &[1, 2] } else { &[2, 1] };
        assert_eq!(all("x\t\t y", ['\t', ' '], forward), want);
        // A same pair is still a run's start only.
        assert_eq!(all("x \t\t", ['\t', '\t'], forward), [2]);
    }
    // And as the motion.
    let mut vim = HeadlessVim::new("x  \ty", (0, 0));
    let args = MoveArgs {
        motion: jump(" \t", true),
        extend: false,
        count: None,
    };
    vim.dispatch(&invocation("vim.move", args)).unwrap();
    assert_eq!(vim.cursor(), (0, 2));
}

#[test]
fn no_match_fails_with_nothing_changed() {
    for keys in ["gsxy", "gSab", "3gsab", "dgsxy", "\"aygs  ", "cgSab"] {
        let mut vim = HeadlessVim::new("ab ab\ncd", (0, 0));
        vim.feed(keys).unwrap();
        assert!(vim.state().failed(), "{keys}");
        assert_eq!(vim.text(), "ab ab\ncd", "{keys}");
        assert_eq!(vim.cursor(), (0, 0), "{keys}");
        assert_eq!(vim.mode(), "normal", "{keys}");
        assert_eq!(vim.mark('\''), None, "{keys}");
        assert!(vim.registers().get('"').is_none(), "{keys}");
    }
    // In a visual mode the selection stays.
    let mut vim = HeadlessVim::new("ab ab\ncd", (0, 1));
    vim.feed("vgSxy").unwrap();
    assert!(vim.state().failed());
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((0, 1), Some((0, 1))));
    // A failed operator isn't what `.` repeats.
    let mut vim = HeadlessVim::new("abcabc", (0, 0));
    vim.feed("xdgsqq.").unwrap();
    assert_eq!(vim.text(), "cabc");
}

#[test]
fn a_jump_sets_the_jump_mark() {
    let mut vim = HeadlessVim::new("ab\ncd ef", (0, 1));
    vim.feed("gsef").unwrap();
    assert_eq!(vim.cursor(), (1, 3));
    assert_eq!(vim.mark('\''), Some((0, 1)));
    vim.feed("``").unwrap();
    assert_eq!(vim.cursor(), (0, 1));
}

#[test]
fn as_an_operator_target() {
    // Forward is inclusive, backward exclusive, as `f` and `F`.
    assert_eq!(feed("one two", (0, 0), "dgstw"), ("wo".into(), (0, 0)));
    assert_eq!(feed("one two", (0, 6), "dgSne"), ("oo".into(), (0, 1)));
    // Across rows, with a register; a delete within a row still writes `1`.
    let mut vim = HeadlessVim::new("ab\ncd ef", (0, 1));
    vim.feed("\"ydgsef").unwrap();
    assert_eq!(vim.text(), "af");
    assert_eq!(vim.registers().get('y').unwrap().text, "b\ncd e");
    let mut vim = HeadlessVim::new("ab cd", (0, 0));
    vim.feed("dgscd").unwrap();
    assert_eq!(vim.text(), "d");
    assert_eq!(vim.registers().get('1').unwrap().text, "ab c");
    // `c` and `y`.
    assert_eq!(feed("ab cd", (0, 0), "cgscdX<Esc>").0, "Xd");
    let mut vim = HeadlessVim::new("ab cd", (0, 4));
    vim.feed("ygSb ").unwrap();
    assert_eq!(vim.registers().get('0').unwrap().text, "b c");
    assert_eq!(vim.cursor(), (0, 1));
}

#[test]
fn dot_finds_the_match_again() {
    // Several matches after an operator wait for a label: `s` is the
    // nearest.
    assert_eq!(feed("1ab2ab3ab4", (0, 0), "dgsabs.").0, "b3ab4");
    assert_eq!(feed("1ab2ab3ab4ab5", (0, 0), "dgsabs2.").0, "b4ab5");
    assert_eq!(feed("1ab2ab3ab4", (0, 9), "dgSabs.").0, "1ab24");
    // Undone as one change.
    assert_eq!(feed("1ab2ab3ab4", (0, 0), "dgsabs.u").0, "b2ab3ab4");
}

#[test]
fn from_a_visual_row_end_the_counted_jump_is_the_nth_target() {
    let text = "abc\nabc\nxc";
    let rope = Text::new(text);
    for mode in ["v", "V", "<C-v>"] {
        for (row, forward) in [(1, false), (1, true), (0, true), (2, false)] {
            let mut vim = HeadlessVim::new(text, (row, 0));
            vim.feed(&format!("{mode}$")).unwrap();
            let caret = vim.state().visual_cursor(vim.model()).unwrap();
            assert_eq!(caret, rope.row_start(row) + rope.row_len(row) as usize);
            let found = targets(&rope, 0..rope.row_count(), caret, ['c', ' '], forward);
            assert!(!found.is_empty(), "{mode} {row}");
            for (ix, &target) in found.iter().chain([&usize::MAX]).enumerate() {
                let mut vim = HeadlessVim::new(text, (row, 0));
                let keys = format!("{mode}${}{}c ", ix + 1, if forward { "gs" } else { "gS" });
                vim.feed(&keys).unwrap();
                let moved = vim.state().visual_cursor(vim.model()).unwrap();
                match target {
                    usize::MAX => {
                        assert!(vim.state().failed(), "{keys} from {row}");
                        assert_eq!(moved, caret, "{keys} from {row}");
                    }
                    target => assert_eq!(moved, target, "{keys} from {row}"),
                }
            }
        }
    }
    // `''` is set only when it lands.
    let mut vim = HeadlessVim::new(text, (1, 0));
    vim.feed("v$gSc ").unwrap();
    assert_eq!((vim.cursor(), vim.mark('\'')), ((1, 2), Some((1, 3))));
    let mut vim = HeadlessVim::new(text, (1, 0));
    vim.feed("v$3gSc ").unwrap();
    assert_eq!((vim.cursor(), vim.mark('\'')), ((1, 3), None));
}

#[test]
fn visual_modes_extend_to_it() {
    let mut vim = HeadlessVim::new("ab cd\nef cd", (0, 1));
    vim.feed("vgscd").unwrap();
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((0, 3), Some((0, 1))));
    vim.feed("gscdd").unwrap();
    assert_eq!(vim.text(), "ad");
    let mut vim = HeadlessVim::new("ab cd\nef cd", (1, 4));
    vim.feed("VgSb d").unwrap();
    assert_eq!(vim.text(), "");
    let mut vim = HeadlessVim::new("abcd\nefgh\nijkl", (0, 1));
    vim.feed("<C-v>gsklx").unwrap();
    assert_eq!(vim.text(), "ad\neh\nil");
}

/// A small seeded generator, so the property tests need no crate.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick(&mut self, from: &str) -> char {
        let chars: Vec<char> = from.chars().collect();
        chars[self.below(chars.len())]
    }
}

/// Every count up to a few pages, with and without an operator.
fn labelings() -> impl Iterator<Item = (usize, bool)> {
    (0..=4 * FULL.chars().count()).flat_map(|count| [(count, false), (count, true)])
}

#[test]
fn labeler_gives_labels_unique_within_a_page() {
    for (count, op) in labelings() {
        let labeled = label(count, op);
        assert_eq!(labeled.labels.len(), count, "{count} {op}");
        let labels: Vec<_> = labeled.labels.iter().flatten().collect();
        let unique: HashSet<_> = labels.iter().map(|l| (l.page, l.ch)).collect();
        assert_eq!(unique.len(), labels.len(), "{count} {op}");
    }
}

#[test]
fn labeler_takes_the_safe_set_when_it_autojumps_and_the_full_set_otherwise() {
    for (count, op) in labelings() {
        let labeled = label(count, op);
        let set = if labeled.autojump { SAFE } else { FULL };
        for label in labeled.labels.iter().flatten() {
            assert!(set.contains(label.ch), "{count} {op}: {label:?}");
            if labeled.autojump {
                assert_eq!(label.page, 0, "{count} {op}");
            }
        }
        // It autojumps exactly when everything after the first fits.
        let fits = count > 0 && count - 1 <= SAFE.chars().count();
        assert_eq!(labeled.autojump, fits && !(op && count > 1), "{count} {op}");
    }
}

#[test]
fn labeler_never_autojumps_after_an_operator_with_more_than_one_target() {
    for (count, op) in labelings() {
        if op && count > 1 {
            assert!(!label(count, op).autojump, "{count}");
        }
    }
    assert!(label(1, true).autojump);
}

#[test]
fn labeler_reaches_every_target_through_its_pages() {
    for (count, op) in labelings() {
        let labeled = label(count, op);
        for (ix, label) in labeled.labels.iter().enumerate() {
            // Only the first goes unlabelled, and only when jumped to.
            assert_eq!(
                label.is_none(),
                ix == 0 && labeled.autojump,
                "{count} {op} {ix}"
            );
        }
        // Pages fill in order, each but the last full.
        let pages = labeled.labels.iter().flatten().map(|l| l.page).max();
        let full = FULL.chars().count();
        if let Some(last) = pages {
            for page in 0..last {
                let on = labeled.labels.iter().flatten().filter(|l| l.page == page);
                assert_eq!(on.count(), full, "{count} {op} page {page}");
            }
        }
    }
}

#[test]
fn labeler_gives_the_same_result_for_the_same_input() {
    for (count, op) in labelings() {
        assert_eq!(label(count, op), label(count, op), "{count} {op}");
    }
}

/// The chars the random texts are made of, by case class: each group is
/// one letter's case variants.
const CLASSES: [&str; 9] = [
    "aA",
    "b",
    "=",
    "sSſ",
    "σςΣ",
    "kK\u{212a}",
    "\u{390}\u{1fd3}",
    "ı",
    "iI",
];

/// The matches of `chars` in `text` (made of [`CLASSES`], blanks and line
/// breaks), the slow way, as byte offsets, first to last.
fn reference(text: &str, [c1, c2]: [char; 2]) -> Vec<usize> {
    let class = |ch: char| CLASSES.iter().position(|class| class.contains(ch));
    let folds = |a: char, b: char| a == b || class(a).is_some() && class(a) == class(b);
    let takes = |ch: char, at: Option<char>| match (ch, at) {
        (' ', None | Some(' ' | '\t')) => true,
        (_, Some(at)) => folds(at, ch),
        (_, None) => false,
    };
    let same = folds(c1, c2);
    let mut found = Vec::new();
    let mut start = 0;
    for row in text.split('\n') {
        let chars: Vec<(usize, char)> = row.char_indices().collect();
        if chars.is_empty() && c1 == ' ' && c2 == ' ' {
            found.push(start);
        }
        for ix in 0..chars.len() {
            if takes(c1, Some(chars[ix].1))
                && takes(c2, chars.get(ix + 1).map(|&(_, ch)| ch))
                && !(same && ix > 0 && takes(c1, Some(chars[ix - 1].1)))
            {
                found.push(start + chars[ix].0);
            }
        }
        start += row.len() + 1;
    }
    found
}

#[test]
fn labeler_targets_are_the_matches_inside_the_rows_nearest_first() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..3000 {
        let len = rng.below(40);
        let typed: String = CLASSES.concat() + " \t";
        let all = typed.clone() + "\n";
        let text: String = (0..len).map(|_| rng.pick(&all)).collect();
        let chars = [rng.pick(&typed), rng.pick(&typed)];
        let rope = Text::new(&text);
        let rows = rope.row_count();
        let first = rng.below(rows as usize + 1) as u32;
        let end = first + rng.below(rows as usize + 2 - first as usize) as u32;
        let carets: Vec<usize> = text.char_indices().map(|(ix, _)| ix).collect();
        let caret = match rng.below(carets.len() + 1) {
            ix if ix == carets.len() => text.len(),
            ix => carets[ix],
        };
        let forward = rng.below(2) == 0;
        let got = targets(&rope, first..end, caret, chars, forward);

        let row = |offset: usize| rope.offset_to_point(offset).row;
        let mut want: Vec<usize> = reference(&text, chars)
            .into_iter()
            .filter(|&offset| (first..end).contains(&row(offset)))
            .filter(|&offset| match forward {
                true => offset > caret,
                false => offset < caret,
            })
            .collect();
        if !forward {
            want.reverse();
        }
        assert_eq!(
            got, want,
            "case {case}: {text:?} rows {first}..{end} caret {caret} {chars:?} forward {forward}"
        );
    }
}

/// `rows` rows, each `row`.
fn rows(count: usize, row: &str) -> String {
    vec![row; count].join("\n")
}

/// `text` from `cursor` in a view of `window` rows, after `keys`.
fn session(text: &str, cursor: (u32, usize), window: u32, keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.set_window_rows(window);
    vim.feed(keys).unwrap();
    vim
}

/// The labels shown, as (row, column, label, state).
fn shown(vim: &HeadlessVim) -> Vec<((u32, usize), String, JumpLabelState)> {
    let text = vim.model().text();
    let overlay = vim.jump_overlay().unwrap_or_default();
    overlay
        .labels
        .into_iter()
        .map(|label| {
            let point = text.offset_to_point(label.offset);
            let at = (point.row, point.column as usize);
            (at, label.text, label.state)
        })
        .collect()
}

/// The labels shown live, as (row, column, label).
fn live(vim: &HeadlessVim) -> Vec<((u32, usize), String)> {
    shown(vim)
        .into_iter()
        .filter(|(_, _, state)| *state == JumpLabelState::Live)
        .map(|(at, text, _)| (at, text))
        .collect()
}

#[test]
fn session_autojumps_to_the_nearest_and_labels_the_rest_in_view() {
    // Ten matches in view after the caret (one a row, in rows 0 to 9), 30
    // more below it.
    let text = rows(40, "x ab");
    let vim = session(&text, (0, 0), 10, "gsab");
    assert_eq!(vim.cursor(), (0, 2));
    assert_eq!(vim.mark('\''), Some((0, 0)));
    assert_eq!(vim.state().pending().kind(), Some(crate::PendingKind::Jump));
    let safe: Vec<String> = SAFE.chars().take(9).map(String::from).collect();
    let want: Vec<_> = (1..10).map(|row| (row, 2)).zip(safe).collect();
    assert_eq!(live(&vim), want);
    // Nothing outside the view is labelled.
    assert!(shown(&vim).iter().all(|((row, _), _, _)| *row < 10));

    // A label goes to its target; `''` is still where the jump started.
    let mut vim = session(&text, (0, 0), 10, "gsabn");
    assert_eq!(vim.cursor(), (3, 2));
    assert_eq!(vim.mark('\''), Some((0, 0)));
    assert_eq!(vim.jump_overlay(), None);
    vim.feed("``").unwrap();
    assert_eq!(vim.cursor(), (0, 0));

    // A key that isn't a label falls through and acts as itself.
    let vim = session(&text, (0, 0), 10, "gsabx");
    assert_eq!(vim.text().lines().next(), Some("x b"));
    assert_eq!((vim.cursor(), vim.mode()), ((0, 2), "normal"));
    assert_eq!(vim.jump_overlay(), None);
    let vim = session(&text, (0, 0), 10, "gsabj");
    assert_eq!(vim.cursor(), (1, 2));

    // `<Esc>` leaves the caret on the first target.
    let vim = session(&text, (0, 0), 10, "gsab<Esc>");
    assert_eq!((vim.cursor(), vim.text()), ((0, 2), text.clone()));
    assert!(!vim.state().failed());
    assert_eq!(vim.jump_overlay(), None);

    // One target goes straight there, with no session.
    let vim = session("x ab\nx cd", (0, 0), 10, "gsab");
    assert_eq!((vim.cursor(), vim.jump_overlay()), ((0, 2), None));
}

#[test]
fn session_a_normal_starts_ends_with_it_unrecorded() {
    // `:normal` ends the session its keys leave after an autojump, and a
    // recording keeps the `:normal` line, not a jump its keys made.
    let text = rows(40, "x ab");
    let mut vim = session(&text, (0, 0), 10, "qa:normal gsab<CR>q");
    assert_eq!((vim.cursor(), vim.jump_overlay()), ((0, 2), None));
    assert_eq!(vim.state().pending().kind(), None);
    let recorded = vim.registers().get('a').unwrap().text.clone();
    assert_eq!(recorded, ":normal gsab\r");
    vim.feed("j0@a").unwrap();
    assert_eq!(vim.cursor(), (1, 2));
}

#[test]
fn session_with_more_matches_keeps_the_caret_and_labels_from_the_full_set() {
    // Six matches a row: 59 in view after the caret, more than the full
    // set, so two pages.
    let text = rows(40, "ab ab ab ab ab ab");
    let vim = session(&text, (0, 0), 10, "gsab");
    assert_eq!(vim.cursor(), (0, 0));
    assert_eq!(vim.mark('\''), None);
    let full: Vec<char> = FULL.chars().collect();
    let on_page = |vim: &HeadlessVim, state| {
        shown(vim)
            .into_iter()
            .filter(|(_, _, s)| *s == state)
            .count()
    };
    assert_eq!(on_page(&vim, JumpLabelState::Live), full.len());
    assert_eq!(on_page(&vim, JumpLabelState::Concealed), 59 - full.len());
    let first = &live(&vim)[..3];
    let want = [((0, 3), "s"), ((0, 6), "f"), ((0, 9), "n")];
    assert_eq!(first, want.map(|(at, l)| (at, l.to_string())));
    assert!(shown(&vim).iter().all(|((row, _), _, _)| *row < 10));

    // `<Space>` pages forward (round to the first), `<BS>` back.
    let mut vim = session(&text, (0, 0), 10, "gsab ");
    assert_eq!(on_page(&vim, JumpLabelState::Live), 59 - full.len());
    // The 55th target, row 9's second match, is page 2's first.
    assert_eq!(live(&vim)[0], ((9, 3), "s".to_string()));
    vim.feed("<BS>").unwrap();
    assert_eq!(live(&vim)[0], ((0, 3), "s".to_string()));
    vim.feed("<BS>f").unwrap();
    assert_eq!(vim.cursor(), (9, 6));
    let vim = session(&text, (0, 0), 10, "gsab  f");
    assert_eq!(vim.cursor(), (0, 6));

    // A key that isn't a label cancels, with nothing changed.
    let vim = session(&text, (0, 0), 10, "gsab1");
    assert_eq!((vim.cursor(), vim.text()), ((0, 0), text.clone()));
    assert!(vim.state().failed());
    assert_eq!(vim.jump_overlay(), None);
    // As does `<Esc>`, quietly.
    let vim = session(&text, (0, 0), 10, "gsab<Esc>");
    assert_eq!((vim.cursor(), vim.text()), ((0, 0), text.clone()));
    assert!(!vim.state().failed());
    assert_eq!((vim.mark('\''), vim.mode()), (None, "normal"));
}

#[test]
fn session_with_no_match_in_view_fails() {
    // The motion would reach row 20; the session sees only the view.
    let mut text = rows(40, "x");
    text.replace_range(text.len() - 1.., "ab");
    let vim = session(&text, (0, 0), 10, "gsab");
    assert!(vim.state().failed());
    assert_eq!((vim.cursor(), vim.mark('\'')), ((0, 0), None));
    assert_eq!(session(&text, (0, 0), 10, "1gsab").cursor(), (39, 0));
}

#[test]
fn session_enter_as_the_first_char_repeats_the_last_jump() {
    let text = rows(6, "x ab");
    let mut vim = session(&text, (0, 0), 10, "gsab<Esc>");
    assert_eq!(vim.cursor(), (0, 2));
    vim.feed("gs<CR>").unwrap();
    assert_eq!(vim.cursor(), (1, 2));
    assert_eq!(vim.state().jump_query(), Some("ab"));
    // In the last jump's direction, whichever key started it.
    vim.feed("<Esc>gS<CR><Esc>").unwrap();
    assert_eq!(vim.cursor(), (2, 2));
    // Counted too.
    vim.feed("2gs<CR>").unwrap();
    assert_eq!(vim.cursor(), (4, 2));
    // With no jump before, it cancels.
    let vim = session(&text, (0, 0), 10, "gs<CR>");
    assert!(vim.state().failed());
    assert_eq!((vim.cursor(), vim.jump_overlay()), ((0, 0), None));
}

#[test]
fn operator_never_autojumps_and_a_label_acts_through_its_target() {
    let text = "1ab2ab3ab4";
    let mut vim = session(text, (0, 0), 10, "dgsab");
    assert_eq!((vim.text(), vim.cursor()), (text.to_string(), (0, 0)));
    assert_eq!(vim.mode(), "operator_pending");
    let want = [((0, 1), "s"), ((0, 4), "f"), ((0, 7), "n")];
    assert_eq!(live(&vim), want.map(|(at, l)| (at, l.to_string())));
    // Forward takes the target too.
    vim.feed("f").unwrap();
    assert_eq!((vim.text(), vim.cursor()), ("b3ab4".to_string(), (0, 0)));
    assert_eq!(vim.registers().get('1').unwrap().text, "1ab2a");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), text);
    // Backward stops short of it.
    let vim = session(text, (0, 9), 10, "dgSabf");
    assert_eq!(vim.text(), "1ab24");
    // With a register; a single target needs no label.
    let vim = session(text, (0, 0), 10, "\"xdgSab");
    assert_eq!(vim.text(), text);
    let vim = session(text, (0, 0), 10, "\"xdgsabn");
    assert_eq!(vim.text(), "b4");
    assert_eq!(vim.registers().get('x').unwrap().text, "1ab2ab3a");
    let vim = session("1ab2", (0, 0), 10, "dgsab");
    assert_eq!(vim.text(), "b2");
    // `c` and `y`.
    assert_eq!(session(text, (0, 0), 10, "cgsabfX<Esc>").text(), "Xb3ab4");
    let vim = session(text, (0, 0), 10, "ygsabn");
    assert_eq!(vim.registers().get('0').unwrap().text, "1ab2ab3a");
    // `<Esc>` and a key that isn't a label change nothing.
    for keys in ["dgsab<Esc>", "dgsab1"] {
        let vim = session(text, (0, 0), 10, keys);
        assert_eq!((vim.text(), vim.mode()), (text.to_string(), "normal"));
    }
}

#[test]
fn operator_dot_repeats_the_counted_jump_from_the_new_caret() {
    let mut vim = session("1ab2ab3ab4ab5ab6", (0, 0), 10, "dgsabf");
    assert_eq!(vim.text(), "b3ab4ab5ab6");
    let change = vim.state().last_change().cloned();
    vim.feed(".").unwrap();
    assert_eq!(vim.text(), "b5ab6");
    // Its record is the counted jump.
    let mut counted = session("1ab2ab3ab4ab5ab6", (0, 0), 10, "d2gsab");
    assert_eq!(counted.text(), "b3ab4ab5ab6");
    assert_eq!(
        format!("{:?}", counted.state().last_change()),
        format!("{change:?}")
    );
    counted.feed(".").unwrap();
    assert_eq!(counted.text(), "b5ab6");
}

#[test]
fn operator_visual_label_extends_the_selection() {
    let text = rows(4, "ab");
    // An autojump, then a label from the safe set.
    let mut vim = session(&text, (0, 0), 10, "vgsab");
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((1, 0), Some((0, 0))));
    vim.feed("f").unwrap();
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((3, 0), Some((0, 0))));
    assert_eq!(vim.mode(), "visual");
    assert_eq!(vim.mark('\''), Some((0, 0)));
    let vim = session(&text, (0, 0), 10, "Vgsabsd");
    assert_eq!(vim.text(), "ab");
    // With more targets than safe labels, the caret stays.
    let text = rows(10, "ab ab");
    let mut vim = session(&text, (0, 0), 10, "vgsab");
    assert_eq!((vim.cursor(), vim.mode()), ((0, 0), "visual"));
    vim.feed("n").unwrap();
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((1, 3), Some((0, 0))));
    let vim = session(&text, (2, 3), 10, "VgSabfd");
    assert_eq!(vim.text(), rows(8, "ab ab"));
}

#[test]
fn overlay_shows_labels_only_after_c2() {
    let mut vim = session("ab ab ab ac", (0, 0), 10, "");
    assert_eq!(vim.jump_overlay(), None);
    assert_eq!(vim.state().jump_query(), None);
    vim.feed("gs").unwrap();
    assert_eq!(vim.state().jump_query(), Some(""));
    assert_eq!(shown(&vim), []);
    // After `{c1}`, nothing.
    vim.feed("a").unwrap();
    assert_eq!(vim.state().jump_query(), Some("a"));
    assert_eq!(shown(&vim), []);
    // After an operator, `ab` takes the full set and doesn't autojump.
    let mut vim = session("ab ab ab ac", (0, 0), 10, "dgsa");
    assert_eq!(shown(&vim), []);
    vim.feed("b").unwrap();
    assert_eq!(vim.state().jump_query(), Some("ab"));
    let live = |at, l: &str| (at, l.to_string(), JumpLabelState::Live);
    assert_eq!(shown(&vim), [live((0, 3), "s"), live((0, 6), "f")]);
    vim.feed("<Esc>").unwrap();
    assert_eq!((vim.jump_overlay(), vim.state().jump_query()), (None, None));
    // A blank or the row's end is `<Space>`'s group.
    let vim = session("a\nab a\ta", (0, 0), 10, "dgSa");
    assert_eq!(vim.jump_overlay(), Some(Default::default()));
}

#[test]
fn macro_records_resolved_jump() {
    // A labelled jump records as the counted one.
    let text = rows(20, "ab ab ab ab ab");
    let mut vim = session(&text, (9, 12), 10, "qqgSabkq");
    assert_eq!(vim.cursor(), (8, 12));
    assert_eq!(vim.registers().get('q').unwrap().text, "5gSab");
    // Replayed from the same place in another view, which would autojump
    // and make `k` a move, it lands on the same match.
    vim.feed("``<C-e><C-e><C-e><C-e><C-e><C-e><C-e><C-e>")
        .unwrap();
    assert_eq!((vim.cursor(), vim.top_row()), ((9, 12), 8));
    vim.feed("@q").unwrap();
    assert_eq!(vim.cursor(), (8, 12));
    let mut typed = session(
        &text,
        (9, 12),
        10,
        "``<C-e><C-e><C-e><C-e><C-e><C-e><C-e><C-e>",
    );
    typed.feed("gSabk").unwrap();
    assert_ne!(typed.cursor(), (8, 12));

    // Paging keys and an `<Esc>` after an autojump are dropped, a key
    // falling through is kept, and so are the keys around the jump.
    let wide = rows(40, "ab ab ab ab ab ab");
    for (text, keys, want) in [
        (rows(10, "x ab"), "qqgsab<Esc>q", "1gsab"),
        (rows(10, "x ab"), "qqgsabnq", "4gsab"),
        (rows(10, "x ab"), "qqgsabxjq", "1gsabxj"),
        (wide.clone(), "qqgsab  <BS>fq", "56gsab"),
        (wide.clone(), "qqdgsabfq", "d2gsab"),
        (wide.clone(), "qq\"adgsab<Esc>2gsabq", "2gsab"),
        (wide.clone(), "qqvgsabflq", "v2gsabl"),
        (wide.clone(), "qq3gsabq", "3gsab"),
        // `<CR>` records the chars it stood for.
        (rows(10, "x ab"), "gsab<Esc>qqgs<CR>q", "1gsab"),
        (rows(10, "x ab"), "gsab<Esc>qq2gs<CR>q", "2gsab"),
    ] {
        let vim = session(&text, (0, 0), 10, keys);
        let recorded = vim.registers().get('q').map(|r| r.text.clone());
        assert_eq!(recorded.as_deref(), Some(want), "{keys}");
    }

    // A cancelled jump leaves none of its keys.
    for keys in [
        "qqgsab<Esc>q",
        "qqgsab1q",
        "qqdgsab<Esc>q",
        "qq\"a3dgsa<Esc>q",
        "qqgs<Esc>q",
    ] {
        let vim = session(&wide, (0, 0), 10, keys);
        let recorded = vim.registers().get('q').map(|r| r.text.clone());
        assert_eq!(recorded.as_deref().unwrap_or(""), "", "{keys}");
    }
    let vim = session(&wide, (0, 0), 10, "qqxgsab<Esc>lq");
    assert_eq!(vim.registers().get('q').unwrap().text, "xl");
}

#[test]
fn opt_in_s() {
    let text = "ab x ab\ncd ab";
    let opted = |cursor, keys: &str| {
        let mut vim = HeadlessVim::new(text, cursor);
        vim.set_jump_on_s(true);
        vim.feed(keys).unwrap();
        vim
    };
    // As `gs` and `gS`, in normal mode, a visual mode, after an operator,
    // with a count and labels.
    assert_eq!(opted((0, 0), "sab").cursor(), (0, 5));
    assert_eq!(opted((0, 0), "sabs").cursor(), (1, 3));
    assert_eq!(opted((1, 3), "Sab").cursor(), (0, 5));
    assert_eq!(opted((0, 0), "2sab").cursor(), (1, 3));
    assert_eq!(opted((0, 0), "dsabs").text(), "b\ncd ab");
    assert_eq!(opted((0, 0), "dsabf").text(), "b");
    assert_eq!(opted((1, 3), "dSabs").text(), "ab x ab");
    let vim = opted((0, 0), "vsab");
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((0, 5), Some((0, 0))));
    let vim = opted((1, 3), "VSabd");
    assert_eq!(vim.text(), "");
    // `gs` still jumps; `s` after a prefix is that prefix's.
    assert_eq!(opted((0, 0), "gsab").cursor(), (0, 5));
    assert_eq!(opted((0, 0), "fs").cursor(), (0, 0));
    assert_eq!(opted((0, 0), "rs").text(), "sb x ab\ncd ab");

    // Off by default: `s` `S` `v_s` `v_S` are what they were.
    let plain = |keys: &str| session(text, (0, 0), 10, keys).text();
    assert_eq!(plain("sX<Esc>"), "Xb x ab\ncd ab");
    assert_eq!(plain("SX<Esc>"), "X\ncd ab");
    assert_eq!(plain("vlsX<Esc>"), "X x ab\ncd ab");
    assert_eq!(plain("vSX<Esc>"), "X\ncd ab");
    assert_eq!(session(text, (0, 0), 10, "gsab").cursor(), (0, 5));
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.set_jump_on_s(true);
    vim.set_jump_on_s(false);
    vim.feed("sX<Esc>").unwrap();
    assert_eq!(vim.text(), "Xb x ab\ncd ab");
}

/// Another origin's edit, as an agent's would come in.
fn edit_elsewhere(vim: &HeadlessVim, range: std::ops::Range<usize>, text: &str) {
    let ai = Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    vim.model().split().buffer_mut().edit([(range, text)], ai);
}

#[test]
fn session_labels_go_stale_when_another_origin_edits() {
    // Each session from the text's start, the edit made while its labels
    // wait, and the label then typed: it fails with nothing more changed,
    // and a recording keeps only an autojump that went ahead.
    type Case<'a> = (
        &'a str,
        &'a str,
        &'a str,
        std::ops::Range<usize>,
        &'a str,
        &'a str,
    );
    let wide = rows(10, "ab ab");
    let cases: [Case; 10] = [
        // After an operator: an insertion before the targets, a deletion.
        ("x ab ab", "dgsab", "XX ", 2..2, "f", ""),
        ("x ab ab", "dgsab", "", 0..1, "f", ""),
        // No autojump: an insertion at the caret's row, a deletion.
        (&wide, "gsab", "Z", 0..0, "f", ""),
        (&wide, "gsab", "", 0..1, "f", ""),
        // After an autojump: an insertion, a deletion.
        ("x ab\nx ab\nx ab", "gsab", "QQ", 5..5, "s", "1gsab"),
        ("x ab\nx ab\nx ab", "gsab", "", 4..6, "s", "1gsab"),
        // In a visual mode, with and without an autojump: an insertion, a
        // deletion.
        ("ab\nab\nab\nab", "vgsab", "W", 3..3, "f", "v1gsab"),
        ("ab\nab\nab\nab", "vgsab", "", 6..7, "f", "v1gsab"),
        (&wide, "vgsab", "W", 9..9, "n", "v"),
        (&wide, "vgsab", "", 6..7, "n", "v"),
    ];
    for (text, keys, put, range, label, recorded) in cases {
        let mut vim = session(text, (0, 0), 10, &format!("qq{keys}"));
        assert!(!shown(&vim).is_empty(), "{keys} {range:?}");
        edit_elsewhere(&vim, range.clone(), put);
        vim.sync_visual();
        // The labels are no longer shown.
        assert_eq!(shown(&vim), [], "{keys} {range:?}");
        let (edited, at) = (vim.text(), vim.cursor());
        vim.feed(label).unwrap();
        assert!(vim.state().failed(), "{keys} {range:?}");
        assert_eq!((vim.text(), vim.cursor()), (edited, at), "{keys} {range:?}");
        let mode = if keys.starts_with('v') {
            "visual"
        } else {
            "normal"
        };
        assert_eq!(vim.mode(), mode, "{keys} {range:?}");
        vim.feed("q").unwrap();
        let got = vim.registers().get('q').map(|r| r.text.clone());
        assert_eq!(got.as_deref().unwrap_or(""), recorded, "{keys} {range:?}");
    }

    // A caret moved meanwhile, with no edit, makes them stale too.
    let mut vim = session("x ab\nx ab\nx ab", (0, 0), 10, "gsab");
    vim.model_mut().set_cursor(0);
    assert_eq!(shown(&vim), []);
    vim.feed("s").unwrap();
    assert!(vim.state().failed());
    assert_eq!(vim.cursor(), (0, 0));

    // With nothing changed, a picked label still acts.
    let vim = session("x ab ab ab", (0, 0), 10, "dgsabf");
    assert_eq!(vim.text(), "b ab");
}

/// Carets at `offsets`, the last the primary.
fn carets(offsets: &[usize]) -> Vec<ResolvedSelection> {
    offsets
        .iter()
        .map(|&offset| ResolvedSelection {
            range: offset..offset,
            reversed: false,
        })
        .collect()
}

#[test]
fn session_labels_go_stale_when_the_selections_change() {
    // Each session, its keys typed with carets at `from`; then, while its
    // labels wait, a caret before the primary added, removed or moved, the
    // primary where it was. The label typed then fails with nothing
    // changed, and the carets stay as they were set.
    enum Change {
        Add(usize),
        Remove,
        Move(usize),
    }
    let wide = rows(10, "ab ab");
    let four = rows(4, "x ab");
    let cases: [(&str, &[usize], &str, Change, &str); 9] = [
        // After an operator.
        ("ab x ab ab", &[3], "dgsab", Change::Add(0), "f"),
        ("ab x ab ab", &[0, 3], "dgsab", Change::Remove, "f"),
        ("ab x ab ab", &[0, 3], "dgsab", Change::Move(1), "f"),
        // No autojump.
        (&wide, &[3], "gsab", Change::Add(1), "f"),
        (&wide, &[1, 3], "gsab", Change::Remove, "f"),
        (&wide, &[1, 3], "gsab", Change::Move(2), "f"),
        // After an autojump, which moved both carets of two.
        (&four, &[0], "gsab", Change::Add(0), "s"),
        (&four, &[0, 5], "gsab", Change::Remove, "s"),
        (&four, &[0, 5], "gsab", Change::Move(1), "s"),
    ];
    for (text, from, keys, change, label) in cases {
        let mut vim = HeadlessVim::new(text, (0, 0));
        vim.set_window_rows(10);
        vim.model_mut().set_selections(carets(from));
        vim.feed(keys).unwrap();
        assert!(!shown(&vim).is_empty(), "{keys} {from:?}");
        let mut now = vim.model().selections();
        match change {
            Change::Add(offset) => now.insert(0, carets(&[offset]).remove(0)),
            Change::Remove => drop(now.remove(0)),
            Change::Move(offset) => now[0] = carets(&[offset]).remove(0),
        }
        vim.model_mut().set_selections(now.clone());
        assert_eq!(vim.model().selections(), now, "{keys} {from:?}");
        assert_eq!(shown(&vim), [], "{keys} {from:?}");
        let at = vim.cursor();
        vim.feed(label).unwrap();
        assert!(vim.state().failed(), "{keys} {from:?}");
        assert_eq!(vim.text(), text, "{keys} {from:?}");
        assert_eq!(vim.cursor(), at, "{keys} {from:?}");
        assert_eq!(vim.model().selections(), now, "{keys} {from:?}");
    }
}
