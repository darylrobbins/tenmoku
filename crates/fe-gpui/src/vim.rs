//! Vim mode in the editor: one app-wide switch, each editor's
//! [`VimState`], and the keys `fe-vim`'s reducer reads. The design is
//! `docs/decisions/vim-engine.md`, §9 and §10.
//!
//! With vim on, a focused editor's key context is
//! `editor vim_mode=<mode>`, plus `vim_pending` and `vim_pending_kind`
//! while a command is incomplete; [`crate::keymap`] reads them as
//! `vim.mode`, `vim.pending` and `vim.pending_kind`. A keystroke in normal
//! or operator-pending mode goes:
//!
//! 1. to the keymap, as always, unless a command is pending. A match runs,
//!    except `vim.key`, which only says the keystroke is vim's here;
//! 2. otherwise to the editor's key listener, [`EditorView::on_vim_key`],
//!    which feeds it to the reducer: `Wait` and `Cancel` swallow it,
//!    `Commit` runs the invocation through `fe-vim`'s registry, and
//!    `Forward` lets it go on, to type nothing: no input handler is
//!    registered outside insert mode.
//!
//! The keymap matches against each editor's live state, stamped onto its
//! frame before matching ([`live_stack`]), not the context the last frame
//! drew, which can trail the keys typed since.

use std::rc::Rc;

use fe_buffer::TransactionId;
use fe_command::{ContextStack, DispatchError, Dispatched, Invocation, Registry};
use fe_vim::{Registers, Step, VimCommands, VimKey, VimState, VimTarget};
use gpui::{
    App, Context, Entity, FocusHandle, Global, KeyContext, KeyDownEvent, WeakEntity,
    WeakFocusHandle, Window,
};

use crate::{EditorView, commands::ViewHost, view::synced_syntax};

/// The command a vim-layer binding names to hand its keystroke to the
/// reducer. It's never dispatched from a key.
pub(crate) const KEY: &str = "vim.key";
/// Flips the switch, for this session.
pub(crate) const TOGGLE: &str = "vim.toggle";

/// The prefix of vim's keys in GPUI's key contexts, whose identifiers can't
/// hold `.`: `vim_mode` is `vim.mode`.
pub(crate) const GPUI_PREFIX: &str = "vim_";

/// The switch, and every editor, by its focus handle, so a keystroke can
/// find the focused one's live state.
#[derive(Default)]
struct Vim {
    enabled: bool,
    /// Bumped each time the switch flips, so an editor that hasn't seen the
    /// flip yet drops its state, even if the switch flipped back since.
    generation: u64,
    editors: Vec<(WeakFocusHandle, WeakEntity<EditorView>)>,
    /// Shared by every editor, as Vim's are by every window.
    registers: Registers,
}

impl Global for Vim {}

thread_local! {
    /// Built once, as `EDITOR_COMMANDS` is.
    static VIM_COMMANDS: Rc<Registry<VimCommands>> = Rc::new(fe_vim::registry());
}

/// `fe-vim`'s registry.
pub(crate) fn registry() -> Rc<Registry<VimCommands>> {
    VIM_COMMANDS.with(Rc::clone)
}

/// Turns vim on or off in every editor, and redraws every window. Turning
/// it off drops each editor's pending command. Turning it on moves no
/// caret: the first vim motion puts carets where normal mode keeps them.
pub fn set_vim_enabled(cx: &mut App, enabled: bool) {
    let vim = cx.default_global::<Vim>();
    if vim.enabled != enabled {
        vim.enabled = enabled;
        vim.generation += 1;
    }
    let editors: Vec<_> = vim.editors.iter().map(|(_, view)| view.clone()).collect();
    // Once no view is being updated: so the status bar, which observes its
    // editor, sees the mode.
    cx.defer(move |cx| {
        for editor in editors {
            editor
                .update(cx, |view, cx| {
                    if view.sync_vim(cx) {
                        cx.notify();
                    }
                })
                .ok();
        }
    });
    cx.refresh_windows();
}

/// Whether vim is on.
pub fn vim_enabled(cx: &App) -> bool {
    cx.try_global::<Vim>().is_some_and(|vim| vim.enabled)
}

/// The switch's state: whether it's on, and its generation.
fn switch(cx: &App) -> (bool, u64) {
    cx.try_global::<Vim>()
        .map_or((false, 0), |vim| (vim.enabled, vim.generation))
}

/// Lists `view`'s editor, focused by `focus`, until it's released.
pub(crate) fn register(focus: &FocusHandle, cx: &mut Context<EditorView>) {
    let view = cx.weak_entity();
    let id = view.entity_id();
    cx.default_global::<Vim>()
        .editors
        .push((focus.downgrade(), view));
    cx.on_release(move |_, cx| {
        cx.default_global::<Vim>()
            .editors
            .retain(|(_, view)| view.entity_id() != id);
    })
    .detach();
}

