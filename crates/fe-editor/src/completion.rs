//! A completion session (docs/decisions/lsp-client.md §7, "Completion"):
//! the items a host's completion source offered at a caret, filtered by
//! the word typed there, with a selection, and accepting one as a single
//! undoable edit. GPUI-free, and knows nothing of language servers.
//!
//! - **The word** runs from the start of the identifier (letters, digits
//!   and `_`) before the caret when completions were asked for, to the
//!   caret now. The items are matched against it fuzzily: its chars in
//!   order, anywhere in the item's filter text, case aside. An empty word
//!   matches every item. They're ranked by how well they match, then by
//!   their sort text, as the source ordered them.
//! - **Accept** applies the item's edit and its additional edits, both in
//!   the request's version, as one [`Origin::User`] transaction through
//!   [`Buffer::edit_from`](fe_buffer::Buffer::edit_from) on the request's
//!   snapshot: typing since the request is rebased over. Word chars typed
//!   since the request, which the item's edit replaces, go in the same
//!   transaction, so one undo restores the text from before the accept,
//!   and the caret where it was. Inside a transaction already open, as
//!   vim's insert session holds, it joins that one, as typing does. An
//!   item whose edit is a [`Snippet`] goes in as
//!   [`EditorModel::insert_snippet`] puts one in, in that same
//!   transaction, starting its session.

use std::{any::Any, fmt, ops::Range, rc::Rc};

use fe_buffer::{Bias, BufferSnapshot, EditError, Origin, Text, TransactionId, Version};

use crate::{
    CaretAt, CaretPick, CharClass, EditorModel, caret,
    snippet::{Expansion, Snippet, row_indent},
};

/// Why completions are asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletionTrigger {
    /// By the user, as with `ctrl-space`.
    Invoked,
    /// By typing a trigger character.
    Character(String),
    /// By typing more while the last list was incomplete.
    Incomplete,
}

/// An item a completion source offers.
#[derive(Clone, Default)]
pub struct CompletionItem {
    pub label: String,
    /// Shown after the label, such as a signature or a module path.
    pub label_detail: Option<String>,
    /// What sort of thing it is, as the menu names it: "function".
    pub kind: Option<String>,
    pub detail: Option<String>,
    /// Markdown, or plain text.
    pub documentation: Option<String>,
    /// What the typed word is matched against, if not the label.
    pub filter_text: Option<String>,
    /// What the items are ordered by, if not their labels.
    pub sort_text: Option<String>,
    /// What accepting it does, in bytes of the request's version.
    pub edit: (Range<usize>, String),
    /// What else it does elsewhere, such as adding an import.
    pub additional_edits: Vec<(Range<usize>, String)>,
    /// Whether `edit`'s text is a [`Snippet`], with tabstops and
    /// placeholders, rather than plain text.
    pub snippet: bool,
    /// Characters that, typed while the item is selected, accept it and
    /// are then typed after it, as in VS Code.
    pub commit_characters: Vec<String>,
    /// The id of a command to run once the item is accepted. The editor
    /// runs `editor.action.triggerSuggest` and
    /// `editor.action.triggerParameterHints` itself; any other is handed to
    /// the source.
    pub command: Option<String>,
    /// The source's own, to resolve the item with.
    pub data: Option<Rc<dyn Any>>,
}

impl fmt::Debug for CompletionItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompletionItem")
            .field("label", &self.label)
            .field("kind", &self.kind)
            .field("detail", &self.detail)
            .field("edit", &self.edit)
            .field("additional_edits", &self.additional_edits)
            .field("snippet", &self.snippet)
            .finish_non_exhaustive()
    }
}

/// What a completion source offers at a caret.
#[derive(Clone, Debug, Default)]
pub struct CompletionList {
    pub items: Vec<CompletionItem>,
    /// Typing more should ask again.
    pub is_incomplete: bool,
}

/// The items offered at a caret, as the word there is typed.
pub struct CompletionSession {
    /// The buffer when completions were asked for.
    base: BufferSnapshot,
    /// The caret then, in `base`.
    offset: usize,
    /// The start of the word then, in `base`.
    word_start: usize,
    items: Vec<CompletionItem>,
    is_incomplete: bool,
    /// The word as last typed.
    word: String,
    /// The items that match it, best first: indices into `items`.
    matches: Vec<usize>,
    /// The selected item: an index into `items`.
    selected: Option<usize>,
}

