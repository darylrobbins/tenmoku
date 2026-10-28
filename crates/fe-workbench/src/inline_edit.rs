//! Inline Edit with AI (`ai.inline_edit`): a one-line input under the
//! selection, whose instruction goes to the chat's Claude provider through
//! [`fe_chat::run_inline_edit`], and whose reply shows in place as a
//! proposal's hunks to accept or reject with the keys and buttons any
//! proposal has.
//!
//! - **Input.** An [`InlineEdit`] per editor is the editor's overlay
//!   ([`EditorView::set_overlay`]), anchored under the selection's last row.
//!   The selection (or the caret) is kept as anchors, so edits elsewhere
//!   move it. The entities are kept by editor in [`InlineEdits`] and go
//!   with their editor. A second `ai.inline_edit` in the editor focuses the
//!   input there; an input never takes focus but when it opens.
//! - **Keys.** The input's root sets the `inline_edit` key context: Enter
//!   runs `inline_edit.submit` and Escape `inline_edit.cancel`, which the
//!   input in focus takes. Its editing keys are the usual `text_input.*`.
//! - **Request.** Enter sends the instruction with the selection and its
//!   surroundings. With no key in the chat, a toast says so and the input
//!   stays. While the reply streams the row says `Generating…`; Escape
//!   drops the stream, which cancels the request, and the buffer was never
//!   touched.
//! - **Reply.** The finished reply is proposed over the anchored range, as
//!   a chat's Apply proposes a block, beside the proposals the editor
//!   shows, and the first hunk is revealed. A failed request, or a reply
//!   with no code, shows in the row with the instruction kept. A range that
//!   edits deleted discards the reply with a toast.

use std::{cell::RefCell, collections::HashMap, ops::Range, rc::Rc, sync::Arc};

use fe_buffer::{Anchor, BufferId};
use fe_chat::{
    ChatProvider, ChatSettings, InlineEditInput, InlineEditUpdate, run_inline_edit, to_proposal,
};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_ui::{TextInput, ui_theme};
use futures::StreamExt as _;
use gpui::{
    AppContext as _, Context, Entity, EntityId, FocusHandle, Focusable as _, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};
use kiln::platform::command_actions::typed_command;

use crate::{
    Workbench, chat_panel,
    picker::{self, PaletteOp},
    workbench::toast::{self, Toast},
};

/// The key context the input's root sets.
pub(crate) const KEY_CONTEXT: &str = "inline_edit";

/// What the input says with no key in the chat.
const NO_KEY: &str = "Set an API key in the chat";

/// The input's placeholder.
const PLACEHOLDER: &str = "Edit selected code, or describe code to insert";

/// The inputs open, by their editor.
#[derive(Default)]
struct InlineEdits(HashMap<EntityId, Entity<InlineEdit>>);

impl Global for InlineEdits {}

fn edits(cx: &mut gpui::App) -> &mut HashMap<EntityId, Entity<InlineEdit>> {
    &mut cx.default_global::<InlineEdits>().0
}

/// What the input is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Taking an instruction.
    Asking,
    /// The reply is streaming.
    Generating,
}

/// The input under an editor's selection.
pub(crate) struct InlineEdit {
    editor: WeakEntity<EditorView>,
    input: Entity<TextInput>,
    buffer: BufferId,
    /// The selection (or caret) it edits, which follows edits.
    range: Range<Anchor>,
    /// Whether the selection had text.
    selected: bool,
    phase: Phase,
    /// Why the last request ended in no proposal.
    message: Option<SharedString>,
    /// The request under way: dropping it drops the stream.
    task: Option<Task<()>>,
    /// Counts the requests, for the proposals' session ids.
    requests: u64,
    /// The text the request was made over: a reply for a range that no
    /// longer holds it is discarded.
    sent: String,
    /// Where the overlay was last put, which it follows the range to.
    placed: Range<usize>,
    _editor: Option<gpui::Subscription>,
    /// Typed text, taken before the editor's bindings.
    _typing: gpui::Subscription,
}

