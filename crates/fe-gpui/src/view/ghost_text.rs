//! Ghost text: a suggestion the host sets ([`EditorView::set_ghost_text`]),
//! shown dimmed at the caret but not part of the buffer, its first line
//! inline after the caret and its other lines in rows of their own below
//! the caret's row. Laid out and painted by [`crate::element`]'s
//! `ghost_text`.
//!
//! Each view holds its own, as it follows that view's caret. Typing what it
//! suggests consumes it; anything else that ends it
//! ([`fe_editor::ghost_text`]), or leaving vim's insert mode, drops it
//! before the next frame. While an input method composes, the composition
//! takes the caret's inlay and the suggestion hides.
//!
//! - **Keys**, in the [`KEY_CONTEXT`] while it shows: [`ACCEPT`] (`tab`)
//!   inserts the rest as one undo step, or, in vim's insert mode, as part
//!   of the insert session; [`ACCEPT_WORD`] and [`ACCEPT_LINE`] insert its
//!   next word or line, the rest still shown; [`DISMISS`] drops it. The
//!   completion menu's `tab` wins while both show.
//! - **`escape`**, wherever it's pressed, and losing focus drop it, before
//!   any binding takes the key: so one `escape` in vim's insert mode drops
//!   it and goes back to normal mode.
//! - **The palette.** Opening it blurs the view, so the suggestion is gone
//!   by the time a pick runs; the view keeps the one blur dropped, and a
//!   [`ACCEPT`], [`ACCEPT_WORD`] or [`ACCEPT_LINE`] from the palette takes
//!   it back while it's still live at the caret. Any other command or edit
//!   in the view, [`set_ghost_text`](EditorView::set_ghost_text), and the
//!   first frame drawn focused forget it, so escaping the palette or any
//!   other focus change just drops it.
//! - **Events.** The host hears each accept and the suggestion going any
//!   other way as a [`GhostTextEvent`], but its own
//!   [`set_ghost_text`](EditorView::set_ghost_text).

use fe_buffer::TransactionId;
use fe_command::{ContextStack, Invocation, Source};
use fe_editor::ghost_text::GhostText;
use gpui::{Context, EventEmitter};

use super::EditorView;

pub(crate) const ACCEPT: &str = "ghost_text.accept";
pub(crate) const ACCEPT_WORD: &str = "ghost_text.accept_word";
pub(crate) const ACCEPT_LINE: &str = "ghost_text.accept_line";
pub(crate) const DISMISS: &str = "ghost_text.dismiss";

/// The key context the suggestion's keys are bound in while it shows.
pub(crate) const KEY_CONTEXT: &str = "ghost_text";

/// What an [`EditorView`] tells its subscribers the user did with its
/// suggestion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GhostTextEvent {
    /// `text`, some or all of what remained, went in as `transaction`, the
    /// user's; `whole` if nothing of it remains to show. One accepted from
    /// the command palette follows the `Dismissed` its blur sent.
    Accepted {
        transaction: TransactionId,
        text: String,
        whole: bool,
    },
    /// It went without being accepted: `escape`, losing focus,
    /// `ghost_text.dismiss`, or anything that ended it, typing all of it
    /// among others. Not for one the host replaced or cleared.
    Dismissed,
}

impl EventEmitter<GhostTextEvent> for EditorView {}

impl EditorView {
    /// Shows `ghost` from the next frame, in place of any shown before, or
    /// nothing with `None`. One that doesn't show at the caret now, such as
    /// one for another buffer or one set in vim's normal mode, is dropped
    /// with a warning. Neither tells subscribers anything.
    pub fn set_ghost_text(&mut self, ghost: Option<GhostText>, cx: &mut Context<Self>) {
        // Made against the buffer as it is, which the model may trail.
        self.model.sync_display();
        self.sync_vim(cx);
        let accepts_text = self.accepts_text();
        self.blurred_ghost_text = None;
        self.ghost_text = ghost.filter(|ghost| {
            let live = accepts_text && ghost.is_live(&self.model);
            if !live {
                let buffer = self.model.snapshot().id();
                log::warn!(
                    "a suggestion that doesn't show at the caret of {buffer:?}, \
                     is for another buffer or comes in a vim mode that takes no \
                     text, isn't shown"
                );
            }
            live
        });
        cx.notify();
    }

    /// The suggestion shown, if any.
    pub fn ghost_text(&self) -> Option<&GhostText> {
        self.ghost_text.as_ref()
    }

    /// Whether the suggestion shows, so its keys apply: it's live, and no
    /// input method composes over it.
    pub(crate) fn ghost_text_shown(&self) -> bool {
        self.ghost_text.is_some() && self.composition().is_none()
    }

    /// Drops a suggestion that's over, or that vim's normal or visual mode
    /// hides, for a frame about to be drawn or a key about to be matched,
    /// and tells subscribers. Checked against the buffer as it is, so an
    /// edit through another view of it counts.
    pub(crate) fn drop_stale_ghost_text(&mut self, cx: &mut Context<Self>) {
        if self.ghost_text.is_none() {
            return;
        }
        self.model.sync_display();
        let accepts_text = self.accepts_text();
        if (self.ghost_text.as_ref())
            .is_some_and(|ghost| !accepts_text || !ghost.is_live(&self.model))
        {
            self.dismiss_ghost_text(cx);
        }
    }