impl CompletionSession {
    /// A session for `list`, offered at `offset` of `base`, the buffer as
    /// it was asked for.
    pub fn new(base: BufferSnapshot, offset: usize, list: CompletionList) -> Self {
        let offset = base.text().clip_offset(offset, Bias::Left);
        let word_start = word_start(base.text(), offset);
        let mut items = list.items;
        items.sort_by(|a, b| {
            let key = |item: &CompletionItem| item.sort_text.clone().unwrap_or(item.label.clone());
            key(a).cmp(&key(b))
        });
        let mut session = Self {
            word: base.text().slice_to_string(word_start..offset),
            base,
            offset,
            word_start,
            items,
            is_incomplete: list.is_incomplete,
            matches: Vec::new(),
            selected: None,
        };
        session.filter();
        session
    }

    /// The version completions were asked for at.
    pub fn version(&self) -> Version {
        self.base.version()
    }

    /// The caret completions were asked for at, in that version.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Where the word starts in `model`'s text.
    pub fn word_start_in(&self, model: &EditorModel) -> usize {
        (model.snapshot()).map_offset(self.word_start, Bias::Left, self.version())
    }

    /// The word as last typed.
    pub fn word(&self) -> &str {
        &self.word
    }

    pub fn is_incomplete(&self) -> bool {
        self.is_incomplete
    }

    pub fn items(&self) -> &[CompletionItem] {
        &self.items
    }

    /// The items that match the word, best first: indices into
    /// [`items`](Self::items).
    pub fn matches(&self) -> &[usize] {
        &self.matches
    }

    /// The selected item's index in [`items`](Self::items).
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The selected item's position among the [`matches`](Self::matches).
    pub fn selected_match(&self) -> Option<usize> {
        let selected = self.selected?;
        self.matches.iter().position(|&ix| ix == selected)
    }

    /// Selects the next match, or the previous with `back`, wrapping.
    pub fn select_next(&mut self, back: bool) {
        let count = self.matches.len();
        let Some(at) = self.selected_match() else {
            return;
        };
        let next = if back {
            (at + count - 1) % count
        } else {
            (at + 1) % count
        };
        self.selected = Some(self.matches[next]);
    }

    /// Selects item `ix`, if it matches the word.
    pub fn select(&mut self, ix: usize) {
        if self.matches.contains(&ix) {
            self.selected = Some(ix);
        }
    }

    /// Replaces item `ix` with `item`, as resolved: with its
    /// documentation, say.
    pub fn set_item(&mut self, ix: usize, item: CompletionItem) {
        if let Some(slot) = self.items.get_mut(ix) {
            *slot = item;
        }
    }

    /// Filters by the word typed at `model`'s caret now. `false` if the
    /// session is over: the buffer isn't the one asked about, there isn't
    /// a lone caret, the text up to where completions were asked for has
    /// changed, or the caret has left what was typed since, which holds
    /// only word chars while the session goes on.
    pub fn update(&mut self, model: &EditorModel) -> bool {
        let Some((word, _)) = self.live(model) else {
            return false;
        };
        if word != self.word {
            self.word = word;
            self.filter();
        }
        true
    }

    /// Where the session stands in `model`: the word from its start to
    /// the caret, and the text typed since the request (`typed`), if the
    /// session goes on. It's over if the buffer isn't the one asked about,
    /// there isn't a lone caret, the text from the char before the word to
    /// the request's caret has changed (a trigger character deleted, say),
    /// or the caret has left what was typed since, or that holds anything
    /// but word chars.
    fn live(&self, model: &EditorModel) -> Option<(String, Range<usize>)> {
        let current = model.snapshot();
        let base = &self.base;
        if current.id() != base.id()
            || current.version() < base.version()
            || base.version() < current.oldest_version()
        {
            return None;
        }
        let selections = model.selections();
        let [selection] = &selections[..] else {
            return None;
        };
        let before = char_before(base.text(), self.word_start).map_or(0, char::len_utf8);
        if !selection.range.is_empty()
            || !unchanged(base, current, self.word_start - before..self.offset)
        {
            return None;
        }
        // Typing since the request went in at its caret.
        let version = base.version();
        let typed = current.map_offset(self.offset, Bias::Left, version)
            ..current.map_offset(self.offset, Bias::Right, version);
        let caret = selection.range.start;
        let start = self.word_start_in(model);
        if !typed.contains(&caret) && caret != typed.end {
            return None;
        }
        let word = current.text().slice_to_string(start..typed.end);
        word.chars()
            .all(|c| CharClass::of(c) == CharClass::Word)
            .then(|| (word[..caret - start].to_owned(), typed))
    }

    /// Whether, the session over, the caret in `model` is still in the
    /// same word, cut back or moved within it: completions should be asked
    /// for again there. Not if the char before the word changed (a
    /// trigger character deleted, say), or the word is gone.
    pub fn may_ask_again(&self, model: &EditorModel) -> bool {
        let current = model.snapshot();
        let base = &self.base;
        if current.id() != base.id()
            || current.version() < base.version()
            || base.version() < current.oldest_version()
        {
            return false;
        }
        let selections = model.selections();
        let [selection] = &selections[..] else {
            return false;
        };
        let start = self.word_start_in(model);
        let caret = selection.range.start;
        selection.range.is_empty()
            && caret > start
            && char_before(base.text(), self.word_start) == char_before(current.text(), start)
            && (current.text().slice_to_string(start..caret).chars())
                .all(|c| CharClass::of(c) == CharClass::Word)
    }