/// The focused editor, if the focused element is one.
fn focused_editor(window: &Window, cx: &App) -> Option<Entity<EditorView>> {
    let focused = window.focused(cx)?;
    cx.try_global::<Vim>()?
        .editors
        .iter()
        .find(|(focus, _)| *focus == focused)?
        .1
        .upgrade()
}

/// The focused editor, if vim owns its keys: vim is on and it isn't in
/// insert mode.
pub(crate) fn vim_editor(window: &Window, cx: &mut App) -> Option<Entity<EditorView>> {
    let editor = focused_editor(window, cx)?;
    editor
        .update(cx, |view, cx| {
            view.sync_vim(cx);
            view.vim.is_some() && !view.accepts_text()
        })
        .then_some(editor)
}

/// Feeds `keystrokes` to `editor`'s reducer, each as its key listener
/// would, with no event dispatched: so GPUI's propagation flag, shared by
/// every event, is left as it was.
pub(crate) fn feed(
    editor: &Entity<EditorView>,
    keystrokes: &[gpui::Keystroke],
    window: &mut Window,
    cx: &mut App,
) {
    for keystroke in keystrokes {
        editor.update(cx, |view, cx| view.feed_vim(keystroke, window, cx));
    }
}

/// The context a keystroke is matched in: `stack`, the focused element's
/// as last drawn, with the focused editor's frame stamped from its live
/// vim state, or with no vim keys if vim is off. Also whether a vim
/// command is pending there, in which case the editor owns the keystroke
/// (§9.1).
pub(crate) fn live_stack(
    stack: ContextStack,
    window: &Window,
    cx: &mut App,
) -> (ContextStack, bool) {
    let Some(editor) = focused_editor(window, cx) else {
        return (stack, false);
    };
    editor.update(cx, |view, cx| {
        view.sync_vim(cx);
        let pending = view
            .vim
            .as_ref()
            .is_some_and(|state| state.pending().kind().is_some());
        (stamp(stack, view.vim.as_ref()), pending)
    })
}

/// `stack` with its innermost editor frame's vim keys replaced by
/// `state`'s, or removed for `None`.
pub(crate) fn stamp(stack: ContextStack, state: Option<&VimState>) -> ContextStack {
    let frames = stack.frames();
    let Some(editor) = frames
        .iter()
        .rposition(|frame| frame.name == fe_editor::commands::CONTEXT)
    else {
        return stack;
    };
    frames
        .iter()
        .enumerate()
        .map(|(ix, frame)| {
            let mut frame = frame.clone();
            if ix == editor {
                frame.flags.retain(|flag| !flag.starts_with("vim."));
                frame.values.retain(|key, _| !key.starts_with("vim."));
                if let Some(state) = state {
                    frame = state.stamp(frame);
                }
            }
            frame
        })
        .collect()
}

/// `state`'s keys on the editor's GPUI key context, in GPUI's spelling.
pub(crate) fn add_to_key_context(context: &mut KeyContext, state: &VimState) {
    let frame = state.stamp(fe_command::ContextFrame::new(""));
    let gpui_key = |key: &str| key.replacen("vim.", GPUI_PREFIX, 1);
    for flag in &frame.flags {
        context.add(gpui_key(flag));
    }
    for (key, value) in &frame.values {
        context.set(gpui_key(key), value.clone());
    }
}

/// The key the reducer reads for `keystroke`, if any: none with `cmd`;
/// `ctrl-[` is `<Esc>` and `ctrl-<letter>` is `<C-letter>`; `escape`,
/// `enter`, `backspace`, `tab` and the arrows are those keys; otherwise the
/// character typed, case kept, so `shift-4` is `$`.
pub(crate) fn vim_key(keystroke: &gpui::Keystroke) -> Option<VimKey> {
    let held = &keystroke.modifiers;
    let key = keystroke.key.as_str();
    if held.platform {
        return None;
    }
    if held.control {
        return match key.as_bytes() {
            b"[" => Some(VimKey::Esc),
            &[letter] if letter.is_ascii_alphabetic() => {
                Some(VimKey::Ctrl(letter.to_ascii_lowercase() as char))
            }
            _ => named(key),
        };
    }
    named(key).or_else(|| {
        let mut typed = keystroke.key_char.as_deref()?.chars();
        let ch = typed.next()?;
        typed.next().is_none().then_some(VimKey::Char(ch))
    })
}

