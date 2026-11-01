//! vim-abolish's case coercion, as its `doc/abolish.txt` describes it:
//! `cr` then a char coerces the word under the cursor ([`Operator::Coerce`]).
//! A feature module; see [`crate::feature`].
//!
//! `r` after `c` (in normal mode) starts the feature's prefix
//! (`vim.pending_kind` `abolish`), which waits for the coercion's char:
//!
//! - `s` or `_`: snake_case. `m` or `p`: MixedCase. `c`: camelCase.
//! - `u` or `U`: UPPER_CASE. `-` or `k`: dash-case. `.`: dot.case.
//! - `<Space>`: space case. `t`: Title Case.
//!
//! `<Esc>`, or any other key, cancels and changes nothing.
//!
//! The transforms are abolish's (`g:Abolish.Coercions`), its patterns'
//! `\u` `\l` `\d` being ASCII and its case mapping Unicode, by nvim's tables
//! (one char for one, as `tolower()` and `toupper()`):
//!
//! - snake: `::` becomes `/`; a `_` goes between a run of uppercase
//!   letters and an uppercase letter followed by a lowercase one
//!   (`HTTPServer` to `HTTP_Server`), and between a lowercase letter or digit
//!   and an uppercase letter; `.` and `-` become `_`; then all is lowercased.
//! - camel: `-` becomes `_`; then with no `_` and some lowercase letter, only
//!   the first char is lowercased; else each `_` goes, the char after it
//!   uppercased and every other char lowercased.
//! - mixed: camel, its first char uppercased.
//! - upper: snake, uppercased. dash, dot and space: snake, its `_` each a
//!   `-`, `.` or ` `. title: space case, the first char of each word
//!   uppercased (a `[0-9A-Za-z_]` char not after a word char, as `\<\w`).
//!
//! It covers what `{count}iw` does, as abolish's `{count}g@iw`: `2crs` on
//! `fooBar bazQux` coerces `fooBar ` (the word and its blank) as one string.
//! Each caret acts (carets whose words overlap, once), as one undo step; a
//! word the coercion leaves as it was makes no edit, and with none changed
//! there's no undo step. The caret goes to the start of what it covered, and
//! `'[` and `']` mark the text it became. `.` repeats it at the caret then,
//! with the same coercion and count.
//!
//! A deliberate difference: abolish yanks the word and puts the coerced one
//! back, which leaves the old word in `"0` and `"-` (and `""`); this writes
//! no register. There's no visual-mode `cr`, as abolish maps none.

use std::ops::Range;

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, Dispatched, Effect};
use fe_editor::commands::origin;
use fe_text::{Bias, Text};

use crate::{
    Change, OperateArgs, Operator, Pending, Step, Target, TextObject, VimKey, VimTarget, edit,
    feature::{Feature, FeaturePrefix},
    history, marks,
    motion::Cursor,
    operator::{self, Span},
    text_object,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    prefix: Some(prefix),
    ..Feature::NONE
};

/// The prefix's name, `vim.pending_kind` while it waits.
const NAME: &str = "abolish";

/// `r` after `c` in normal mode: waits on the prefix for the coercion.
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    let take = key == VimKey::Char('r')
        && !pending.has_prefix()
        && !pending.is_visual()
        && pending.operator() == Some(Operator::Change);
    take.then(|| {
        pending
            .clone()
            .with_feature_prefix(FeaturePrefix::new(NAME))
    })
}

/// The coercion's char: commits `cr{char}` over `{count}iw`.
fn prefix(pending: &Pending, prefix: &FeaturePrefix, key: VimKey) -> Option<Step> {
    if prefix.feature() != NAME {
        return None;
    }
    let VimKey::Char(to) = key else {
        return None;
    };
    coerce(to, "")?;
    Some(pending.clone().operate(
        Operator::Coerce { to },
        Target::TextObject(TextObject::InnerWord),
    ))
}

/// `word` coerced as `cr{to}` coerces it; `None` if `to` names no coercion.
pub(crate) fn coerce(to: char, word: &str) -> Option<String> {
    Some(match to {
        's' | '_' => snake(word),
        'm' | 'p' => mixed(word),
        'c' => camel(word),
        'u' | 'U' => snake(word).chars().map(edit::raised).collect(),
        '-' | 'k' => snake(word).replace('_', "-"),
        '.' => snake(word).replace('_', "."),
        ' ' => snake(word).replace('_', " "),
        't' => title(&snake(word).replace('_', " ")),
        _ => return None,
    })
}