    /// Whether the session goes on in `model`: see [`update`](Self::update).
    pub fn is_live(&self, model: &EditorModel) -> bool {
        self.live(model).is_some()
    }

    fn filter(&mut self) {
        let mut scored: Vec<(i32, usize)> = (self.items.iter().enumerate())
            .filter_map(|(ix, item)| {
                let target = item.filter_text.as_deref().unwrap_or(&item.label);
                Some((-fuzzy_score(&self.word, target)?, ix))
            })
            .collect();
        // Stable: equal scores keep the sort text's order.
        scored.sort_by_key(|&(score, _)| score);
        self.matches = scored.into_iter().map(|(_, ix)| ix).collect();
        if !self.selected.is_some_and(|ix| self.matches.contains(&ix)) {
            self.selected = self.matches.first().copied();
        }
    }

    /// Accepts the selected item into `model`: see the module docs. The
    /// caret goes after its main edit's text. `Ok(None)` with nothing
    /// selected, the session over ([`update`](Self::update)) or nothing
    /// changed; an error, with nothing applied, if the text an edit
    /// replaces has changed since the request.
    pub fn accept(&self, model: &mut EditorModel) -> Result<Option<TransactionId>, EditError> {
        model.sync_display();
        let Some(item) = self.selected.and_then(|ix| self.items.get(ix)) else {
            return Ok(None);
        };
        let Some((_, typed)) = self.live(model) else {
            return Ok(None);
        };
        let caret_was = model.primary_selection().head();
        let (base, version) = (&self.base, self.version());
        let range = clip(base.text(), item.edit.0.clone());
        let snippet = item.snippet.then(|| Snippet::parse(&item.edit.1));
        let unit = model.indent_unit();
        // Borrowed apart from the model, so mirrors of a snippet stop the
        // edit lands in can be filled in its transaction.
        let shared = Rc::clone(&model.buffer);
        let mut buffer = shared.borrow_mut();
        let current = buffer.snapshot();
        // Indented as the row the edit starts on is now.
        let expansion = snippet.map(|snippet| {
            let start = current.map_offset(range.start, Bias::Right, version);
            Expansion::new(&snippet, &row_indent(current.text(), start), &unit)
        });
        let text = expansion.as_ref().map_or_else(
            || item.edit.1.clone(),
            |expansion| expansion.text().to_owned(),
        );
        // The additional edits first, so one at the main edit's start goes
        // before it, in the source's order.
        let mut edits: Vec<_> = (item.additional_edits.iter())
            .map(|(range, text)| (clip(base.text(), range.clone()), text.clone()))
            .collect();
        edits.push((range.clone(), text));
        let main = edits.len() - 1;
        // Checked first, as `edit_from` checks, so nothing is applied if
        // any conflicts: the main edit split at the request's caret, where
        // the text typed since sits, so that text is never inside it.
        let mut checks: Vec<_> = edits.iter().map(|(range, _)| range.clone()).collect();
        if range.start < self.offset && self.offset < range.end {
            checks[main] = range.start..self.offset;
            checks.push(self.offset..range.end);
        }
        let conflict = (checks.into_iter().enumerate())
            .find(|(_, range)| !unchanged(base, &current, range.clone()));
        if let Some((index, _)) = conflict {
            return Err(EditError::Conflict {
                index: index.min(main),
            });
        }

        // What was typed since goes first, in a batch of its own, so the
        // main edit replaces the text it was computed on.
        buffer.start_transaction(Origin::User);
        if !typed.is_empty() {
            buffer.edit([(typed.clone(), "")], Origin::User);
        }
        let after = buffer.snapshot();
        let end = match range.is_empty() {
            true => after.map_offset(range.start, Bias::Right, version),
            false => after.map_offset(range.end, Bias::Left, version),
        };
        let end = after.anchor_after(end);
        let applied = buffer.edit_from(base, edits, Origin::User);
        if expansion.is_none() {
            model.sync_snippet_mirrors(&mut buffer, None);
        }
        let id = buffer.end_transaction();
        // Checked above; a failure now would be a bug.
        debug_assert!(applied.is_ok(), "{applied:?}");
        let current = buffer.snapshot();
        let caret_at = current.resolve(&end);
        // Undoing ends on the first batch, which puts back the typed text
        // or else the text the main edit replaced, around where the caret
        // was; redoing on the last, whose main edit the caret follows.
        let batch = current.edits_since(after.version()).next().cloned();
        let at = |edits: &[fe_buffer::Edit], edit: usize, offset: usize| CaretAt {
            edit,
            count: edits.len(),
            offset,
        };
        let redo = batch.as_deref().and_then(|edits| {
            let ix = edits.iter().position(|edit| edit.new.end == caret_at)?;
            Some(at(edits, ix, caret_at - edits[ix].new.start))
        });
        let undo = match typed.is_empty() {
            false => Some(CaretAt {
                edit: 0,
                count: 1,
                offset: caret_was - typed.start,
            }),
            true => batch.as_deref().and_then(|edits| {
                let old = |edit: &fe_buffer::Edit| edit.old.start..=edit.old.end;
                let ix = edits
                    .iter()
                    .position(|edit| old(edit).contains(&caret_was))?;
                Some(at(edits, ix, caret_was - edits[ix].old.start))
            }),
        };
        drop(buffer);
        model.sync_display();
        if let Some(id) = id {
            model.pick_caret(id, CaretPick { undo, redo });
            if let Some(transactions) = &mut model.transactions {
                transactions.push(id);
            }
        }
        match expansion {
            // The snippet's text went in as it was given, as it's always
            // checked to have.
            Some(expansion)
                if caret_at
                    .checked_sub(expansion.text().len())
                    .is_some_and(|start| {
                        current.text().range_eq(start..caret_at, expansion.text())
                    }) =>
            {
                let start = caret_at - expansion.text().len();
                model.set_selections(expansion.first_selections(start));
                model.start_snippet(&current, start, expansion);
            }
            Some(_) => model.set_selections([caret(caret_at)]),
            None => model.set_selections([caret(caret_at)]),
        }
        Ok(id)
    }
}

