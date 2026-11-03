//! The spelling commands, typed in `workbench_commands.rs`:
//!
//! - `spell.suggest`: a [`Picker`] over the speller's suggestions for the
//!   misspelled word at the caret; picking one replaces the word, as one edit.
//! - `spell.add_word`: accepts the word for good, in the dictionary's file.
//! - `spell.ignore_word`: accepts the word until the app quits.
//! - `spell.toggle`: flips `editor.spell_check`, saved as the other toggles are
//!   ([`crate::persist`]).
//! - `spell.next`, `spell.previous`: move the caret to a squiggled word.
//!
//! They all go by the squiggles an editor has: the word at the caret is the one
//! whose squiggle holds the caret or ends at it, and only rows that have been
//! checked ([`crate::spell_squiggles`]) have any. So `spell.next` goes no further
//! than those, which are the rows around what's been shown since the last edit.
//!
//! An accepted word goes from every window's editors, and stays out of those of
//! windows opened later in the session.

use std::ops::Range;

use fe_buffer::{Anchor, Version};
use fe_editor::EditorModel;
use fe_gpui::EditorView;
use fe_ui::{HighlightedLabel, char_ranges};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, IntoElement as _,
    ParentElement as _, SharedString, Styled as _, Task, WeakEntity, Window, div,
};
use kiln_shell::ModalId;

use crate::{
    Workbench, persist,
    picker::{Picker, PickerDelegate},
    spell,
    toolbar_picker::subsequence,
    workbench::toast::{self, Toast},
};

/// What the commands on the word at the caret say with none there.
pub(crate) const NO_MISSPELLING_HERE: &str = "No misspelling here";

/// What `spell.next` and `spell.previous` say with nowhere to go.
pub(crate) const NO_MISSPELLINGS: &str = "No misspellings";

/// What `spell.suggest` says of an editor that takes no edits.
pub(crate) const READ_ONLY: &str = "This editor is read-only";

/// The picker's name in the modal layer.
const SPELL_PICKER: ModalId = ModalId("spelling");

/// What the status line says until the speller has answered.
const ASKING: &str = "Finding suggestions…";

/// What the status line says if the speller has nothing to offer.
const NO_SUGGESTIONS: &str = "No suggestions";

/// The misspelled word at an editor's caret.
struct WordAtCaret {
    editor: Entity<EditorView>,
    /// Where it is in the editor's model's text.
    range: Range<usize>,
    word: String,
}

/// The misspelling in `words`, sorted and apart, that holds `caret`, or else one
/// that ends at it: `caret` can be between two, in `tehWrod`.
fn misspelling_at(words: &[Range<usize>], caret: usize) -> Option<&Range<usize>> {
    (words.iter().find(|word| word.contains(&caret)))
        .or_else(|| words.iter().find(|word| word.end == caret))
}

/// Which of `words`, sorted, the caret at `from` goes to: the next start after it
/// or the last before it, wrapping around.
fn misspelling_from(words: &[Range<usize>], from: usize, forward: bool) -> Option<&Range<usize>> {
    if forward {
        (words.iter().find(|word| word.start > from)).or_else(|| words.first())
    } else {
        (words.iter().rev().find(|word| word.start < from)).or_else(|| words.last())
    }
}

/// `word`, at `at`, as `replacement`, if that's still what's there. Nothing else
/// says the word is where it was, with an edit since the picker opened, and a
/// plan has to go by the text as it is: ending a vim session can change it.
fn plan_replacement(
    model: &EditorModel,
    (at, since): (&Range<Anchor>, Version),
    word: &str,
    replacement: &str,
) -> Vec<(Range<usize>, String)> {
    let snapshot = model.snapshot();
    if model.buffer().version() != snapshot.version() || since < snapshot.oldest_version() {
        return Vec::new();
    }
    let range = snapshot.resolve_range(at);
    if snapshot.text().slice_to_string(range.clone()) != word {
        return Vec::new();
    }
    vec![(range, replacement.to_owned())]
}

/// The picker over the suggestions for a word.
pub(crate) struct SpellPicker {
    workbench: WeakEntity<Workbench>,
    editor: WeakEntity<EditorView>,
    word: String,
    /// Where the word is, as of `since`.
    at: Range<Anchor>,
    since: Version,
    /// What was focused when it opened.
    previous_focus: Option<FocusHandle>,
    suggestions: Vec<String>,
    /// The suggestions matching the query, with the ranges it matched.
    matches: Vec<(usize, Vec<Range<usize>>)>,
    query: String,
    /// Whether the speller has answered.
    loaded: bool,
    /// Asking the speller.
    _asking: Option<Task<()>>,
}

impl SpellPicker {
    /// Shows the suggestions once `asking` has them.
    fn ask(&mut self, asking: Task<Vec<String>>, cx: &mut Context<Picker<Self>>) {
        self._asking = Some(cx.spawn(async move |picker, cx| {
            let suggestions = asking.await;
            let shown = picker.update(cx, |picker, cx| {
                picker.delegate.suggestions = suggestions;
                picker.delegate.loaded = true;
                picker.delegate.refilter(cx);
            });
            shown.ok();
        }));
    }