/// abolish's `s:snakecase`.
fn snake(word: &str) -> String {
    let chars: Vec<char> = word.replace("::", "/").chars().collect();
    // `\(\u\+\)\(\u\l\)` to `\1_\2`: before the last of an uppercase run
    // that a lowercase letter follows.
    let mut split = Vec::with_capacity(chars.len());
    let mut ix = 0;
    while ix < chars.len() {
        if !chars[ix].is_ascii_uppercase() {
            split.push(chars[ix]);
            ix += 1;
            continue;
        }
        let end = (ix..chars.len())
            .find(|&at| !chars[at].is_ascii_uppercase())
            .unwrap_or(chars.len());
        if end - ix >= 2 && end < chars.len() && chars[end].is_ascii_lowercase() {
            split.extend_from_slice(&chars[ix..end - 1]);
            split.extend(['_', chars[end - 1], chars[end]]);
            ix = end + 1;
        } else {
            split.extend_from_slice(&chars[ix..end]);
            ix = end;
        }
    }
    // `\(\l\|\d\)\(\u\)` to `\1_\2`.
    let mut snake = String::with_capacity(split.len() + 4);
    let mut ix = 0;
    while ix < split.len() {
        let ch = split[ix];
        snake.push(ch);
        if (ch.is_ascii_lowercase() || ch.is_ascii_digit())
            && split.get(ix + 1).is_some_and(char::is_ascii_uppercase)
        {
            snake.push('_');
            snake.push(split[ix + 1]);
            ix += 1;
        }
        ix += 1;
    }
    snake
        .chars()
        .map(|ch| match ch {
            '.' | '-' => '_',
            ch => edit::lowered(ch),
        })
        .collect()
}

/// abolish's `s:camelcase`.
fn camel(word: &str) -> String {
    let word = word.replace('-', "_");
    if !word.contains('_') && word.chars().any(|ch| ch.is_ascii_lowercase()) {
        return with_first(&word, edit::lowered);
    }
    // `\(_\)\=\(.\)`, where `.` takes any char but a line break.
    let mut camel = String::with_capacity(word.len());
    let mut chars = word.chars().peekable();
    while let Some(ch) = chars.next() {
        match chars.peek() {
            Some(&next) if ch == '_' && next != '\n' => {
                camel.push(edit::raised(next));
                chars.next();
            }
            _ => camel.push(edit::lowered(ch)),
        }
    }
    camel
}

/// abolish's `s:mixedcase`.
fn mixed(word: &str) -> String {
    with_first(&camel(word), edit::raised)
}

/// abolish's `s:titlecase` over the space case `spaced`: `\<\w` uppercased.
fn title(spaced: &str) -> String {
    let mut after_word = false;
    spaced
        .chars()
        .map(|ch| {
            let starts = !after_word && (ch.is_ascii_alphanumeric() || ch == '_');
            after_word = word_char(ch);
            match starts {
                true => edit::raised(ch),
                false => ch,
            }
        })
        .collect()
}

/// Whether `ch` is in a word, as vim's default 'iskeyword' (`@,48-57,_,192-255`)
/// and its classes past Latin-1 read it.
fn word_char(ch: char) -> bool {
    match ch {
        '\u{d7}' | '\u{f7}' => true,
        _ if ch.is_ascii() => ch.is_ascii_alphanumeric() || ch == '_',
        _ => ch >= '\u{c0}' && ch.is_alphanumeric(),
    }
}

/// `word` with its first char (`^.`, not a line break) mapped by `map`.
fn with_first(word: &str, map: fn(char) -> char) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) if first != '\n' => std::iter::once(map(first)).chain(chars).collect(),
        _ => word.to_string(),
    }
}

