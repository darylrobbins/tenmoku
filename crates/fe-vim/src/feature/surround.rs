//! vim-surround, as its `doc/surround.txt` describes it: `ds{char}` and
//! `cs{old}{new}` delete or change the pair around the cursor;
//! `ys{motion}{new}`, `yss`, `yS`, `ySS` and visual `S` add one. A feature
//! module; see [`crate::feature`].
//!
//! `s` after `d` or `c` (in normal mode) starts the feature's prefix
//! (`vim.pending_kind` `surround`), which waits for the pair's char and, for
//! `cs`, the replacement's; `<Esc>`, or a char naming no pair, cancels. With
//! [`VimState::set_jump_on_s`](crate::VimState::set_jump_on_s) on, `s` after
//! an operator is the jump's instead.
//!
//! The pairs (`{old}`): `( ) b`, `{ } B`, `[ ] r`, `< > a`, `" ' `` ` ``, and
//! `t`, an HTML tag. They're found as the `a(`, `a"` and `at` objects find
//! them ([`text_object::select`]), so `ds"` follows their quote rules, and a
//! count takes the count-th enclosing pair, as `2di(` does (for quotes it's
//! ignored). An opening bracket also takes the blanks just inside the pair.
//! With no pair around a caret it changes nothing; with none around any,
//! the command fails.
//!
//! `{new}` is the pair to put in its place: an opening bracket adds a space
//! inside each side, a closing one (or its letter) doesn't, and any other
//! ASCII punctuation goes on both sides as itself. `<` or `t` reads a tag,
//! its name and attributes typed up to `>` (or `<CR>`; `<BS>` takes back a
//! char, and with none typed cancels, as `<Esc>` does): `<a href="x">`,
//! closing with `</a>`. A tag as `{old}` keeps its contents.
//!
//! Each caret acts (a pair two carets share, once), as one undo step, and
//! the caret goes to where the pair began. `.` repeats it, at the caret then
//! (vim-repeat's behaviour).
//!
//! ## Adding a pair
//!
//! `s` after `y` starts the [`Operator::Surround`] operator; its motion or
//! text object, once typed, waits on the prefix (holding the target) for
//! `{new}`. `yS` is the same with `rows` on. Doubled (`yss`, `ySs`, `ySS`)
//! it takes the caret's row, or `count` rows from it, without its indent:
//! `_` alone, after `ys`, is read so too. Visual `S` (as vim-surround and
//! VSCodeVim take it, not nvim's `R`) waits on the prefix with the selection
//! as its target. A search (`ys/`) cancels: a known limit.
//!
//! What it surrounds:
//!
//! - A charwise target, from `ys`: its text, without the blanks it ends
//!   with, which stay after the pair (vim-surround's `ysaw` keeps the space
//!   outside). A charwise selection: the text as selected.
//! - A linewise target (`ysj`, `ysip`) or selection (`V`): the rows whole,
//!   the pair on rows of their own at the first row's indent, without an
//!   opening bracket's inner space.
//! - A block: each row's part of it, on each row it reaches.
//! - With `rows` (`yS`, `ySS`): the text on rows of its own between the
//!   pair, each indented one 'shiftwidth' more than the row it began on, the
//!   pair at that row's indent (vim-surround reindents them by the buffer's
//!   indent rules instead).
//!
//! It's one undo step, the caret going to the start of the opening side it
//! put in (for a block, the top row's). `.` repeats `ys` with the same pair
//! and target from the caret; visual `S` it doesn't (vim-repeat's doesn't
//! either).

use std::ops::{Range, RangeInclusive};

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_editor::commands::origin;
use fe_text::Text;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Change, Mode, Motion, Operator, Pending, Step, Target, TextObject, VimCommands, VimKey,
    VimTarget, VisualKind, cmdline, commands,
    feature::{Feature, FeaturePrefix},
    history, insert, line_motions, marks,
    motion::Cursor,
    operator::{self, Span},
    reducer::invocation,
    text_object, visual,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    prefix: Some(prefix),
    register,
    ..Feature::NONE
};

/// The prefix's name, `vim.pending_kind` while it waits.
const NAME: &str = "surround";