    /// Matches the suggestions against the query, selecting the first.
    fn refilter(&mut self, cx: &mut Context<Picker<Self>>) {
        let query: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.suggestions.iter().enumerate())
            .filter_map(|(ix, suggestion)| {
                let indices = subsequence(&query, suggestion)?;
                Some((ix, char_ranges(suggestion, &indices)))
            })
            .collect();
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    /// Focuses what was focused before it opened.
    fn restore_focus(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }
}

#[cfg(test)]
impl SpellPicker {
    /// The suggestions matching the query.
    pub(crate) fn shown(&self) -> Vec<String> {
        (self.matches.iter())
            .map(|(ix, _)| self.suggestions[*ix].clone())
            .collect()
    }
}

impl PickerDelegate for SpellPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.refilter(cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// The suggestion's place in the list, which is asked for once per picker.
    fn match_id(&self, ix: usize) -> SharedString {
        format!("spell-row-{}", self.matches[ix].0).into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let (suggestion, highlights) = &self.matches[ix];
        let suggestion = self.suggestions[*suggestion].clone();
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .child(HighlightedLabel::new(suggestion, highlights.clone(), &ui))
            .into_any_element()
    }

    /// Replaces the word, as one edit, focus going back to where it was first.
    /// If the word isn't where it was any more, nothing is replaced, and it says so.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let replacement = self.suggestions[self.matches[ix].0].clone();
        self.restore_focus(window, cx);
        let Some(editor) = self.editor.upgrade() else {
            return true;
        };
        if editor.read(cx).is_read_only() {
            toast::show(cx, Toast::info(READ_ONLY));
            return true;
        }
        let (at, since, word) = (&self.at, self.since, self.word.as_str());
        let mut start = None;
        editor.update(cx, |view, cx| {
            view.model_mut().sync_display();
            view.edit_and_reveal(
                |model| {
                    let edits = plan_replacement(model, (at, since), word, &replacement);
                    start = edits.first().map(|(range, _)| range.start);
                    edits
                },
                cx,
            )
        });
        let Some(start) = start else {
            let toast = Toast::info(format!("{word} changed while the suggestions were open"))
                .detail("It wasn't replaced. Try again.");
            toast::show(cx, toast);
            return true;
        };
        // Its squiggle would stretch over the new word until the next check.
        if let Some(workbench) = self.workbench.upgrade() {
            workbench.update(cx, |workbench, cx| {
                let range = start..start + replacement.len();
                workbench.forget_spell_squiggles_in(&editor, range, cx);
            });
        }
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.restore_focus(window, cx);
    }

    fn rows_fill_width(&self) -> bool {
        true
    }

    fn status(&self) -> Option<SharedString> {
        if !self.loaded {
            Some(ASKING.into())
        } else if self.suggestions.is_empty() {
            Some(NO_SUGGESTIONS.into())
        } else {
            None
        }
    }
}

impl Workbench {
    /// The misspelled word at the active editor's primary caret, if any.
    fn misspelling_at_caret(&self, cx: &mut Context<Self>) -> Option<WordAtCaret> {
        let editor = self.active_editor(cx)?.clone();
        // The squiggles and the caret are both read from what this catches up.
        editor.update(cx, |view, _| view.model_mut().sync_display());
        let words = self.misspellings(&editor, cx);
        let model = editor.read(cx).model();
        let range = misspelling_at(&words, model.primary_selection().head())?.clone();
        let word = model.snapshot().text().slice_to_string(range.clone());
        // A squiggle stretches with what's typed inside it until the next check,
        // by which it can have stopped being one word.
        if word.contains(char::is_whitespace) {
            return None;
        }
        Some(WordAtCaret {
            editor,
            range,
            word,
        })
    }

