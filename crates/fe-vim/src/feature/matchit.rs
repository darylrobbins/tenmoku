//! matchit's `%` and `g%` over keyword groups (`:help matchit`), a feature
//! module; see [`crate::feature`].
//!
//! `%` without a count goes through a keyword group of the buffer's
//! language (`if`, `elseif`, `else`, `end` in Lua) as nvim's matchit does
//! with the language's `b:match_words`; `g%` goes the other way. The
//! language comes from the host ([`EditorHost::language_name`]), which
//! [`line_motions::with_view`] asks for before `%` and `g%`, and only then;
//! [`motion::match_pair`] reads its [`Table`] before it looks for a
//! bracket. With no language, or one without a table, `%` is nvim's
//! built-in one and `g%` fails.
//!
//! In html, xml and markdown `%` and `g%` go through tags instead, `<` and
//! `>`, and `<!--` and `-->` ([`tags`]).
//!
//! The tables are nvim 0.12.4's `b:match_words` for `lua`, `sh` (and
//! `bash`) and `make`, written out as words, not Vim patterns. Probed in
//! nvim 0.12.4 (`nvim --clean --cmd 'packadd matchit' --cmd 'filetype
//! plugin on'`, no syntax, so `b:match_skip` skips nothing):
//!
//! - From the keyword under the caret (anywhere on it), or else the first
//!   one at or after the caret on its row, `%` goes to the group's next
//!   word, the middles and the closer, and from the closer back to the
//!   opener; `g%` from the opener goes to the closer, and from elsewhere to
//!   the word before, back to the opener. A group nested in between is
//!   passed over whole, its middles too, as `searchpair()` passes it. Each
//!   lands on the word's first char; it doesn't wrap around the buffer.
//! - Keywords count wherever the group's place allows them, in strings and
//!   comments too: Lua's anywhere as whole words; sh's at the row's start
//!   after blanks, or after `;` and blanks; make's conditionals at the
//!   row's start after spaces, and nmake's after a `!` there, both landing
//!   on the row's first column (the match takes the spaces).
//! - A bracket on the row before that first keyword wins: `%` matches it
//!   as before. With no keyword on the row, `%` is as before.
//! - A keyword with no partner (an `end` alone) fails the motion.
//! - After an operator a word matched ahead of the caret is taken whole
//!   (`d%` from `if` takes its `end`), charwise and inclusive; one behind is
//!   taken from its first char.
//!
//! Limits: Lua's long brackets (`[[ ]]`, `[==[ ]==]`) aren't a group here,
//! so `%` on one matches its inner bracket. matchit sets `''` when it finds
//! a keyword even if `searchpair()` then fails; here a failing `%` or `g%`
//! leaves it. A delete over `%` still writes register `1` (as nvim's
//! built-in `%` does, not matchit's mapping); one over `g%` doesn't.
//! On a C preprocessor row `g%` goes through `#if`, `#else` and `#endif`
//! the other way, a jump over rows as `%`'s is, where matchit's is
//! charwise.
//!
//! [`EditorHost::language_name`]: fe_editor::commands::EditorHost::language_name
//! [`line_motions::with_view`]: crate::line_motions::with_view
//! [`motion::match_pair`]: crate::motion::match_pair

use std::ops::Range;

use fe_text::Text;

use crate::{
    Pending, Step, VimKey,
    args::Motion,
    feature::Feature,
    motion::{self, PAIRS},
};

pub(crate) mod tags;

use tags::Dialect;

pub(crate) const FEATURE: Feature = Feature { g, ..Feature::NONE };

/// `%` after `g`; `{count}g%` is `{count}%`, as matchit runs it.
fn g(pending: &Pending, key: VimKey) -> Option<Step> {
    if key != VimKey::Char('%') {
        return None;
    }
    Some(pending.clone().motion(match pending.count() {
        Some(_) => Motion::MatchPair,
        None => Motion::MatchPairBackward,
    }))
}

/// Where a group's words count, as its `b:match_words` patterns place them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// Anywhere, as a whole word (`\<if\>`).
    Word,
    /// Where a command starts: after the row's leading blanks, or after a
    /// `;` and blanks (`\%(;\s*\|^\s*\)\@<=if\>`).
    Command,
    /// After the row's leading spaces, which the match takes (`^ *ifeq\>`).
    RowStart,
    /// After a `!` in the row's first column and blanks, which the match
    /// takes (`^!\s*if\>`).
    Bang,
}

/// One group of `b:match_words`: its openers, middles and closer. A space
/// in a word stands for one or more blanks (`else ifeq`).
struct Group {
    place: Place,
    open: &'static [&'static str],
    middle: &'static [&'static str],
    close: &'static [&'static str],
}