/// `vim.surround`'s args: `ds{old}`, or `cs{old}{new}` with `new`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SurroundArgs {
    /// The pair's char, as `ds` takes it: `(`, `b`, `"`, `t`, ...
    pub old: char,
    /// The replacement, as `cs` takes it: a char naming a pair, or a whole
    /// opening tag (`<em>`); `None` deletes the pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
    /// Which enclosing pair: the count-th.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// `vim.surround_add`'s args: `ys{target}{pair}`, `yS` with `rows`, or
/// visual `S` over the selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SurroundAddArgs {
    /// What it surrounds: a motion or text object from each caret (`line`
    /// is the row's text without its indent, as `yss` takes it), or in a
    /// visual mode the `selection`.
    pub target: Target,
    /// The pair, as `cs` takes `{new}`: a char naming one, or a whole
    /// opening tag (`<em>`).
    pub pair: String,
    /// `yS`: the text goes on rows of its own between the pair, indented.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rows: bool,
    /// The target's count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// `s` after `d` or `c` starts the prefix, and after `y` (or `S`) the
/// surround operator, which doubled takes the row; `S` in a visual mode
/// waits on the prefix for the pair.
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    let VimKey::Char(ch) = key else {
        return None;
    };
    if pending.has_prefix() {
        return None;
    }
    let pending = pending.clone();
    if pending.is_visual() {
        return (ch == 'S' && pending.operator().is_none()).then(|| {
            pending.with_feature_prefix(FeaturePrefix::new(NAME).with_target(Target::Selection))
        });
    }
    match (pending.operator()?, ch) {
        (Operator::Delete | Operator::Change, 's') => {
            Some(pending.with_feature_prefix(FeaturePrefix::new(NAME)))
        }
        (Operator::Yank, 's' | 'S') => {
            Some(pending.with_operator(Operator::Surround { rows: ch == 'S' }))
        }
        (operator @ Operator::Surround { rows }, 's' | 'S') if ch == 's' || rows => {
            Some(pending.operate(operator, Target::Motion(Motion::Line)))
        }
        _ => None,
    }
}

/// `ys` with its target: waits on the prefix, holding it, for the pair.
pub(crate) fn target(pending: Pending, target: Target) -> Step {
    pending.with_feature_prefix(FeaturePrefix::new(NAME).with_target(target))
}

/// The pair's char, for `cs` and `ys` the replacement's, and a tag's
/// chars.
fn prefix(pending: &Pending, prefix: &FeaturePrefix, key: VimKey) -> Option<Step> {
    if prefix.feature() != NAME {
        return None;
    }
    let adding = prefix.target();
    // Where `{new}` begins among the keys: after `cs`'s `{old}`.
    let new_at = match (adding, pending.operator()) {
        (Some(_), _) => 0,
        (None, Some(Operator::Change)) => 1,
        (None, Some(Operator::Delete)) => {
            let VimKey::Char(ch) = key else {
                return None;
            };
            Pair::of(ch)?;
            return Some(on_pair(pending, ch, None));
        }
        (None, _) => return None,
    };
    let keys = prefix.keys();
    let wait = |prefix: FeaturePrefix| Some(pending.clone().with_feature_prefix(prefix));
    let commit = |new: String| match adding {
        Some(target) => invocation(
            "vim.surround_add",
            SurroundAddArgs {
                target,
                pair: new,
                rows: matches!(pending.operator(), Some(Operator::Surround { rows: true })),
                count: pending.count(),
            },
        ),
        None => on_pair(pending, keys[0], Some(new)),
    };
    // A tag being typed, after `<` or `t`.
    if keys.len() > new_at {
        let typed: String = keys[new_at + 1..].iter().collect();
        return match key {
            VimKey::Char('>') | VimKey::Enter => {
                let tag = format!("<{typed}>");
                sides(&tag)?;
                Some(commit(tag))
            }
            VimKey::Backspace if typed.is_empty() => None,
            VimKey::Backspace => wait(prefix.back()),
            VimKey::Char(ch) => wait(prefix.then(ch)?),
            _ => None,
        };
    }
    let VimKey::Char(ch) = key else {
        return None;
    };
    match ch {
        _ if new_at == 1 && keys.is_empty() => {
            Pair::of(ch)?;
            wait(prefix.then(ch)?)
        }
        '<' | 't' => wait(prefix.then(ch)?),
        ch => {
            replacement(ch)?;
            Some(commit(ch.to_string()))
        }
    }
}