/// `vim.operate` with [`Operator::Coerce`]: over `{count}iw` from each
/// caret (as `cr` gives it), or any motion or text object.
pub(crate) fn operate(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let Operator::Coerce { to } = args.operator else {
        unreachable!("only the coerce operator gets here");
    };
    if coerce(to, "").is_none() {
        return Err(CommandError::new(format!("cr{to} names no coercion")));
    }
    let object = match args.target {
        Target::TextObject(object) => object,
        _ => {
            return Err(CommandError::new(
                "the coerce operator acts only on a text object",
            ));
        }
    };
    let VimTarget { model, state, .. } = target;
    operator::sync(model);
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    let before = model.selections();
    // Each caret's text, or where it stays.
    let covered: Vec<Result<Range<usize>, usize>> = before
        .iter()
        .map(|selection| {
            let head = Cursor::at(&text, selection.head()).offset();
            match text_object::select(&text, head, object, args.count.unwrap_or(1), tab) {
                Span::Chars(range) if !range.is_empty() => Ok(range),
                _ => Err(head),
            }
        })
        .collect();
    if covered.iter().all(Result::is_err) {
        state.failed = true;
        return Ok(Dispatched::new(Effect::Nothing));
    }
    state.set_last_change(Change::Operate(args).into());
    // The carets' ranges that overlap go as one; ranges that only touch
    // stay apart, each coerced as the word it is.
    let mut ranges: Vec<Range<usize>> = covered.iter().filter_map(|r| r.clone().ok()).collect();
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start < last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    // Each merged range's coerced text and where it lands in the text after.
    let mut edits = Vec::new();
    let mut landed: Vec<(Range<usize>, Range<usize>)> = Vec::with_capacity(merged.len());
    let mut shift = 0isize;
    for range in merged {
        let old = text.slice_to_string(range.clone());
        let new = coerce(to, &old).expect("checked above");
        let start = range.start.saturating_add_signed(shift);
        landed.push((range.clone(), start..start + new.len()));
        shift += new.len() as isize - old.len() as isize;
        edits.extend(trimmed(range, &old, &new));
    }
    // Where each caret goes: the start of what it covered, in the text
    // after; for undo, that same place in the text before.
    let (begins, landings): (Vec<usize>, Vec<usize>) = covered
        .iter()
        .map(|covered| match covered {
            Ok(range) => {
                let merged = landed
                    .iter()
                    .find(|(old, _)| old.contains(&range.start))
                    .expect("a caret's range is among the merged ones");
                (merged.0.start, merged.1.start)
            }
            // In another caret's range, at its start; else past the
            // ranges before it, by their shift.
            Err(head) => match landed.iter().find(|(old, _)| old.contains(head)) {
                Some((old, new)) => (old.start, new.start),
                None => {
                    let shifted = landed
                        .iter()
                        .rev()
                        .find(|(old, _)| old.end <= *head)
                        .map_or(*head, |(old, new)| head - old.end + new.end);
                    (*head, shifted)
                }
            },
        })
        .unzip();
    let first = landed
        .first()
        .map(|(_, new)| new.start)
        .expect("one at least");
    let last = landed.last().map(|(_, new)| new.end).expect("one at least");
    let ends = |text: &Text| {
        let end = text.clip_offset(last.max(first + 1) - 1, Bias::Left);
        (
            history::pair(text, first),
            history::pair(text, end.max(first)),
        )
    };
    if edits.is_empty() {
        let (start, end) = ends(&text);
        marks::recased(state, model, None, start, end);
        let carets: Vec<_> = landings
            .iter()
            .map(|&at| operator::caret(Cursor::at(&text, at).offset()))
            .collect();
        let moved = carets != before;
        model.set_selections(carets);
        let landed: Vec<usize> = model.selections().iter().map(|s| s.head()).collect();
        state.goals = None;
        operator::keep_goals(model, state, &landed, &vec![None; landed.len()]);
        return Ok(Dispatched::new(match moved {
            true => Effect::Changed,
            false => Effect::Nothing,
        }));
    }
    let (transaction, after) = operator::apply(model, edits, origin(actor));
    let (start, end) = ends(after.text());
    marks::recased(state, model, Some(after.version()), start, end);
    if let Some(id) = transaction {
        let begins = begins.iter().map(|&at| history::pair(&text, at)).collect();
        state.carets.record(id, begins);
    }
    let landed = operator::landed_at(model, &after, landings);
    state.goals = None;
    operator::keep_goals(model, state, &landed, &vec![None; landed.len()]);
    Ok(Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect()))
}

/// The edit that turns `old`, at `range`, into `new`, if it changes it: only
/// the chars between what they start and end with in common.
fn trimmed(range: Range<usize>, old: &str, new: &str) -> Option<(Range<usize>, String)> {
    if old == new {
        return None;
    }
    let prefix: usize = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let suffix: usize = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    Some((
        range.start + prefix..range.end - suffix,
        new[prefix..new.len() - suffix].to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::coerce;

    fn all(word: &str) -> Vec<String> {
        "s_mpcuU-k. t"
            .chars()
            .map(|to| coerce(to, word).unwrap())
            .collect()
    }

    #[test]
    fn coercions_are_abolishs() {
        assert_eq!(
            all("HTTPServer"),
            [
                "http_server",
                "http_server",
                "HTTPServer",
                "HTTPServer",
                "hTTPServer",
                "HTTP_SERVER",
                "HTTP_SERVER",
                "http-server",
                "http-server",
                "http.server",
                "http server",
                "Http Server",
            ]
        );
        assert_eq!(coerce('c', "foo_bar__baz_").unwrap(), "fooBar_baz_");
        assert_eq!(coerce('s', "Foo::BarBaz").unwrap(), "foo/bar_baz");
        assert_eq!(coerce('c', "FOO").unwrap(), "foo");
        assert_eq!(coerce('t', "émile_zola").unwrap(), "émile Zola");
        assert_eq!(coerce('z', "foo"), None);
    }
}