/// A language's keyword groups, or its tags.
pub(crate) struct Table {
    groups: &'static [Group],
    tags: Option<Dialect>,
}

/// Lua's: `\<\%(do\|function\|if\)\>:\<\%(return\|else\|elseif\)\>:\<end\>`
/// and `\<repeat\>:\<until\>`.
const LUA: Table = Table {
    groups: &[
        Group {
            place: Place::Word,
            open: &["do", "function", "if"],
            middle: &["return", "else", "elseif"],
            close: &["end"],
        },
        Group {
            place: Place::Word,
            open: &["repeat"],
            middle: &[],
            close: &["until"],
        },
    ],
    tags: None,
};

/// sh's (and bash's): `if:elif:else:fi`, `for` or `while` to `done`, and
/// `case:esac`, each where a command starts.
const SH: Table = Table {
    groups: &[
        Group {
            place: Place::Command,
            open: &["if"],
            middle: &["elif", "else"],
            close: &["fi"],
        },
        Group {
            place: Place::Command,
            open: &["for", "while"],
            middle: &[],
            close: &["done"],
        },
        Group {
            place: Place::Command,
            open: &["case"],
            middle: &[],
            close: &["esac"],
        },
    ],
    tags: None,
};

/// make's: `^ *ifn\=\(eq\|def\)\>:^ *else\(\s\+ifn\=\(eq\|def\)\)\=\>:^ *endif\>`,
/// `\<define\>:\<endef\>`, and nmake's
/// `^!\s*if\(n\=def\)\=\>:^!\s*else\(if\(n\=def\)\=\)\=\>:^!\s*endif\>`.
const MAKE: Table = Table {
    groups: &[
        Group {
            place: Place::RowStart,
            open: &["ifeq", "ifneq", "ifdef", "ifndef"],
            middle: &[
                "else ifeq",
                "else ifneq",
                "else ifdef",
                "else ifndef",
                "else",
            ],
            close: &["endif"],
        },
        Group {
            place: Place::Word,
            open: &["define"],
            middle: &[],
            close: &["endef"],
        },
        Group {
            place: Place::Bang,
            open: &["if", "ifdef", "ifndef"],
            middle: &["else", "elseif", "elseifdef", "elseifndef"],
            close: &["endif"],
        },
    ],
    tags: None,
};

/// html's and markdown's tags (markdown's ftplugin sources html's).
const HTML: Table = Table {
    groups: &[],
    tags: Some(Dialect::Html),
};

/// xml's tags.
const XML: Table = Table {
    groups: &[],
    tags: Some(Dialect::Xml),
};

/// The keyword table of the language fe-syntax calls `language`, if it
/// has one.
pub(crate) fn table(language: &str) -> Option<&'static Table> {
    match language {
        "lua" => Some(&LUA),
        "bash" => Some(&SH),
        "make" => Some(&MAKE),
        "html" | "markdown" => Some(&HTML),
        "xml" => Some(&XML),
        _ => None,
    }
}

/// Which word of its group a keyword is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Open,
    Middle,
    Close,
}

/// A keyword on a row: its bytes in the row, its group and its part.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Hit {
    span: Range<usize>,
    group: usize,
    part: Part,
}

/// What `%` or `g%` makes of the keywords around the caret.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Keyword {
    /// No keyword decides it: the caret's row has none at or after it, or
    /// a bracket comes first.
    None,
    /// It lands on the word at these bytes.
    Lands(Range<usize>),
    /// The keyword has no partner that way.
    Fails,
}