/// `vim.surround` for `ds{old}`, or `cs{old}{new}`.
fn on_pair(pending: &Pending, old: char, new: Option<String>) -> Step {
    invocation(
        "vim.surround",
        SurroundArgs {
            old,
            new,
            count: pending.count(),
        },
    )
}

fn register(registry: &mut Registry<VimCommands>) {
    let normal = ContextPredicate::parse("editor && vim.mode == normal")
        .expect("vim's context predicate parses");
    registry
        .register_with_args(
            "vim.surround",
            "Vim Surround",
            |target: &mut VimTarget<'_>, actor: &Actor, args: SurroundArgs| {
                commands::follow(target);
                insert::adopt_closing(target.state);
                surround(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Deletes the brackets, quotes or HTML tag around each caret, or with `new` replaces them, as vim-surround's ds and cs do; one transaction for all carets. Fails with nothing changed when no caret has such a pair around it.",
        )
        .context(normal)
        .mutates_buffer(true);
    let normal_or_visual = ContextPredicate::parse("editor && (vim.mode == normal || vim.visual)")
        .expect("vim's context predicate parses");
    registry
        .register_with_args(
            "vim.surround_add",
            "Vim Surround Add",
            |target: &mut VimTarget<'_>, actor: &Actor, args: SurroundAddArgs| {
                commands::follow(target);
                insert::adopt_closing(target.state);
                surround_add(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Puts a pair of brackets or quotes, or an HTML tag, around what a motion or text object covers from each caret, or around the visual selection, as vim-surround's ys, yS and visual S do; one transaction for all carets. Fails with nothing changed when the target covers nothing.",
        )
        .context(normal_or_visual)
        .mutates_buffer(true);
}

/// What `{old}` names.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Pair {
    /// A bracket pair, found as this object finds it; `trim` (named by its
    /// opening bracket) takes the blanks inside too.
    Brackets { object: TextObject, trim: bool },
    /// A quote pair, found as this object finds it.
    Quotes { object: TextObject, quote: u8 },
    /// An HTML tag's opening and closing tags.
    Tag,
}

impl Pair {
    fn of(ch: char) -> Option<Self> {
        use TextObject::*;
        let brackets = |object, trim| Some(Pair::Brackets { object, trim });
        let quotes = |object, quote| Some(Pair::Quotes { object, quote });
        match ch {
            '(' => brackets(AParen, true),
            ')' | 'b' => brackets(AParen, false),
            '{' => brackets(ABrace, true),
            '}' | 'B' => brackets(ABrace, false),
            '[' => brackets(ABracket, true),
            ']' | 'r' => brackets(ABracket, false),
            '<' => brackets(AAngle, true),
            '>' | 'a' => brackets(AAngle, false),
            '"' => quotes(ADoubleQuote, b'"'),
            '\'' => quotes(ASingleQuote, b'\''),
            '`' => quotes(ABacktick, b'`'),
            't' => Some(Pair::Tag),
            _ => None,
        }
    }

    /// The pair around `offset`, the `count`-th out: its opening and its
    /// closing side's bytes.
    fn around(self, text: &Text, offset: usize, count: u32, tab: usize) -> Option<Found> {
        let chars = |object, count| match text_object::select(text, offset, object, count, tab) {
            Span::Chars(range) => Some(range),
            _ => None,
        };
        match self {
            Pair::Brackets { object, trim } => {
                let whole = chars(object, count).filter(|whole| whole.len() >= 2)?;
                let mut open = whole.start..whole.start + 1;
                let mut close = whole.end - 1..whole.end;
                if trim {
                    let bytes = text.slice_to_string(open.end..close.start);
                    let blank = |byte: &u8| matches!(byte, b' ' | b'\t');
                    let leading = bytes.bytes().take_while(blank).count();
                    let trailing = bytes.bytes().rev().take_while(blank).count();
                    open.end += leading;
                    close.start = (close.start - trailing).max(open.end);
                }
                Some(Found { open, close })
            }
            Pair::Quotes { object, quote } => {
                // `a"` takes the white space after (or before) the quotes.
                let whole = chars(object, 1)?;
                let bytes = text.slice_to_string(whole.clone());
                let open = whole.start + bytes.bytes().position(|byte| byte == quote)?;
                let close = whole.start + bytes.bytes().rposition(|byte| byte == quote)?;
                (open < close).then(|| Found {
                    open: open..open + 1,
                    close: close..close + 1,
                })
            }
            Pair::Tag => {
                let whole = chars(TextObject::ATag, count)?;
                let inner = chars(TextObject::InnerTag, count)?;
                Some(Found {
                    open: whole.start..inner.start,
                    close: inner.end..whole.end,
                })
            }
        }
    }
}

/// One caret's pair: the bytes each side replaces.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Found {
    open: Range<usize>,
    close: Range<usize>,
}

