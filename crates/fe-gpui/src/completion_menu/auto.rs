//! Quick suggestions (docs/decisions/mgacx379x8-completion-as-you-type.md
//! §1): the completion menu asked for by itself, as a word is typed.
//!
//! - **When.** A word char typed into a word of at least
//!   `editor.completion.min_word_chars` chars, with no menu open or
//!   awaited, asks the provider (as [`CompletionTrigger::Invoked`]) once
//!   `editor.completion.delay_ms` pass with the text and caret as they
//!   were: anything typed, or the caret moved, sooner, drops the ask, and a
//!   word char typed then waits again.
//! - **Not** with `editor.completion.auto_trigger` off, outside vim's
//!   insert mode, with more than the one caret, while a snippet's tabstops
//!   are under way or an input method composes (which also closes a menu
//!   that opened by itself, or awaits its answer), or in a comment or a string (as the syntax's
//!   highlights at the char typed say, once the tree has caught up with
//!   the text) unless `in_comments` or `in_strings` is on.
//!   `ctrl-space` and trigger characters ask anyway.
//! - **Keys.** A menu that opened by itself, its selection not moved, lets
//!   `enter` type its new line and closes, under
//!   `editor.completion.accept_on_enter = "explicit"`; `"always"` has it
//!   accept, `"never"` never. `tab` accepts either way.
//!
//! An editor no host has given `[editor.completion]` to
//! ([`EditorView::set_completion_settings`]) never asks by itself and
//! offers no words: `ctrl-space` and trigger characters work as they did.

use std::{cell::RefCell, rc::Rc, time::Duration};

use fe_buffer::Version;
use fe_config::settings::{AcceptOnEnter, CompletionSettings, CompletionWords};
use fe_editor::{completion::CompletionTrigger, word_completion};
use gpui::{Context, Task};

use crate::EditorView;

/// How many more times a quick suggestion whose delay has passed waits for
/// the syntax tree to catch up with the text, which it reads the caret's
/// context from, before it gives up.
const CATCH_UP_TRIES: u32 = 30;

/// How long each of those waits is, at least.
const CATCH_UP_WAIT: Duration = Duration::from_millis(16);

/// An editor's `[editor.completion]` and the ask waiting out its delay.
#[derive(Default)]
pub(crate) struct Auto {
    /// As the host last gave them: `None` until it does.
    pub(super) settings: Option<CompletionSettings>,
    /// The same, for other editors to read without this one
    /// ([`super::words::register`]).
    shared: Rc<RefCell<Option<CompletionSettings>>>,
    /// The quick suggestion waiting out the delay, dropping it drops it,
    /// and the text's version and the caret it waits at.
    pending: Option<(Task<()>, (Version, usize))>,
}

impl Auto {
    pub(super) fn shared_settings(&self) -> Rc<RefCell<Option<CompletionSettings>>> {
        self.shared.clone()
    }
}

impl EditorView {
    /// Takes `[editor.completion]`: whether the menu opens by itself as a
    /// word is typed, and the words it offers. Turned off, an ask waiting
    /// out its delay is dropped, and so is a menu that opened by itself
    /// still awaiting its first answer; one asked for stays.
    pub fn set_completion_settings(&mut self, settings: &CompletionSettings) {
        if !settings.auto_trigger {
            self.completion_auto.pending = None;
            let awaited = (self.completion.as_ref()).is_some_and(|menu| {
                menu.auto && menu.keyword.is_none() && menu.session.items().is_empty()
            });
            if awaited {
                self.completion = None;
            }
        }
        self.completion_auto.settings = Some(settings.clone());
        *self.completion_auto.shared.borrow_mut() = Some(settings.clone());
    }

    /// Whether a quick suggestion waits out its delay.
    pub fn completion_pending(&self) -> bool {
        self.completion_auto.pending.is_some()
    }

    /// Drops a quick suggestion waiting out its delay, if any.
    pub(crate) fn drop_pending_completion(&mut self) {
        self.completion_auto.pending = None;
    }

    /// The text's version and the caret now.
    fn typed_at(&self) -> (Version, usize) {
        (
            self.model.snapshot().version(),
            self.model.primary_selection().head(),
        )
    }

    /// Drops a quick suggestion waiting out its delay if the text or the
    /// caret has changed since it was typed: on each keystroke, and each
    /// sync after a command or a click, so moving away and back drops it.
    pub(crate) fn drop_moved_pending_completion(&mut self) {
        let now = self.typed_at();
        if (self.completion_auto.pending.as_ref()).is_some_and(|(_, at)| *at != now) {
            self.completion_auto.pending = None;
        }
    }

    /// `text` was typed, and was no trigger character: with a word char
    /// last, asks for a quick suggestion once the delay passes, if it's
    /// wanted here ([`Self::wants_quick_suggestion`]) and the caret's
    /// context allows it then ([`Self::caret_context`]).
    pub(crate) fn schedule_quick_suggestion(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(settings) = &self.completion_auto.settings else {
            return;
        };
        let delay = Duration::from_millis(settings.delay_ms);
        if !(text.chars().next_back()).is_some_and(word_completion::is_word_char)
            || !self.wants_quick_suggestion()
        {
            return;
        }
        let at = self.typed_at();
        let task = cx.spawn(async move |view, cx| {
            let mut wait = delay;
            for _ in 0..=CATCH_UP_TRIES {
                if !wait.is_zero() {
                    cx.background_executor().timer(wait).await;
                }
                let done = view.update(cx, |view, cx| view.quick_suggestion_due(at, cx));
                if done.unwrap_or(true) {
                    return;
                }
                wait = delay.max(CATCH_UP_WAIT);
            }
            // The tree never caught up: nothing is asked.
            view.update(cx, |view, _| view.finish_pending_completion())
                .ok();
        });
        self.completion_auto.pending = Some((task, at));
    }