impl Workbench {
    /// Opens the input under the active editor's selection, or focuses the
    /// one it has.
    pub(crate) fn inline_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        if let Some(open) = edits(cx).get(&editor.entity_id()).cloned() {
            return open.update(cx, |open, cx| open.focus(window, cx));
        }
        if editor.read(cx).is_read_only() {
            return toast::show(cx, Toast::info("This editor is read-only"));
        }
        // The key may be in the keychain: read it while the instruction is
        // typed.
        chat_panel::provider(self, cx);
        let open = cx.new(|cx| InlineEdit::new(&editor, cx));
        let id = editor.entity_id();
        edits(cx).insert(id, open.clone());
        // Dropped with its editor.
        cx.observe_release(&editor, move |_, _, cx| {
            if let Some(open) = edits(cx).remove(&id) {
                open.update(cx, |open, _| open.task = None);
            }
        })
        .detach();
        let range = open.read(cx).range(cx).unwrap_or(0..0);
        editor.update(cx, |editor, _| {
            editor.set_overlay(Some(open.clone().into()), range);
        });
        open.update(cx, |open, cx| open.focus(window, cx));
    }

    /// Sends the focused input's instruction.
    pub(crate) fn inline_edit_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = focused(window, cx) else {
            return;
        };
        if open.read(cx).phase == Phase::Generating {
            return;
        }
        let instruction = open.read(cx).input.read(cx).text().trim().to_owned();
        if instruction.is_empty() {
            return;
        }
        let Some((provider, settings)) = chat_panel::provider(self, cx) else {
            return toast::show(cx, Toast::info(NO_KEY));
        };
        let root = self.root().map(std::path::Path::to_owned);
        open.update(cx, |open, cx| {
            open.generate(provider, settings, instruction, root.as_deref(), window, cx);
        });
    }

    /// Stops the focused input's request, or closes the input.
    pub(crate) fn inline_edit_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = focused(window, cx) {
            open.update(cx, |open, cx| open.cancel(window, cx));
        }
    }
}

/// The input that has focus.
fn focused(window: &Window, cx: &mut gpui::App) -> Option<Entity<InlineEdit>> {
    let open: Vec<_> = edits(cx).values().cloned().collect();
    open.into_iter()
        .find(|open| open.read(cx).focus_handle(cx).contains_focused(window, cx))
}

impl InlineEdit {
    fn new(editor: &Entity<EditorView>, cx: &mut Context<Self>) -> Self {
        let ui = ui_theme(cx);
        let input = cx.new(|cx| TextInput::new(PLACEHOLDER, &ui, cx));
        let model = editor.read(cx).model();
        let snapshot = model.snapshot();
        let selection = model.primary_selection().range;
        let range = if selection.is_empty() {
            snapshot.anchor_before(selection.start)..snapshot.anchor_before(selection.start)
        } else {
            // Text typed at either end isn't the selection's.
            snapshot.anchor_after(selection.start)..snapshot.anchor_before(selection.end)
        };
        // The overlay's own anchors lean the other way at the ends: put it
        // where the range is whenever the editor changes.
        let (buffer, selected, placed) = (snapshot.id(), !selection.is_empty(), selection);
        let follow = cx.observe(editor, |this, editor, cx| this.follow(&editor, cx));
        let this = cx.weak_entity();
        let typing = cx.intercept_keystrokes(move |event, window, cx| {
            let Some(this) = this.upgrade() else {
                return;
            };
            if !this.read(cx).focus_handle(cx).contains_focused(window, cx) {
                return;
            }
            if this.update(cx, |this, cx| this.typed(&event.keystroke, cx)) {
                cx.stop_propagation();
            }
        });
        Self {
            _typing: typing,
            editor: editor.downgrade(),
            input,
            buffer,
            range,
            selected,
            phase: Phase::Asking,
            message: None,
            task: None,
            requests: 0,
            sent: String::new(),
            placed,
            _editor: Some(follow),
        }
    }