/// The sides `new` puts in: a pair's, by its char ([`replacement`]), or a
/// tag's, `<a href="x">` closing with `</a>`.
fn sides(new: &str) -> Option<(String, String)> {
    let mut chars = new.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        return replacement(ch);
    }
    let inner = new.strip_prefix('<')?.strip_suffix('>')?;
    let name: String = inner
        .chars()
        .take_while(|ch| !ch.is_whitespace() && *ch != '/' && *ch != '>')
        .collect();
    (!name.is_empty()).then(|| (new.to_string(), format!("</{name}>")))
}

/// What `{new}` puts in a pair's place: its opening and closing sides.
fn replacement(ch: char) -> Option<(String, String)> {
    let sides = |open: &str, close: &str| Some((open.to_string(), close.to_string()));
    match ch {
        '(' => sides("( ", " )"),
        ')' | 'b' => sides("(", ")"),
        '{' => sides("{ ", " }"),
        '}' | 'B' => sides("{", "}"),
        '[' => sides("[ ", " ]"),
        ']' | 'r' => sides("[", "]"),
        '>' | 'a' => sides("<", ">"),
        // A tag, typed after it: see [`sides`].
        '<' | 't' => None,
        ch if ch.is_ascii_punctuation() => Some((ch.to_string(), ch.to_string())),
        _ => None,
    }
}

/// `vim.surround`: deletes or replaces the pair around each caret, as one
/// transaction.
pub(crate) fn surround(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: SurroundArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let pair = Pair::of(args.old)
        .ok_or_else(|| CommandError::new(format!("no pair is named {:?}", args.old)))?;
    let (open, close) = match &args.new {
        Some(new) => sides(new)
            .ok_or_else(|| CommandError::new(format!("no replacement is named {new:?}")))?,
        None => Default::default(),
    };
    target.model.sync_display();
    let text = target.model.text().clone();
    let tab = target.model.tab_size().max(1);
    let count = args.count.unwrap_or(1).max(1);
    let before = target.model.selections();
    let carets_before = history::pairs(target.model);
    let found: Vec<Option<Found>> = before
        .iter()
        .map(|selection| pair.around(&text, selection.head(), count, tab))
        .collect();
    // Each pair once, and none whose sides overlap one already taken.
    let mut pairs: Vec<&Found> = found.iter().flatten().collect();
    pairs.sort_by_key(|found| (found.open.start, found.close.start));
    pairs.dedup();
    let mut taken: Vec<&Found> = Vec::new();
    for found in pairs {
        let overlaps = |range: &Range<usize>| {
            taken.iter().any(|other| {
                [&other.open, &other.close]
                    .iter()
                    .any(|side| range.start < side.end && side.start < range.end)
            })
        };
        if !overlaps(&found.open) && !overlaps(&found.close) {
            taken.push(found);
        }
    }
    if taken.is_empty() {
        target.state.failed = true;
        return Ok(Dispatched::new(Effect::Nothing));
    }
    let mut edits: Vec<(Range<usize>, String)> = taken
        .iter()
        .flat_map(|found| {
            [
                (found.open.clone(), open.clone()),
                (found.close.clone(), close.clone()),
            ]
        })
        .collect();
    edits.sort_by_key(|(range, _)| range.start);
    // An offset once the edits are in: one inside a replaced range goes to
    // the start of its new text.
    let after = |offset: usize| {
        let mut shift = 0isize;
        for (range, new) in &edits {
            if offset <= range.start {
                break;
            }
            if offset < range.end {
                return range.start.checked_add_signed(shift).expect("sorted edits");
            }
            shift += new.len() as isize - range.len() as isize;
        }
        offset
            .checked_add_signed(shift)
            .expect("an offset past its edits")
    };
    // A caret whose pair changed goes to where it began.
    let heads: Vec<usize> = before
        .iter()
        .zip(&found)
        .map(|(selection, found)| match found {
            Some(found) if taken.contains(&found) => after(found.open.start),
            _ => after(selection.head()),
        })
        .collect();
    let (transaction, snapshot) = operator::apply(target.model, edits, origin(actor));
    let after_text = snapshot.text().clone();
    let heads: Vec<usize> = heads
        .into_iter()
        .map(|head| Cursor::at(&after_text, head).offset())
        .collect();
    operator::land(target.model, &snapshot, heads);
    if let Some(id) = transaction {
        target.state.carets.record(id, carets_before);
    }
    target.state.goals = None;
    target.state.set_last_change(Change::Surround(args).into());
    Ok(Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect()))
}