/// Where `%` (or `g%`, `backward`) goes from `offset` through `table`'s
/// groups.
pub(crate) fn find(text: &Text, offset: usize, table: &Table, backward: bool) -> Keyword {
    if let Some(dialect) = table.tags {
        return tags::find(text, offset, dialect);
    }
    let row = text.offset_to_point(offset).row;
    let line = text.row_to_string(row);
    let col = offset - text.row_start(row);
    let hits = hits(table, &line);
    let under = hits.iter().rfind(|hit| hit.span.contains(&col));
    let hit = match under {
        Some(hit) => hit,
        None => {
            let Some(next) = hits.iter().find(|hit| hit.span.start > col) else {
                return Keyword::None;
            };
            let bracket = |byte: &u8| PAIRS.iter().any(|&(a, b)| *byte == a || *byte == b);
            if line.as_bytes()[col..next.span.start].iter().any(bracket) {
                return Keyword::None;
            }
            next
        }
    };
    // As matchit sets up `searchpair()`: forward from all but the closer
    // (to the next middle or closer), back from the closer to the opener;
    // `g%` forward from the opener to the closer, else back to the word
    // before.
    let (forward, middles) = match (backward, hit.part) {
        (false, Part::Close) => (false, false),
        (false, _) => (true, true),
        (true, Part::Open) => (true, false),
        (true, _) => (false, true),
    };
    let group = hit.group;
    let start = hit.span.start;
    let mut nest = 1usize;
    let mut step = |found: &Hit| {
        if found.group != group {
            return false;
        }
        let deeper = if forward { Part::Open } else { Part::Close };
        match found.part {
            part if part == deeper => nest += 1,
            Part::Middle if !middles || nest > 1 => {}
            _ => nest -= 1,
        }
        nest == 0
    };
    if forward {
        let mut later = hits.iter().filter(|found| found.span.start > start);
        if let Some(found) = later.find(|found| step(found)) {
            return lands(text, row, found);
        }
        for other in row + 1..text.row_count() {
            let line = text.row_to_string(other);
            if let Some(found) = hits_in(table, &line).find(|found| step(found)) {
                return lands(text, other, &found);
            }
        }
    } else {
        let mut earlier = hits.iter().rev().filter(|found| found.span.start < start);
        if let Some(found) = earlier.find(|found| step(found)) {
            return lands(text, row, found);
        }
        for other in (0..row).rev() {
            let line = text.row_to_string(other);
            let found = self::hits(table, &line)
                .into_iter()
                .rev()
                .find(|found| step(found));
            if let Some(found) = found {
                return lands(text, other, &found);
            }
        }
    }
    Keyword::Fails
}

/// [`Keyword::Lands`] on `hit`, on `row`.
fn lands(text: &Text, row: u32, hit: &Hit) -> Keyword {
    let start = text.row_start(row);
    Keyword::Lands(start + hit.span.start..start + hit.span.end)
}

/// The keywords of `table` on `line`, in order.
fn hits(table: &Table, line: &str) -> Vec<Hit> {
    hits_in(table, line).collect()
}

/// [`hits`], as they're found.
fn hits_in<'a>(table: &'a Table, line: &'a str) -> impl Iterator<Item = Hit> + 'a {
    let lead = Lead::of(line);
    words(line).filter_map(move |word| {
        (table.groups.iter().enumerate()).find_map(|(group, rules)| {
            let (end, part) = matched(rules, line, &lead, word.clone())?;
            let start = match rules.place {
                Place::RowStart | Place::Bang => 0,
                Place::Word | Place::Command => word.start,
            };
            Some(Hit {
                span: start..end,
                group,
                part,
            })
        })
    })
}

