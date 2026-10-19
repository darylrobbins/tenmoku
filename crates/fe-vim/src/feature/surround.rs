//! `ds{char}` and `cs{old}{new}`: delete or change the pair around the
//! cursor, as vim-surround's `doc/surround.txt` describes them. A feature
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
//! ASCII punctuation goes on both sides as itself. `<` and `t`, a tag, come
//! with `ys`, so they cancel for now. A tag as `{old}` keeps its contents.
//!
//! Each caret acts (a pair two carets share, once), as one undo step, and
//! the caret goes to where the pair began. `.` repeats it, at the caret then
//! (vim-repeat's behaviour).

use std::ops::Range;

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_editor::commands::origin;
use fe_text::Text;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Change, Operator, Pending, Step, TextObject, VimCommands, VimKey, VimTarget, commands,
    feature::{Feature, FeaturePrefix},
    history, insert,
    motion::Cursor,
    operator::{self, Span},
    reducer::invocation,
    text_object,
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
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SurroundArgs {
    /// The pair's char, as `ds` takes it: `(`, `b`, `"`, `t`, ...
    pub old: char,
    /// The replacement's char, as `cs` takes it; `None` deletes the pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<char>,
    /// Which enclosing pair: the count-th.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// `s` after `d` or `c`, in normal mode.
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    let surrounds = matches!(
        pending.operator(),
        Some(Operator::Delete | Operator::Change)
    );
    if key != VimKey::Char('s') || !surrounds || pending.is_visual() || pending.has_prefix() {
        return None;
    }
    Some(
        pending
            .clone()
            .with_feature_prefix(FeaturePrefix::new(NAME)),
    )
}

/// The pair's char, and for `cs` the replacement's.
fn prefix(pending: &Pending, prefix: FeaturePrefix, key: VimKey) -> Option<Step> {
    if prefix.feature() != NAME {
        return None;
    }
    let VimKey::Char(ch) = key else {
        return None;
    };
    let commit = |old: char, new: Option<char>| {
        invocation(
            "vim.surround",
            SurroundArgs {
                old,
                new,
                count: pending.count(),
            },
        )
    };
    match (pending.operator()?, prefix.keys()) {
        (Operator::Delete, []) => {
            Pair::of(ch)?;
            Some(commit(ch, None))
        }
        (Operator::Change, []) => {
            Pair::of(ch)?;
            Some(pending.clone().with_feature_prefix(prefix.then(ch)?))
        }
        (Operator::Change, &[old]) => {
            replacement(ch)?;
            Some(commit(old, Some(ch)))
        }
        _ => None,
    }
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
        // A tag, which `ys` brings.
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
    let (open, close) = match args.new {
        Some(new) => replacement(new)
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