/// What one caret's target has a pair put around.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Wrap {
    /// These bytes.
    Chars(Range<usize>),
    /// These rows whole, the pair on rows of its own.
    Rows(RangeInclusive<u32>),
}

impl Wrap {
    /// The bytes it surrounds.
    fn range(&self, text: &Text) -> Range<usize> {
        match self {
            Wrap::Chars(range) => range.clone(),
            Wrap::Rows(rows) => text.row_start(*rows.start())..operator::row_end(text, *rows.end()),
        }
    }
}

/// Text put in at an offset: the opening side's carries where the caret
/// goes within it.
struct Insert {
    at: usize,
    text: String,
    caret: Option<usize>,
}

/// `vim.surround_add`: puts the pair around each caret's target, or the
/// selection, as one transaction.
pub(crate) fn surround_add(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: SurroundAddArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let (open, close) = sides(&args.pair)
        .ok_or_else(|| CommandError::new(format!("no pair is named {:?}", args.pair)))?;
    let selected = args.target.is_selection();
    // Each caret's wraps: a block's are its rows'.
    let wraps: Vec<Vec<Wrap>> = match selected {
        true => vec![selection(target)?],
        // `af` and the like read the host's syntax, which the view holds.
        false => line_motions::with_view(target, line_motions::object(args.target), |target| {
            targets(target, &args)
        }),
    };
    target.model.sync_display();
    let text = target.model.text().clone();
    let before = target.model.selections();
    let carets_before = history::pairs(target.model);
    // Each wrap once, and none overlapping one already taken.
    let mut taken: Vec<Range<usize>> = Vec::new();
    let wraps: Vec<Vec<Wrap>> = wraps
        .into_iter()
        .map(|wraps| {
            wraps
                .into_iter()
                .filter(|wrap| {
                    let range = wrap.range(&text);
                    let clear = taken.iter().all(|other| {
                        *other != range && (range.end <= other.start || other.end <= range.start)
                    });
                    if clear {
                        taken.push(range);
                    }
                    clear
                })
                .collect()
        })
        .collect();
    if wraps.iter().all(Vec::is_empty) {
        target.state.failed = true;
        return Ok(Dispatched::new(match selected {
            true => Effect::Changed,
            false => Effect::Nothing,
        }));
    }
    let (unit, tabs) = operator::shift_unit(target.model);
    let unit = operator::indent_fill(unit, target.model.tab_size().max(1), tabs);
    // Each caret's inserts, the first its opening side.
    let inserts: Vec<Vec<Insert>> = wraps
        .iter()
        .map(|wraps| {
            wraps
                .iter()
                .flat_map(|wrap| inserts(&text, wrap, (&open, &close), args.rows, &unit))
                .collect()
        })
        .collect();
    // In offset order, a caret's before the next's at the same offset.
    let mut order: Vec<(usize, usize, usize)> = inserts
        .iter()
        .enumerate()
        .flat_map(|(caret, inserts)| {
            inserts
                .iter()
                .enumerate()
                .map(move |(ix, insert)| (insert.at, caret, ix))
        })
        .collect();
    order.sort();
    // Where each insert's text lands, and each caret.
    let mut shift = 0;
    let mut landed = vec![Vec::new(); inserts.len()];
    for &(at, caret, ix) in &order {
        landed[caret].push((ix, at + shift));
        shift += inserts[caret][ix].text.len();
    }
    let after = |offset: usize| {
        offset
            + order
                .iter()
                .take_while(|(at, _, _)| *at <= offset)
                .map(|&(_, caret, ix)| inserts[caret][ix].text.len())
                .sum::<usize>()
    };
    let heads: Vec<usize> = match selected {
        true => vec![opening(&inserts[0], &landed[0]).expect("a wrap")],
        false => before
            .iter()
            .zip(inserts.iter().zip(&landed))
            .map(|(selection, (inserts, landed))| {
                opening(inserts, landed).unwrap_or_else(|| after(selection.head()))
            })
            .collect(),
    };
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for &(at, caret, ix) in &order {
        let text = &inserts[caret][ix].text;
        match edits.last_mut() {
            Some((range, joined)) if range.start == at => joined.push_str(text),
            _ => edits.push((at..at, text.clone())),
        }
    }
    let (transaction, snapshot) = operator::apply(target.model, edits, origin(actor));
    let after_text = snapshot.text().clone();
    let heads: Vec<usize> = heads
        .into_iter()
        .map(|head| Cursor::at(&after_text, head).offset())
        .collect();
    operator::land(target.model, &snapshot, heads);
    if let Some(id) = transaction {
        target.state.carets.record(id, carets_before);
    }
    target.state.goals = None;
    if !selected {
        target
            .state
            .set_last_change(Change::SurroundAdd(args).into());
    }
    Ok(Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect()))
}

