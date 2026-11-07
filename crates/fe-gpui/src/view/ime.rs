//! Input method composition. Text an input method is composing stays out of
//! the buffer until it commits: it's drawn as the frame's inlay at the caret
//! ([`fe_display::layout::Inlay`]), and the input method sees the buffer with
//! it spliced in. A commit types it as the keyboard would.

use std::ops::Range;

use fe_buffer::{Anchor, Text};
use fe_command::{Actor, ContextPredicate, Dispatched, Effect, Invocation, Registry};
use fe_editor::commands::{Editor, EditorTarget};
use gpui::{Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window};

use super::EditorView;
use crate::{completion_menu, vim};

thread_local! {
    /// fe-editor's commands and accepting a completion, in its context as
    /// dispatch has it: to tell which end a composition.
    static EDITOR_COMMANDS: Registry<Editor> = {
        let mut registry = fe_editor::commands::registry();
        let accept = |_: &mut EditorTarget<'_>, _: &Actor| Ok(Dispatched::new(Effect::Nothing));
        registry
            .register(completion_menu::ACCEPT, "Accept Completion", accept)
            .expect("a valid, distinct command ID")
            .context(ContextPredicate::identifier(completion_menu::KEY_CONTEXT));
        registry
    };
}

/// Text an input method is composing at a caret, not yet in the buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Composition {
    /// The buffer position it's drawn at: the primary caret when it started.
    pub(crate) at: Anchor,
    pub(crate) text: String,
    /// The input method's selection within it, in bytes of `text`: its
    /// caret is drawn at the end.
    pub(crate) selected: Range<usize>,
    /// The vim switch's generation it started in: vim coming on or going
    /// off ends it.
    vim_generation: u64,
}

/// The prefixes of the fold commands, which move carets a fold hides.
const FOLD_PREFIXES: [&str; 2] = ["editor.fold", "editor.unfold"];

/// The document an input method sees: the buffer text with the composition
/// spliced in at `at`. Its offsets are bytes of that document ("composed"
/// offsets); the input method addresses it in UTF-16. With nothing
/// composing it's the buffer text.
struct Composed<'a> {
    text: &'a Text,
    at: usize,
    composing: &'a str,
}

impl Composed<'_> {
    fn at_utf16(&self) -> usize {
        self.text.offset_to_utf16(self.at)
    }

    /// The composed offset at a UTF-16 offset, at a char's start.
    fn offset_for_utf16(&self, utf16: usize) -> usize {
        let at = self.at_utf16();
        let len = utf16_len(self.composing);
        if utf16 <= at {
            self.text.utf16_to_offset(utf16)
        } else if utf16 <= at + len {
            self.at + byte_at_utf16(self.composing, utf16 - at)
        } else {
            self.text.utf16_to_offset(utf16 - len) + self.composing.len()
        }
    }

    fn utf16_for(&self, offset: usize) -> usize {
        let len = self.composing.len();
        if offset <= self.at {
            self.text.offset_to_utf16(offset)
        } else if offset <= self.at + len {
            self.at_utf16() + utf16_len(&self.composing[..offset - self.at])
        } else {
            self.text.offset_to_utf16(offset - len) + utf16_len(self.composing)
        }
    }

    fn utf16_range(&self, range: &Range<usize>) -> Range<usize> {
        self.utf16_for(range.start)..self.utf16_for(range.end)
    }

    fn range_for_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_for_utf16(range.start)..self.offset_for_utf16(range.end)
    }

    /// The buffer offset at a composed offset: a position inside the
    /// composition is where it's drawn.
    fn buffer_offset(&self, offset: usize) -> usize {
        if offset <= self.at {
            offset
        } else {
            offset.saturating_sub(self.composing.len()).max(self.at)
        }
    }

    /// The composed offset at a buffer offset: the composition is drawn
    /// after the buffer text at `at`.
    fn composed_offset(&self, offset: usize) -> usize {
        if offset <= self.at {
            offset
        } else {
            offset + self.composing.len()
        }
    }

    /// The composed offset as a byte of the composition, if it's in it.
    fn in_composition(&self, offset: usize) -> Option<usize> {
        (self.at..=self.at + self.composing.len())
            .contains(&offset)
            .then(|| offset - self.at)
    }

    fn slice(&self, range: Range<usize>) -> String {
        let range = range.start..range.end.max(range.start);
        let (at, len) = (self.at, self.composing.len());
        let before = range.start.min(at)..range.end.min(at);
        let inside = range.start.clamp(at, at + len) - at..range.end.clamp(at, at + len) - at;
        let after = range.start.max(at + len) - len..range.end.max(at + len) - len;
        let mut out = self.text.slice_to_string(before);
        out.push_str(&self.composing[inside]);
        out.push_str(&self.text.slice_to_string(after));
        out
    }
}

fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// The byte of `text` at a UTF-16 offset, at the start of the char it's in.
fn byte_at_utf16(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        units += ch.len_utf16();
        if units > utf16 {
            return byte;
        }
    }
    text.len()
}

/// The char boundary of `text` at or before `byte`.
fn floor_char_boundary(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

impl EditorView {
    /// What an input method is composing, and the buffer offset it's at.
    /// Nothing outside vim's insert mode.
    pub(crate) fn composition(&self) -> Option<(usize, &Composition)> {
        let composition = (self.composition.as_ref())
            .filter(|composition| composition.vim_generation == self.vim_generation)
            .filter(|_| self.accepts_text())?;
        Some((self.model.snapshot().resolve(&composition.at), composition))
    }

    fn composed(&self) -> Composed<'_> {
        let (at, composing) = self
            .composition()
            .map_or((0, ""), |(at, composition)| (at, composition.text.as_str()));
        Composed {
            text: self.model.text(),
            at,
            composing,
        }
    }

    /// The composition's caret, as a composed range.
    fn composition_caret(&self) -> Option<Range<usize>> {
        let (at, composition) = self.composition()?;
        Some(at + composition.selected.start..at + composition.selected.end)
    }

    /// Before `invocation` runs: ends the composition, typing nothing, if
    /// it's an editor command that moves carets or edits, a move that can't
    /// go further too, one of vim's with vim on, or accepting a completion.
    /// Copying, saving, the view's other commands, and one its registry or
    /// context would refuse leave it.
    pub(crate) fn end_composition_for(&mut self, invocation: &Invocation, cx: &mut Context<Self>) {
        if self.composition.is_none() {
            return;
        }
        // In the context and registry the command will run in
        // (`EditorView::run_command`), which must take it.
        let in_editor = ContextPredicate::identifier(fe_editor::commands::CONTEXT);
        let mut invocation = invocation.clone();
        if !in_editor.eval(&invocation.context) {
            invocation.context = fe_editor::commands::context();
        }
        self.sync_vim(cx);
        let registry = vim::registry();
        let ends = if self.vim.is_some() && registry.get(&invocation.command).is_some() {
            invocation.context = vim::stamp(invocation.context, self.vim.as_ref());
            registry.validate(&invocation).is_ok()
        } else {
            let command = invocation.command.as_str();
            EDITOR_COMMANDS.with(|registry| {
                registry.validate(&invocation).is_ok_and(|meta| {
                    meta.mutates_buffer
                        || command == completion_menu::ACCEPT
                        || (vim::MOTION_PREFIXES.iter().chain(&FOLD_PREFIXES))
                            .any(|prefix| command.starts_with(prefix))
                })
            })
        };
        if ends {
            self.cancel_composition(cx);
        }
    }

    /// Drops a composition without an edit, bringing the buffer caret back
    /// into view: the composition's own scrolling stays otherwise
    /// (`autoscroll` is off once a frame is drawn). Whether there was one.
    pub(crate) fn cancel_composition(&mut self, cx: &mut Context<Self>) -> bool {
        let was = self.composition.take().is_some();
        if was {
            self.autoscroll = true;
            cx.notify();
        }
        was
    }

    /// Whether an input method may type, as vim's mode is now: with vim
    /// off or in insert mode, in a view that isn't read-only. Otherwise a
    /// composition goes, typing nothing.
    fn takes_input(&mut self, cx: &mut Context<Self>) -> bool {
        self.sync_vim(cx);
        self.drop_stale_composition();
        if self.read_only {
            self.cancel_composition(cx);
        }
        self.accepts_text() && !self.read_only
    }

    /// Resolves keys held for a longer map before an input method's text
    /// goes in, then asks again whether the editor takes input: they may
    /// have been an `<Esc>`.
    fn resolve_prefix_for_input(&mut self, cx: &mut Context<Self>) -> bool {
        self.resolve_held_prefix(cx);
        self.takes_input(cx)
    }

    /// Whether a read-only view refuses an input method's call: every
    /// one, and one that `edits`, putting in text or taking it out, with
    /// vim on says `E21` ([`EditorView::refuse_edit`]).
    fn refuses_input(&mut self, edits: bool, cx: &mut Context<Self>) -> bool {
        if self.read_only && edits {
            self.refuse_edit(cx);
        }
        self.read_only
    }

    /// Drops a composition vim's mode or switch has ended since it started
    /// ([`EditorView::composition`]).
    pub(crate) fn drop_stale_composition(&mut self) {
        if self.composition().is_none() && self.composition.take().is_some() {
            self.autoscroll = true;
        }
    }

    /// Ends any composition with `text` typed at every caret, as
    /// `editor.insert` from the keyboard, so vim's insert session, macro
    /// recording and undo grouping see one typed run. Ending one with
    /// nothing types nothing.
    fn commit(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.cancel_composition(cx) && text.is_empty() {
            return;
        }
        self.type_text(text, cx);
    }

    /// In replace mode, puts `text` in place of `range` as an input method
    /// asks: as many of the range's graphemes as `text` has chars are
    /// overwritten as typing overwrites, so `<BS>` puts them back; the
    /// range's graphemes left over are taken out first, or the chars left
    /// over go in first, before the overwriting. Where typing wouldn't
    /// overwrite char for grapheme, the range is simply replaced: a line
    /// break or a tab in either, or a char of `text` that joins the one
    /// before it. Recorded as `text` typed, which a replay overwrites with
    /// from the caret.
    fn replace_over(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        use unicode_segmentation::UnicodeSegmentation as _;
        let typed = Invocation::of(vim::INSERT)
            .expect("a valid command ID")
            .args(fe_command::Value::from_iter([("text", text)]))
            .source(fe_command::Source::Keyboard);
        let origin = fe_editor::commands::origin(&typed.actor);
        let taken = self.model.text().slice_to_string(range.clone());
        let graphemes: Vec<&str> = taken.graphemes(true).collect();
        let chars = text.chars().count();
        let breaks = ['\n', '\r', '\t'];
        if taken.contains(breaks)
            || text.contains(breaks)
            || text.graphemes(true).count() != chars
            || !on_grapheme_edges(self.model.text(), &range)
        {
            self.model.run_as(origin, |model| {
                model.select_range(range, false);
                model.insert(text);
            });
            self.record_typing(&typed);
            self.changed(cx);
            return;
        }
        let over = match chars.checked_sub(graphemes.len()) {
            // Fewer chars: the graphemes past them go.
            None => {
                let kept: usize = graphemes[..chars].iter().map(|g| g.len()).sum();
                self.model.run_as(origin, |model| {
                    model.select_range(range.start + kept..range.end, false);
                    model.insert("");
                    model.set_cursor(range.start);
                });
                text
            }
            // More: the first of them go in before the range.
            Some(more) => {
                let split = text
                    .char_indices()
                    .nth(more)
                    .map_or(text.len(), |(at, _)| at);
                self.model.run_as(origin, |model| {
                    model.set_cursor(range.start);
                    if split > 0 {
                        model.insert(&text[..split]);
                    }
                });
                &text[split..]
            }
        };
        self.record_typing(&typed);
        // Recorded whole above: its parts aren't again.
        let replaying = std::mem::replace(&mut self.vim_replaying, true);
        if !over.is_empty() {
            self.type_text(over, cx);
        }
        self.vim_replaying = replaying;
        self.changed(cx);
    }

    /// Whether the last frame drew the composition as it is now, so its
    /// inlay's geometry is the composition's.
    fn composition_drawn(&self) -> bool {
        let layout = self.layout.as_ref();
        self.composition().is_some_and(|(at, composition)| {
            layout.is_some_and(|layout| layout.shows_composition(at, &composition.text))
        })
    }
}