    /// Puts the overlay under the range as edits have moved it.
    fn follow(&mut self, editor: &Entity<EditorView>, cx: &mut Context<Self>) {
        // A closed input leaves the overlay alone.
        if !edits(cx).contains_key(&editor.entity_id()) {
            return;
        }
        let Some(range) = self.range(cx) else {
            return;
        };
        // The overlay's anchors lean the other way at the ends: it is under
        // the row the range ends on, or is put there.
        let text = editor.read(cx).model().text();
        let last = if range.end > range.start {
            range.end - 1
        } else {
            range.end
        };
        let row = text.offset_to_point(last).row as usize;
        if range == self.placed && editor.read(cx).overlay_anchor_row() == Some(row) {
            return;
        }
        self.placed = range.clone();
        let view = cx.entity();
        editor.update(cx, |editor, _| editor.set_overlay(Some(view.into()), range));
    }

    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.input.focus_handle(cx)
    }

    fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.input.focus_handle(cx), cx);
    }

    /// The range now, as edits have moved it; `None` if its editor is gone
    /// or what it was is lost.
    fn range(&self, cx: &gpui::App) -> Option<Range<usize>> {
        let editor = self.editor.upgrade()?;
        let snapshot = editor.read(cx).model().buffer().snapshot();
        if snapshot.id() != self.buffer
            || self.range.start.version() < snapshot.oldest_version()
            || self.range.end.version() < snapshot.oldest_version()
        {
            return None;
        }
        let range = snapshot.resolve_range(&self.range);
        // Anchors crossing each other, or an emptied selection: deleted.
        (range.start <= range.end && (!self.selected || !range.is_empty())).then_some(range)
    }

    /// Asks `provider` to carry out `instruction` on the range.
    fn generate(
        &mut self,
        provider: Arc<dyn ChatProvider>,
        settings: ChatSettings,
        instruction: String,
        root: Option<&std::path::Path>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let Some(range) = self.range(cx) else {
            return self.discard("The selection was deleted: nothing edited", window, cx);
        };
        let model = editor.read(cx).model();
        let text = model.text();
        let path = model.path().map(|path| {
            root.and_then(|root| path.strip_prefix(root).ok())
                .unwrap_or(path)
                .display()
                .to_string()
        });
        let language = (model.path())
            .and_then(|path| path.extension())
            .map(|extension| extension.to_string_lossy().into_owned())
            .unwrap_or_default();
        let input = InlineEditInput {
            path: path.unwrap_or_else(|| "untitled".to_owned()),
            language,
            text_before: text.slice_to_string(0..range.start),
            selection: text.slice_to_string(range.clone()),
            text_after: text.slice_to_string(range.end..text.len()),
            instruction,
            settings,
        };
        self.sent = input.selection.clone();
        let updates = run_inline_edit(&*provider, &input);
        self.phase = Phase::Generating;
        self.message = None;
        self.requests += 1;
        let session = format!("inline-{}", self.requests);
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            // Held while the request runs.
            let _provider = provider;
            let mut updates = Box::pin(updates);
            while let Some(update) = updates.next().await {
                let session = session.clone();
                let ended = this
                    .update_in(cx, |this, window, cx| match update {
                        InlineEditUpdate::TextSoFar(_) => false,
                        InlineEditUpdate::Done(text) => {
                            this.finish(&text, &session, window, cx);
                            true
                        }
                        InlineEditUpdate::Failed(error) => {
                            this.fail(error.message.clone(), cx);
                            true
                        }
                        InlineEditUpdate::NoCode => {
                            this.fail("The reply had no code to apply".to_owned(), cx);
                            true
                        }
                    })
                    .unwrap_or(true);
                if ended {
                    break;
                }
            }
        }));
        cx.notify();
    }

    /// The request ended without a proposal: `message` in the row, the
    /// instruction kept.
    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        self.phase = Phase::Asking;
        self.message = Some(message.into());
        if let Some(task) = self.task.take() {
            task.detach();
        }
        cx.notify();
    }

    /// The reply's `text`: proposed over the range, and the input closed.
    fn finish(&mut self, text: &str, session: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(task) = self.task.take() {
            task.detach();
        }
        let (Some(editor), Some(range)) = (self.editor.upgrade(), self.range(cx)) else {
            return self.discard(
                "The selection was deleted: the edit is discarded",
                window,
                cx,
            );
        };
        let snapshot = editor.read(cx).model().buffer().snapshot();
        if snapshot.text().slice_to_string(range.clone()) != self.sent {
            return self.discard("The selection changed: the edit is discarded", window, cx);
        }
        let proposal = to_proposal(&snapshot, range, text, session);
        let Some(first) = proposal
            .hunks(&snapshot)
            .first()
            .map(|hunk| hunk.range.start)
        else {
            self.close(window, cx);
            return toast::show(cx, Toast::info("The reply changes nothing"));
        };
        editor.update(cx, |editor, cx| {
            let mut proposals = editor.proposals().to_vec();
            proposals.push(Rc::new(RefCell::new(proposal)));
            editor.set_proposals(proposals, cx);
            editor.select_and_reveal(first..first, cx);
        });
        self.close(window, cx);
    }

    /// Closes the input with a toast saying why nothing was done.
    fn discard(&mut self, why: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
        toast::show(cx, Toast::warning(why));
    }

    /// Stops a request, or closes the input.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.phase == Phase::Generating {
            // Dropping the task drops the stream, which cancels the request.
            self.task = None;
            self.phase = Phase::Asking;
            self.message = None;
            self.focus(window, cx);
            cx.notify();
        } else {
            self.close(window, cx);
        }
    }

    /// Removes the input, and gives the editor the focus it had.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.task = None;
        let had_focus = self.focus_handle(cx).contains_focused(window, cx);
        if let Some(editor) = self.editor.upgrade() {
            let id = editor.entity_id();
            edits(cx).remove(&id);
            editor.update(cx, |editor, _| editor.clear_overlay());
            if had_focus {
                window.focus(&editor.focus_handle(cx), cx);
            }
        }
    }

    /// A key typed while the input has focus, before any binding sees it:
    /// text goes in the input, whatever the editor (vim's chords) binds it
    /// to, and the key goes no further.
    fn typed(&mut self, keystroke: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(text) = keystroke.key_char.as_deref() else {
            return false;
        };
        // Option's text on macOS has `alt`; shortcuts have `control` or
        // `platform`.
        if keystroke.modifiers.control || keystroke.modifiers.platform {
            return false;
        }
        self.message = None;
        self.input.update(cx, |input, cx| input.insert(text, cx));
        cx.notify();
        true
    }

    /// The input's editing keys.
    fn on_command(&mut self, run: &RunCommand, _: &mut Window, cx: &mut Context<Self>) {
        let edit: fn(&mut TextInput, &mut Context<TextInput>) = match picker::op(run, cx) {
            Some(PaletteOp::Backspace) => TextInput::backspace,
            Some(PaletteOp::DeleteWordLeft) => TextInput::delete_word_left,
            Some(PaletteOp::Clear) => TextInput::clear,
            Some(PaletteOp::Paste) => TextInput::paste,
            _ => {
                // The editor's own keys, which the input sits in, aren't its.
                if passes(run, cx) {
                    cx.propagate();
                }
                return;
            }
        };
        self.message = None;
        self.input.update(cx, edit);
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }

    #[cfg(test)]
    pub(crate) fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn input(&self) -> &Entity<TextInput> {
        &self.input
    }

    #[cfg(test)]
    pub(crate) fn current_range(&self, cx: &gpui::App) -> Option<Range<usize>> {
        self.range(cx)
    }
}