/// Where a caret's opening side lands, if it has one.
fn opening(inserts: &[Insert], landed: &[(usize, usize)]) -> Option<usize> {
    let (ix, insert) = inserts
        .iter()
        .enumerate()
        .find(|(_, insert)| insert.caret.is_some())?;
    let at = landed.iter().find(|(landed, _)| *landed == ix)?.1;
    Some(at + insert.caret?)
}

/// What one wrap puts in: the opening side first. With `rows` the text
/// goes on rows of its own, `unit` more indented.
fn inserts(
    text: &Text,
    wrap: &Wrap,
    (open, close): (&str, &str),
    rows: bool,
    unit: &str,
) -> Vec<Insert> {
    let row = |offset: usize| text.offset_to_point(offset).row;
    let indent = |row: u32| -> String {
        text.slice_to_string(text.row_start(row)..operator::row_end(text, row))
            .chars()
            .take_while(|ch| matches!(ch, ' ' | '\t'))
            .collect()
    };
    // On rows of their own, an opening bracket's inner space goes.
    let (own_open, own_close) = (open.trim_end_matches(' '), close.trim_start_matches(' '));
    // Each row of `rows` with text, indented by `unit`.
    let indented = |rows: RangeInclusive<u32>| {
        rows.filter(|&row| text.row_len(row) > 0)
            .map(|row| Insert {
                at: text.row_start(row),
                text: unit.to_string(),
                caret: None,
            })
            .collect::<Vec<_>>()
    };
    match wrap {
        Wrap::Chars(range) if !rows => vec![
            Insert {
                at: range.start,
                text: open.to_string(),
                caret: Some(0),
            },
            Insert {
                at: range.end,
                text: close.to_string(),
                caret: None,
            },
        ],
        Wrap::Chars(range) => {
            let first = row(range.start);
            let indent = indent(first);
            let mut inserts = vec![Insert {
                at: range.start,
                text: format!("{own_open}\n{indent}{unit}"),
                caret: Some(0),
            }];
            let last = row(range.end);
            if last > first {
                inserts.extend(indented(first + 1..=last));
            }
            inserts.push(Insert {
                at: range.end,
                text: format!("\n{indent}{own_close}"),
                caret: None,
            });
            inserts
        }
        Wrap::Rows(span) => {
            let (first, last) = (*span.start(), *span.end());
            let indent = indent(first);
            let mut inserts = vec![Insert {
                at: text.row_start(first),
                text: format!("{indent}{own_open}\n"),
                caret: Some(indent.len()),
            }];
            if rows {
                inserts.extend(indented(first..=last));
            }
            inserts.push(Insert {
                at: operator::row_end(text, last),
                text: format!("\n{indent}{own_close}"),
                caret: None,
            });
            inserts
        }
    }
}