fn named(key: &str) -> Option<VimKey> {
    Some(match key {
        "escape" => VimKey::Esc,
        "enter" => VimKey::Enter,
        "backspace" => VimKey::Backspace,
        "tab" => VimKey::Tab,
        "left" => VimKey::Left,
        "right" => VimKey::Right,
        "up" => VimKey::Up,
        "down" => VimKey::Down,
        _ => return None,
    })
}

impl EditorView {
    /// Creates the vim state when vim is on and drops it when it's off.
    /// Returns whether that changed anything.
    pub(crate) fn sync_vim(&mut self, cx: &App) -> bool {
        let (enabled, generation) = switch(cx);
        if self.vim_generation == generation {
            return false;
        }
        self.vim_generation = generation;
        let was_on = self.vim.is_some();
        self.vim = enabled.then(VimState::new);
        was_on || enabled
    }

    /// The vim mode, `vim.mode`'s value (`normal`, `operator_pending`, …),
    /// or `None` with vim off.
    pub fn vim_mode(&self) -> Option<&'static str> {
        self.vim.as_ref().map(VimState::mode_name)
    }

    /// Whether typing and input methods insert text: with vim off, or in
    /// insert mode.
    pub(crate) fn accepts_text(&self) -> bool {
        self.vim_mode().is_none_or(|mode| mode == "insert")
    }

    /// Whether carets paint as blocks: in normal and operator-pending mode.
    pub(crate) fn block_cursor(&self) -> bool {
        matches!(self.vim_mode(), Some("normal" | "operator_pending"))
    }

    /// A keystroke the keymap left for the editor, in normal or
    /// operator-pending mode: fed to the reducer.
    pub(crate) fn on_vim_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.feed_vim(&event.keystroke, window, cx) {
            cx.stop_propagation();
        }
    }

    /// Feeds `keystroke` to the reducer, in normal or operator-pending
    /// mode. Returns whether vim took it: it waits, cancels or commits.
    /// Otherwise it's forwarded, to do nothing.
    fn feed_vim(
        &mut self,
        keystroke: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.sync_vim(cx);
        if self.accepts_text() {
            return false;
        }
        let Some(state) = &mut self.vim else {
            return false;
        };
        let Some(key) = vim_key(keystroke) else {
            // A key vim has no name for cancels a pending command, and
            // otherwise goes on, to do nothing.
            if state.pending().kind().is_none() {
                return false;
            }
            state.feed(VimKey::Esc);
            cx.notify();
            return true;
        };
        match state.feed(key) {
            Step::Wait(_) | Step::Cancel => {
                // The key context changes.
                cx.notify();
                true
            }
            Step::Forward => false,
            Step::Commit(invocation) => {
                // The stack the key came in, stamped as the reducer left
                // the state: nothing pending.
                let stack = crate::keymap::context_stack(&window.context_stack());
                let invocation = invocation.context(stamp(stack, self.vim.as_ref()));
                self.run_vim(&invocation, cx);
                cx.notify();
                true
            }
        }
    }

    /// Runs `invocation` through `fe-vim`'s registry, as `run_command` runs
    /// the editor's. Its context is used as given.
    pub(crate) fn run_vim(&mut self, invocation: &Invocation, cx: &mut Context<Self>) {
        let registry = registry();
        let Some(result) = self.dispatch_vim(&registry, invocation, cx) else {
            log::warn!("{} needs vim on", invocation.command);
            return;
        };
        let mutates = registry
            .get(&invocation.command)
            .is_some_and(|meta| meta.mutates_buffer);
        if let Err(error) = self.dispatched(result, invocation, mutates, cx) {
            log::warn!("{error}");
        }
    }

    /// Builds the [`VimTarget`] and dispatches; `None` with vim off.
    fn dispatch_vim(
        &mut self,
        registry: &Registry<VimCommands>,
        invocation: &Invocation,
        cx: &mut App,
    ) -> Option<Result<Dispatched<TransactionId>, DispatchError>> {
        let page_rows = self.page_rows();
        let syntax = synced_syntax(&self.syntax, &self.model);
        let state = self.vim.as_mut()?;
        // Out of the global while the host holds `cx`, and back after.
        let mut registers = std::mem::take(&mut cx.default_global::<Vim>().registers);
        let mut host = ViewHost {
            cx,
            page_rows,
            syntax,
        };
        let mut target = VimTarget {
            model: &mut self.model,
            host: &mut host,
            state,
            registers: &mut registers,
        };
        let result = registry.dispatch(&mut target, invocation);
        cx.default_global::<Vim>().registers = registers;
        Some(result)
    }
}

#[cfg(test)]
mod tests;