    /// Drops the suggestion, if any, telling subscribers.
    pub(crate) fn dismiss_ghost_text(&mut self, cx: &mut Context<Self>) {
        if self.ghost_text.take().is_some() {
            cx.emit(GhostTextEvent::Dismissed);
            cx.notify();
        }
    }

    /// Has losing focus drop the suggestion, kept for the palette. `escape`
    /// drops it too, through [`dismiss_ghost_text_on_escape`], before any
    /// binding takes it.
    pub(crate) fn dismiss_ghost_text_on_blur(
        view: &mut EditorView,
        window: &mut gpui::Window,
        cx: &mut Context<EditorView>,
    ) {
        // Blur listeners run at the end of a draw, which asks for no frame
        // after it: the redraw waits until it's over.
        let blur = cx.on_blur(&view.focus, window, |view, window, cx| {
            if let Some(ghost) = view.ghost_text.clone() {
                view.dismiss_ghost_text(cx);
                view.blurred_ghost_text = Some((ghost, view.model.snapshot().version()));
                cx.defer_in(window, |_, _, cx| cx.notify());
            }
        });
        view.ghost_text_subscriptions = vec![blur];
    }

    /// Forgets the suggestion blur dropped, for a frame drawn while the view
    /// has focus: focus came back without a palette pick taking it.
    pub(crate) fn forget_blurred_ghost_text_if_focused(&mut self, window: &gpui::Window) {
        if self.focus.is_focused(window) {
            self.blurred_ghost_text = None;
        }
    }

    /// Forgets the suggestion blur dropped, for `invocation` about to run in
    /// the view, unless it's an accept from the palette, which takes it.
    pub(crate) fn forget_blurred_ghost_text_unless_taken(&mut self, invocation: &Invocation) {
        let accept = [ACCEPT, ACCEPT_WORD, ACCEPT_LINE].contains(&invocation.command.as_str());
        if !(accept && invocation.source == Source::Palette) {
            self.blurred_ghost_text = None;
        }
    }

    /// Runs one of the suggestion's commands, which the registry has
    /// dispatched from `source`. `true` if `command` is one. An accept from
    /// the palette takes back the suggestion opening it dropped, if it's
    /// still live.
    pub(crate) fn run_ghost_text_command(
        &mut self,
        command: &str,
        source: Source,
        cx: &mut Context<Self>,
    ) -> bool {
        let accept = match command {
            ACCEPT => GhostText::accept,
            ACCEPT_WORD => GhostText::accept_word,
            ACCEPT_LINE => GhostText::accept_line,
            DISMISS => {
                self.dismiss_ghost_text(cx);
                return true;
            }
            _ => return false,
        };
        self.drop_stale_ghost_text(cx);
        let blurred = self.blurred_ghost_text.take();
        if self.ghost_text.is_none() && source == Source::Palette {
            self.model.sync_display();
            let accepts_text = self.accepts_text();
            let version = self.model.snapshot().version();
            self.ghost_text = blurred.and_then(|(ghost, saved)| {
                (saved == version && accepts_text && ghost.is_live(&self.model)).then_some(ghost)
            });
        }
        let Some(ghost) = self.ghost_text.take() else {
            return true;
        };
        let before = ghost.remaining(&self.model).unwrap_or_default().to_owned();
        let Some(transaction) = accept(&ghost, &mut self.model) else {
            return true;
        };
        let after = ghost.remaining(&self.model).map_or(0, str::len);
        let text = before[..before.len().saturating_sub(after)].to_owned();
        // A partial accept keeps the rest, while there's any.
        let whole = command == ACCEPT || after == 0;
        if !whole {
            self.ghost_text = Some(ghost);
        }
        cx.emit(GhostTextEvent::Accepted {
            transaction,
            text,
            whole,
        });
        self.changed(cx);
        true
    }
}

/// `stack` with its innermost editor frame's [`KEY_CONTEXT`] flag set as
/// `shown` says: keys are matched against the suggestion as it is when they
/// come, as vim's are ([`crate::vim::live_stack`]).
pub(crate) fn stamp(stack: ContextStack, shown: bool) -> ContextStack {
    let frames = stack.frames();
    let editor = (frames.iter()).rposition(|frame| frame.name == fe_editor::commands::CONTEXT);
    (frames.iter().enumerate())
        .map(|(ix, frame)| {
            let mut frame = frame.clone();
            if Some(ix) == editor {
                frame.flags.remove(KEY_CONTEXT);
                if shown {
                    frame.flags.insert(KEY_CONTEXT.to_owned());
                }
            }
            frame
        })
        .collect()
}

/// Holds the app-wide `escape` interceptor.
struct EscapeInterceptor(#[allow(dead_code)] gpui::Subscription);

impl gpui::Global for EscapeInterceptor {}

/// Has an unmodified `escape` drop the focused editor's suggestion before
/// any key binding is matched; the keystroke then goes on as usual, so vim
/// still leaves insert mode. Installed once: a second call does nothing.
/// Call it before the keymap's interceptor, which runs in registration
/// order.
pub(crate) fn dismiss_ghost_text_on_escape(cx: &mut gpui::App) {
    if cx.has_global::<EscapeInterceptor>() {
        return;
    }
    let subscription = cx.intercept_keystrokes(|event, window, cx| {
        if event.keystroke.key != "escape" || event.keystroke.modifiers.modified() {
            return;
        }
        if let Some(editor) = crate::vim::focused_editor(window, cx) {
            editor.update(cx, |view, cx| view.dismiss_ghost_text(cx));
        }
    });
    cx.set_global(EscapeInterceptor(subscription));
}