/// Whether the text over `range` of `base` is unchanged in `current`, as
/// [`Buffer::edit_from`](fe_buffer::Buffer::edit_from) maps it: from after
/// text inserted at its start to before text inserted at its end.
fn unchanged(base: &BufferSnapshot, current: &BufferSnapshot, range: Range<usize>) -> bool {
    let start = current.map_offset(range.start, Bias::Right, base.version());
    let end = match range.is_empty() {
        true => start,
        false => current.map_offset(range.end, Bias::Left, base.version()),
    };
    start <= end
        && current
            .text()
            .range_eq(start..end, &base.text().slice_to_string(range))
}

/// The char that ends at `at`, if any.
fn char_before(text: &Text, at: usize) -> Option<char> {
    let start = text.clip_offset(at.saturating_sub(4), Bias::Left);
    text.slice_to_string(start..at).chars().next_back()
}

/// The start of the identifier (letters, digits and `_`) that ends at
/// `offset`.
fn word_start(text: &Text, offset: usize) -> usize {
    let row_start = text.row_start(text.offset_to_point(offset).row);
    let before = text.slice_to_string(row_start..offset);
    let start = (before.char_indices().rev())
        .take_while(|&(_, c)| CharClass::of(c) == CharClass::Word)
        .last();
    start.map_or(offset, |(ix, _)| row_start + ix)
}

fn clip(text: &Text, range: Range<usize>) -> Range<usize> {
    let start = text.clip_offset(range.start.min(range.end), Bias::Left);
    start..text.clip_offset(range.end.max(start), Bias::Right)
}

/// How well `word` matches `target`: its chars in order, case aside,
/// both lowercased in full (`İ` is `i` and a combining dot, as in
/// `i̇stanbul`). Matches at the start, at word boundaries, in a run and in
/// the same case score higher, by the chars they're in. `None` if it
/// doesn't match.
fn fuzzy_score(word: &str, target: &str) -> Option<i32> {
    // Each lowercase scalar wanted, and the char of `word` it's from.
    let wanted: Vec<(char, char)> = (word.chars())
        .flat_map(|typed| typed.to_lowercase().map(move |lower| (lower, typed)))
        .collect();
    let mut next = 0;
    let mut score = 0;
    let mut before: Option<char> = None;
    let mut last_matched = false;
    for c in target.chars() {
        if next == wanted.len() {
            break;
        }
        let mut typed = None;
        for lower in c.to_lowercase() {
            if wanted.get(next).is_some_and(|&(want, _)| want == lower) {
                typed = Some(wanted[next].1);
                next += 1;
            }
        }
        if let Some(typed) = typed {
            score += 1;
            let boundary = before.is_none_or(|before| {
                !before.is_alphanumeric() || (before.is_lowercase() && c.is_uppercase())
            });
            score += if boundary { 4 } else { 0 };
            score += if last_matched || before.is_none() {
                3
            } else {
                0
            };
            score += i32::from(c == typed);
        }
        last_matched = typed.is_some();
        before = Some(c);
    }
    (next == wanted.len()).then_some(score)
}