    /// Lets go of the quick suggestion that's finishing, from within it:
    /// detached, not dropped.
    fn finish_pending_completion(&mut self) {
        if let Some((task, _)) = self.completion_auto.pending.take() {
            task.detach();
        }
    }

    /// The delay passed for a quick suggestion typed at `at`: asks, if
    /// nothing has changed since and it's still wanted in the caret's
    /// context. `false` to wait for the syntax tree to catch up first.
    fn quick_suggestion_due(&mut self, at: (Version, usize), cx: &mut Context<Self>) -> bool {
        if self.typed_at() != at || !self.wants_quick_suggestion() {
            self.finish_pending_completion();
            return true;
        }
        let Some((comment, string)) = self.caret_context() else {
            return false;
        };
        self.finish_pending_completion();
        let settings = self.completion_auto.settings.as_ref();
        let quiet = settings.is_some_and(|settings| {
            comment && !settings.in_comments || string && !settings.in_strings
        });
        if !quiet {
            self.ask_completion(CompletionTrigger::Invoked, true, cx);
        }
        true
    }

    /// Whether a quick suggestion is wanted at the caret now, its context
    /// aside: see the module docs. Reads a bounded stretch of the text
    /// around the caret, never its whole row.
    fn wants_quick_suggestion(&mut self) -> bool {
        let Some(settings) = self.completion_auto.settings.clone() else {
            return false;
        };
        let selections = self.model.selections();
        let caret = match &selections[..] {
            [selection] if selection.range.is_empty() => selection.range.start,
            _ => return false,
        };
        if !settings.auto_trigger
            || self.completion.is_some()
            || !self.accepts_text()
            || self.model.in_snippet()
            || self.composition().is_some()
            || (self.completion_provider.is_none() && settings.words == CompletionWords::Off)
        {
            return false;
        }
        let text = self.model.text();
        let start = word_completion::word_at(text, caret).start;
        let chars = text.slice_to_string(start..caret).chars().count();
        chars >= settings.min_word_chars.max(1) as usize
    }

    /// Whether the char just typed, the one before the caret, is in a
    /// comment, and in a string. The syntax's highlights there say so with
    /// a `comment*` capture; a `string*` one but a symbol's (Ruby's
    /// `:name`) or a key's (TOML's, JSON's, YAML's), which are names, not
    /// text; or an escape's, inside a string. So does the host tree, whose
    /// nodes an injected language's captures (a doc comment's Markdown, a
    /// string's regex) hide from the highlights: an enclosing comment, or
    /// string with no interpolation between.
    ///
    /// `None` while that can't be known: the tree hasn't caught up with the
    /// text (an edited tree's nodes can stop short of what was typed at
    /// their end), the highlights there are incomplete (a query out of
    /// time, an injected language not compiled yet), the file is too big
    /// to parse whole, with only a window's tree, or its language is still
    /// loading. Plain text is neither.
    fn caret_context(&self) -> Option<(bool, bool)> {
        // No syntax yet isn't plain text while its language loads.
        if self.is_loading_language() {
            return None;
        }
        let Some(syntax) = self.syntax() else {
            return Some((false, false));
        };
        if syntax.version() != self.model.snapshot().version() || syntax.needs_parse() {
            return None;
        }
        let tree = syntax.tree()?;
        let text = self.model.text();
        let caret = self.model.primary_selection().head();
        let Some(typed) = word_completion::char_before(text, caret) else {
            return Some((false, false));
        };
        let probe = caret - typed.len_utf8()..caret;
        let highlights = syntax.checked_highlights(probe.clone());
        if !highlights.complete {
            return None;
        }
        let (mut comment, mut string, mut key) = (false, false, false);
        for span in highlights.spans {
            if span.range.start >= probe.end || span.range.end <= probe.start {
                continue;
            }
            let name = span.highlight.name();
            comment |= name.starts_with("comment");
            key |= name.starts_with("string.special.key");
            string |= name.starts_with("string")
                && !name.starts_with("string.special.symbol")
                && !name.starts_with("string.special.key")
                || name.contains("escape");
        }
        // The host tree's node at the char, and those enclosing it, by
        // kind: one naming a comment, and one naming a string
        // (`string_literal`, `template_string`) with no interpolation
        // (`interpolation`, `template_substitution`) between, which is code
        // again.
        let (mut host_comment, mut host_string, mut code) = (false, false, false);
        let mut node = (tree.root_node()).descendant_for_byte_range(probe.start, probe.end);
        while let Some(at) = node {
            let kind = at.kind();
            host_comment |= kind.contains("comment");
            code |= kind.contains("interpolat") || kind.contains("substitution");
            host_string |= !code && kind.contains("string");
            node = at.parent();
        }
        Some((comment || host_comment, string || host_string && !key))
    }

    /// Whether `enter` accepts the menu's selected item while it shows, as
    /// `editor.completion.accept_on_enter` says: under `"explicit"`, unless
    /// the menu opened by itself and its selection hasn't moved.
    pub(crate) fn enter_accepts_completion(&self) -> bool {
        let Some(menu) = &self.completion else {
            return true;
        };
        let setting = (self.completion_auto.settings.as_ref())
            .map_or(AcceptOnEnter::Always, |settings| settings.accept_on_enter);
        match setting {
            AcceptOnEnter::Always => true,
            AcceptOnEnter::Never => false,
            AcceptOnEnter::Explicit => !menu.auto || menu.moved,
        }
    }
}