    /// `spell.suggest`: shows the picker, whose list follows once the speller has
    /// answered, off the main thread.
    pub(crate) fn suggest_spelling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(found), Some(speller)) = (self.misspelling_at_caret(cx), self.speller()) else {
            return toast::show(cx, Toast::info(NO_MISSPELLING_HERE));
        };
        if found.editor.read(cx).is_read_only() {
            return toast::show(cx, Toast::info(READ_ONLY));
        }
        let language = self.spell_language().to_owned();
        let word = found.word.clone();
        let asking =
            cx.background_spawn(async move { spell::suggest(&*speller, &word, &language) });
        let picker = self.show_spell_picker(found, window, cx);
        picker.update(cx, |picker, cx| picker.delegate.ask(asking, cx));
    }

    /// Shows an empty picker, focused, in place of any other modal.
    fn show_spell_picker(
        &mut self,
        found: WordAtCaret,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Picker<SpellPicker>> {
        // First, as it gives the focus back to what this picker returns to.
        let modals = self.shell().read(cx).modals().clone();
        modals.update(cx, |layer, cx| layer.dismiss_modal(window, cx));
        let snapshot = found.editor.read(cx).model().snapshot();
        // Text put in right before or after the word isn't part of it.
        let at = snapshot.anchor_after(found.range.start)..snapshot.anchor_before(found.range.end);
        let delegate = SpellPicker {
            workbench: cx.entity().downgrade(),
            editor: found.editor.downgrade(),
            at,
            since: snapshot.version(),
            previous_focus: window.focused(cx),
            suggestions: Vec::new(),
            matches: Vec::new(),
            query: String::new(),
            loaded: false,
            _asking: None,
            word: found.word.clone(),
        };
        let placeholder = format!("Replace {}…", found.word);
        let picker = cx.new(|cx| Picker::new(delegate, &placeholder, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| picker.show(window, cx));
        (self.shell().clone()).update(cx, |shell, cx| {
            shell.show_modal(SPELL_PICKER, picker.clone(), window, cx);
        });
        picker
    }

    /// `spell.add_word`: the word's squiggles go now, in every window, and what
    /// became of the write it reports when it's done. It's accepted for this
    /// session even if that fails.
    pub(crate) fn add_word_at_caret(&mut self, cx: &mut Context<Self>) {
        let Some(WordAtCaret { word, .. }) = self.misspelling_at_caret(cx) else {
            return toast::show(cx, Toast::info(NO_MISSPELLING_HERE));
        };
        let writing = self.write_spell_word(&word, cx);
        self.accept_in_other_windows(&word, cx);
        cx.spawn(async move |_, cx| {
            let toast = match writing.await {
                Ok(()) => Toast::info(format!("Added {word} to dictionary")),
                Err(error) => {
                    log::warn!("can't add {word} to the dictionary: {error}");
                    Toast::error(format!("Can't add {word} to dictionary"))
                        .detail(format!("It counts until you quit: {error}"))
                }
            };
            cx.update(|cx| toast::show(cx, toast));
        })
        .detach();
    }

    /// `spell.ignore_word`: the word's squiggles go now, in every window, until
    /// the app quits.
    pub(crate) fn ignore_word_at_caret(&mut self, cx: &mut Context<Self>) {
        let Some(WordAtCaret { word, .. }) = self.misspelling_at_caret(cx) else {
            return toast::show(cx, Toast::info(NO_MISSPELLING_HERE));
        };
        self.ignore_spell_word(&word, cx);
        self.accept_in_other_windows(&word, cx);
    }

    /// Accepts `word` for the session in each other window, which has its own
    /// copy of the dictionary. A word added this way needs only the one write.
    fn accept_in_other_windows(&self, word: &str, cx: &mut Context<Self>) {
        let this = cx.entity();
        let word = word.to_owned();
        // Deferred, as this workbench is being updated now.
        cx.defer(move |cx| {
            let windows = cx.windows();
            let workbenches = windows.iter().filter_map(|window| window.downcast());
            for other in workbenches {
                let accepted = other.update(cx, |workbench: &mut Workbench, _, cx| {
                    if cx.entity() != this {
                        workbench.ignore_spell_word(&word, cx);
                    }
                });
                accepted.ok();
            }
        });
    }

    /// `spell.toggle`: flips the setting for good. It applies now, ahead of the
    /// reload that the write brings, which finds nothing left to change.
    pub(crate) fn toggle_spell_check(&mut self, cx: &mut Context<Self>) {
        let on = !self.spell_check_on();
        self.set_spell_check_on(on, cx);
        persist::save_spell_check(on, cx);
    }

    /// `spell.next` and `spell.previous`: moves the active editor's caret to the
    /// start of a squiggled word.
    pub(crate) fn go_to_misspelling(&mut self, forward: bool, cx: &mut Context<Self>) {
        let word = self.active_editor(cx).cloned().and_then(|editor| {
            editor.update(cx, |view, _| view.model_mut().sync_display());
            let words = self.misspellings(&editor, cx);
            let from = editor.read(cx).model().primary_selection().head();
            let word = misspelling_from(&words, from, forward)?.clone();
            Some((editor, word))
        });
        let Some((editor, word)) = word else {
            return toast::show(cx, Toast::info(NO_MISSPELLINGS));
        };
        editor.update(cx, |view, cx| {
            view.select_and_reveal(word.start..word.start, cx)
        });
    }
}

#[cfg(test)]
impl Workbench {
    /// The spelling picker in the modal layer, if it's the modal showing.
    pub(crate) fn spell_picker(&self, cx: &App) -> Option<Entity<Picker<SpellPicker>>> {
        let layer = self.shell().read(cx).modals().read(cx);
        layer.modal().filter(|id| *id == SPELL_PICKER)?;
        layer.modal_view()?.clone().downcast().ok()
    }
}