/// Whether the input lets command `run` on to the workbench: the input's
/// own and the workbench's, not the editor's or vim's it sits in.
fn passes(run: &RunCommand, cx: &gpui::App) -> bool {
    if run.0.command.as_str().starts_with("inline_edit.") {
        return true;
    }
    let context = typed_command(&run.0.command, cx).and_then(|command| command.context);
    context.is_some_and(|context| {
        let context = context.to_string();
        !context.contains("editor") && !context.contains("vim")
    })
}

impl Render for InlineEdit {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = ui_theme(cx);
        let status = match (self.phase, &self.message) {
            (Phase::Generating, _) => Some(("Generating…  Cancel (Esc)".into(), ui.muted_text)),
            (_, Some(message)) => Some((message.clone(), ui.text)),
            _ => None,
        };
        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_command))
            // The editor's own handler of keys (vim's) isn't for the input's.
            .on_key_down(|_, _, cx| cx.stop_propagation())
            .debug_selector(|| "inline-edit-input".into())
            .flex()
            .flex_col()
            .w(px(480.))
            .bg(ui.panel_background)
            .border_1()
            .border_color(ui.border)
            .rounded_md()
            .text_color(ui.text)
            .child(self.input.clone())
            .when_some(status, |column, (text, color): (SharedString, _)| {
                column.child(
                    div()
                        .debug_selector(|| "inline-edit-status".into())
                        .px_3()
                        .pb_1()
                        .text_color(color)
                        .child(text),
                )
            })
    }
}

#[cfg(test)]
#[path = "inline_edit_tests.rs"]
mod tests;