#[cfg(test)]
thread_local! {
    /// The bytes placement checks have looked at on this thread.
    static PLACED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Notes (for a test) that a placement check looked at `bytes` bytes.
fn looked(bytes: usize) {
    #[cfg(test)]
    PLACED.set(PLACED.get() + bytes);
    let _ = bytes;
}

/// Where a row's leading blanks end, looked for once a row, not once a
/// word.
struct Lead {
    /// The first byte that isn't a space.
    spaces: usize,
    /// After a `!` in the first column, the first byte past it that isn't
    /// a blank.
    bang: Option<usize>,
}

impl Lead {
    fn of(line: &str) -> Self {
        let blank = |ch: char| ch == ' ' || ch == '\t';
        let spaces = line.len() - line.trim_start_matches(' ').len();
        let bang = line.strip_prefix('!').map(|rest| {
            let after = rest.trim_start_matches(blank);
            looked(rest.len() - after.len());
            line.len() - after.len()
        });
        looked(spaces);
        Self { spaces, bang }
    }
}

/// Where the word of `group` at `word` on `line` (whose leading blanks are
/// `lead`) ends, and which part it is, if the word starts one of the
/// group's where its place allows.
fn matched(group: &Group, line: &str, lead: &Lead, word: Range<usize>) -> Option<(usize, Part)> {
    let blank = |ch: char| ch == ' ' || ch == '\t';
    let placed = match group.place {
        Place::Word => true,
        // Only the blanks since the last word: each run once a row.
        Place::Command => {
            let before = &line[..word.start];
            let trimmed = before.trim_end_matches(blank);
            looked(before.len() - trimmed.len());
            matches!(trimmed.chars().last(), None | Some(';'))
        }
        Place::RowStart => word.start <= lead.spaces,
        Place::Bang => lead.bang.is_some_and(|end| word.start <= end),
    };
    if !placed {
        return None;
    }
    let parts = [
        (group.open, Part::Open),
        (group.middle, Part::Middle),
        (group.close, Part::Close),
    ];
    // The longest word that matches, as the patterns' optional tails
    // (`else ifeq`) take all they can.
    let found = parts.into_iter().flat_map(|(keywords, part)| {
        let word = word.clone();
        keywords
            .iter()
            .filter_map(move |keyword| Some((spells(line, word.clone(), keyword)?, part)))
    });
    found.max_by_key(|&(end, _)| end)
}

/// Where `keyword` ends if it's spelled from the whole word at `word` on
/// `line`, each space in it one or more blanks before another whole word.
fn spells(line: &str, word: Range<usize>, keyword: &str) -> Option<usize> {
    let mut parts = keyword.split(' ');
    if &line[word.clone()] != parts.next()? {
        return None;
    }
    let mut end = word.end;
    for part in parts {
        let rest = &line[end..];
        let after = rest.trim_start_matches([' ', '\t']);
        if after.len() == rest.len() {
            return None;
        }
        let start = end + rest.len() - after.len();
        let next = words(&line[start..]).next()?;
        if next.start != 0 || &after[next.clone()] != part {
            return None;
        }
        end = start + next.end;
    }
    Some(end)
}

/// The whole words of `line` (runs of keyword chars), by their bytes.
fn words(line: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut chars = line.char_indices().peekable();
    std::iter::from_fn(move || {
        let (start, _) = chars.find(|&(_, ch)| motion::is_word_char(ch))?;
        let mut end = line.len();
        while let Some(&(ix, ch)) = chars.peek() {
            if !motion::is_word_char(ch) {
                end = ix;
                break;
            }
            chars.next();
        }
        Some(start..end)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(table: &Table, line: &str) -> Vec<Part> {
        hits(table, line).into_iter().map(|hit| hit.part).collect()
    }

    fn spans(table: &Table, line: &str) -> Vec<(usize, usize)> {
        let hits = hits(table, line).into_iter();
        hits.map(|hit| (hit.span.start, hit.span.end)).collect()
    }

    #[test]
    fn words_are_whole_keyword_runs() {
        let words: Vec<_> = words("a.if_x = (é1) end").collect();
        assert_eq!(words, [0..1, 2..6, 10..13, 15..18]);
    }

    #[test]
    fn placement_reads_leading_blanks_once_a_row() {
        // 100,000 leading blanks, then 100,000 short words: each place
        // check is O(1) or reads only the blanks since the last word.
        let words = "x ".repeat(100_000);
        for row in [
            format!("{}ifeq {words}", " ".repeat(100_000)),
            format!("!{}if {words}", " ".repeat(100_000)),
            format!("{}if {words}", "\t".repeat(100_000)),
        ] {
            PLACED.set(0);
            for table in [&MAKE, &SH, &LUA] {
                let _ = hits(table, &row);
            }
            assert!(PLACED.get() <= 4 * row.len(), "{}", PLACED.get());
        }
        assert_eq!(spans(&MAKE, &format!("{}ifeq x", " ".repeat(9))), [(0, 13)]);
        assert_eq!(spans(&MAKE, "!\t else"), [(0, 7)]);
        assert!(spans(&MAKE, "! x else").is_empty());
    }

    #[test]
    fn places_follow_each_language() {
        // Lua: whole words anywhere, `elseif` not `else`.
        assert_eq!(
            spans(&LUA, "if a then return elseif x end ifx"),
            [(0, 2), (10, 16), (17, 23), (26, 29)]
        );
        assert_eq!(parts(&LUA, "elseif"), [Part::Middle]);
        // sh: a command's start only.
        assert_eq!(spans(&SH, "echo if; if x; then :; fi"), [(9, 11), (23, 25)]);
        assert_eq!(spans(&SH, "  if a; then if b"), [(2, 4)]);
        // make: the row's start after spaces, taking them; `else ifeq`
        // whole; nmake's after `!`.
        assert_eq!(spans(&MAKE, "  ifdef A"), [(0, 7)]);
        assert!(spans(&MAKE, "\tifeq (a,b)").is_empty());
        assert!(spans(&MAKE, "x ifeq (a,b)").is_empty());
        assert_eq!(spans(&MAKE, "else  ifdef B"), [(0, 11)]);
        assert_eq!(parts(&MAKE, "else ifdef B"), [Part::Middle]);
        assert_eq!(spans(&MAKE, "else ifdefx"), [(0, 4)]);
        assert_eq!(spans(&MAKE, "! elseifdef A"), [(0, 11)]);
        assert_eq!(spans(&MAKE, "x define y"), [(2, 8)]);
    }
}