/// Whether `range` starts and ends between graphemes of the text around
/// it, as typing there would overwrite them.
fn on_grapheme_edges(text: &Text, range: &Range<usize>) -> bool {
    use unicode_segmentation::UnicodeSegmentation as _;
    let start = text.row_start(text.offset_to_point(range.start).row);
    // Far enough past the end for a grapheme to be whole.
    let mut end = range.end;
    for _ in 0..16 {
        match text.char_at(end) {
            Some('\n') | None => break,
            Some(ch) => end += ch.len_utf8(),
        }
    }
    let window = text.slice_to_string(start..end);
    let edges: Vec<usize> = (window.grapheme_indices(true))
        .map(|(at, _)| at)
        .chain([window.len()])
        .collect();
    let on = |offset: usize| edges.binary_search(&(offset - start)).is_ok();
    on(range.start) && on(range.end)
}

impl EntityInputHandler for EditorView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let composed = self.composed();
        let range = composed.range_for_utf16(&range_utf16);
        actual_range.replace(composed.utf16_range(&range));
        Some(composed.slice(range))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let composed = self.composed();
        if let Some(caret) = self.composition_caret() {
            return Some(UTF16Selection {
                range: composed.utf16_range(&caret),
                reversed: false,
            });
        }
        let selection = self.model.primary_selection();
        Some(UTF16Selection {
            range: composed.utf16_range(&selection.range),
            reversed: selection.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let (at, composition) = self.composition()?;
        Some(
            self.composed()
                .utf16_range(&(at..at + composition.text.len())),
        )
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if !self.takes_input(cx) {
            return;
        }
        if let Some(text) = self.composition.as_ref().map(|c| c.text.clone()) {
            self.commit(&text, cx);
        }
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Nothing over a range takes the range out.
        let edits = !text.is_empty() || range_utf16.as_ref().is_some_and(|r| !r.is_empty());
        if self.refuses_input(edits, cx) || !self.takes_input(cx) {
            return;
        }
        // Keys held for a longer map go on as typed before this text; if
        // they leave insert mode, it isn't typed.
        if !self.resolve_prefix_for_input(cx) {
            return;
        }
        let marked = self.marked_text_range(window, cx);
        let range = range_utf16.filter(|range| Some(range) != marked.as_ref());
        let Some(range) = range else {
            // Typing, or the composition committing: at every caret,
            // recorded like any key binding. Only typing is the host's to
            // hear of.
            let typed = self.composition.is_none();
            let before = self.model.buffer().version();
            self.commit(text, cx);
            if typed {
                self.emit_typed(text, before, cx);
            }
            return;
        };
        let composed = self.composed();
        let range = composed.range_for_utf16(&range);
        let range = composed.buffer_offset(range.start)..composed.buffer_offset(range.end);
        self.composition = None;
        // Typed over the range as the keyboard types, so a recording holds
        // it and replace mode overwrites.
        if self.typing_mode() == Some(true) {
            self.replace_over(range, text, cx);
        } else {
            self.model.select_range(range, false);
            self.type_text(text, cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.refuses_input(!text.is_empty(), cx) || !self.takes_input(cx) {
            return;
        }
        // A composition never completes a map: what's held resolves first,
        // and may leave insert mode.
        if !self.resolve_prefix_for_input(cx) {
            return;
        }
        if text.is_empty() {
            // The input method gave up composing: nothing is typed.
            self.cancel_composition(cx);
            return;
        }
        let marked = self.marked_text_range(window, cx);
        if let Some(range) = range_utf16.filter(|range| Some(range) != marked.as_ref()) {
            // Buffer text to compose over, as when an input method
            // reconverts it.
            let composed = self.composed();
            let range = composed.range_for_utf16(&range);
            let range = composed.buffer_offset(range.start)..composed.buffer_offset(range.end);
            self.composition = None;
            self.model.select_range(range, false);
        }
        let at = match &self.composition {
            Some(composition) => composition.at,
            None => {
                if self.model.selections().iter().any(|s| !s.range.is_empty()) {
                    // Composing replaces the selected text, which goes now,
                    // as one undo step.
                    self.type_text("", cx);
                }
                let head = self.model.primary_selection().head();
                self.model.snapshot().anchor_before(head)
            }
        };
        let selected = new_selected_range_utf16.map_or(text.len()..text.len(), |selected| {
            byte_at_utf16(text, selected.start)..byte_at_utf16(text, selected.end)
        });
        self.composition = Some(Composition {
            at,
            text: text.to_string(),
            selected,
            vim_generation: self.vim_generation,
        });
        self.changed(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let composed = self.composed();
        let range = composed.range_for_utf16(&range_utf16);
        let inside = (composed.in_composition(range.start))
            .zip(composed.in_composition(range.end.max(range.start)))
            .filter(|_| self.composition_drawn());
        let caret = composed.buffer_offset(range.start);
        let layout = self.layout.as_mut()?;
        // Until a frame draws the composition as it is, the caret it's at.
        inside
            .and_then(|(start, end)| layout.inlay_bounds(start..end))
            .or_else(|| layout.caret_bounds(caret))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        if let Some((at, composition)) = self.composition()
            && self.composition_drawn()
            && let Some(index) = self.layout.as_ref()?.inlay_index_at(position)
        {
            let index = floor_char_boundary(&composition.text, index);
            return Some(self.composed().utf16_for(at + index));
        }
        let offset = self.offset_for_position(position);
        let composed = self.composed();
        Some(composed.utf16_for(composed.composed_offset(offset)))
    }
}