/// Each caret's wrap for `ys`'s motion or text object.
fn targets(target: &mut VimTarget<'_>, args: &SurroundAddArgs) -> Vec<Vec<Wrap>> {
    // `n N * #` as the search they are, and a mark's motion at the mark.
    let measured = match args.target {
        Target::Motion(motion) => {
            let motion = cmdline::resolve(target, motion).map_or(motion, |(resolved, _)| resolved);
            Target::Motion(marks::resolve(target, motion).unwrap_or(motion))
        }
        other => other,
    };
    let jump = match measured {
        Target::Motion(motion) => marks::jump(target, motion, args.count),
        _ => None,
    };
    let VimTarget { model, state, .. } = target;
    operator::sync(model);
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    let last_find = state.last_find;
    if let Target::Motion(
        find @ (Motion::FindForward(_)
        | Motion::FindBackward(_)
        | Motion::TillForward(_)
        | Motion::TillBackward(_)),
    ) = args.target
    {
        state.last_find = Some(find);
    }
    let count = args.count.unwrap_or(1).max(1);
    let row = |offset: usize| text.offset_to_point(offset).row;
    let wraps: Vec<Vec<Wrap>> = model
        .selections()
        .iter()
        .map(|selection| {
            let head = Cursor::at(&text, selection.head()).offset();
            let span = match measured {
                // `yss`: the rows' text, without the first's indent.
                Target::Motion(Motion::Line) => {
                    let first = row(head);
                    let last = first
                        .saturating_add(count - 1)
                        .min(text.row_count().saturating_sub(1));
                    let mut cursor = Cursor::at(&text, text.row_start(first));
                    cursor.go_to_first_non_blank();
                    return vec![Wrap::Chars(cursor.offset()..operator::row_end(&text, last))];
                }
                Target::Motion(motion) => {
                    operator::motion_span(&text, head, motion, args.count, last_find, tab)
                }
                Target::TextObject(object) => text_object::select(&text, head, object, count, tab),
                _ => Span::Failed,
            };
            match span {
                Span::Chars(range) => vec![Wrap::Chars(without_trailing_blanks(&text, range))],
                Span::Lines { rows, .. } => vec![Wrap::Rows(rows)],
                Span::Move(_) | Span::Failed => Vec::new(),
            }
        })
        .collect();
    if let Some(from) = jump.filter(|_| wraps.iter().any(|wraps| !wraps.is_empty())) {
        state.marks.jumped(from);
    }
    wraps
}

/// `range` without the blanks it ends with, unless it's all blanks.
fn without_trailing_blanks(text: &Text, range: Range<usize>) -> Range<usize> {
    let bytes = text.slice_to_string(range.clone());
    let kept = bytes.trim_end_matches([' ', '\t']).len();
    match kept {
        0 => range,
        kept => range.start..range.start + kept,
    }
}

/// The visual selection's wraps, leaving the visual mode: a block's are its
/// rows' parts.
fn selection(target: &mut VimTarget<'_>) -> Result<Vec<Wrap>, CommandError> {
    let Mode::Visual(kind) = target.state.mode else {
        return Err(CommandError::new("no visual selection to surround"));
    };
    target.model.sync_display();
    let area = visual::live_area(target.model, target.state)
        .ok_or_else(|| CommandError::new("no visual selection to surround"))?;
    visual::exit(target)?;
    let (Some((first, from)), Some((last, to))) = (area.rows.first(), area.rows.last()) else {
        return Ok(Vec::new());
    };
    Ok(match kind {
        VisualKind::Char => vec![Wrap::Chars(from.start..to.end.max(from.start))],
        VisualKind::Line => vec![Wrap::Rows(*first..=*last)],
        VisualKind::Block => area
            .rows
            .into_iter()
            .filter(|(_, range)| !range.is_empty())
            .map(|(_, range)| Wrap::Chars(range))
            .collect(),
    })
}
