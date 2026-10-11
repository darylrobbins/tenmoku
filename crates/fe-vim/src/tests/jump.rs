//! Tests of the jump feature: `gs` and `gS` as a motion, and the pure
//! matcher and labeler.
use std::collections::HashSet;

use fe_command::ContextFrame;
use fe_text::Text;

use super::{commit, invocation, run};
use crate::{
    HeadlessVim, JumpChars, JumpMotion, Motion, MoveArgs, OperateArgs, Operator, Step, Target,
    VimKey, VimState,
    jump::label::{FULL, SAFE, label, targets},
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
    assert_eq!(run("gq"), Step::Cancel);
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
    assert_eq!(feed("1ab2ab3ab4", (0, 0), "dgsab.").0, "b3ab4");
    assert_eq!(feed("1ab2ab3ab4ab5", (0, 0), "dgsab2.").0, "b4ab5");
    assert_eq!(feed("1ab2ab3ab4", (0, 9), "dgSab.").0, "1ab24");
    // Undone as one change.
    assert_eq!(feed("1ab2ab3ab4", (0, 0), "dgsab.u").0, "b2ab3ab4");
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
