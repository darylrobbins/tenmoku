//! Key input through kiln-platform's [`kiln_platform::keys`]: a keystroke
//! interceptor feeds every keystroke, with the focused element's context, to
//! one matcher, and dispatches what it matches as a [`RunCommand`] to the
//! focused element. Chords, their timeout and the replay of a failed one
//! are kiln's; see that module.
//!
//! Vim takes part as the [`KeyOwner`]. With vim on, the focused editor's
//! frame is stamped from its live vim state before matching, and while a
//! vim command is pending the matcher isn't consulted: the editor owns
//! every keystroke. A match to `vim.key` isn't dispatched either; the
//! chord's keystrokes go to vim, the last on to the editor's key listener.
//! Keystrokes a failed chord swallowed go to vim's reducer, all of them.
//! See [`crate::vim`].
//!
//! [`RunCommand`]: crate::commands::RunCommand

use std::sync::Arc;

use fe_command::{ContextStack, Invocation};
use fe_keymap::{Keymap, KeymapIssue};
use gpui::{App, KeyContext, Window};
use kiln_platform::keys::{self, KeyOwner};

pub use kiln_platform::keys::{CHORD_TIMEOUT, modifiers};

/// Vim's part in key input, through the hooks in `crate::vim` and the
/// editor's hover.
struct VimKeys;

impl KeyOwner for VimKeys {
    fn live_stack(
        &self,
        stack: ContextStack,
        window: &mut Window,
        cx: &mut App,
    ) -> (ContextStack, bool) {
        crate::vim::live_stack(stack, window, cx)
    }

    fn owns(&self, invocation: &Invocation) -> bool {
        invocation.command.as_str() == crate::vim::KEY
    }

    fn replay(&self, keystrokes: &[gpui::Keystroke], window: &mut Window, cx: &mut App) -> bool {
        let Some(editor) = crate::vim::vim_editor(window, cx) else {
            return false;
        };
        crate::vim::feed(&editor, keystrokes, window, cx);
        true
    }

    fn modifiers_released(&self, modifiers: &gpui::Modifiers, window: &mut Window, cx: &mut App) {
        // GPUI sends this, a modifier released alone, wherever focus is,
        // unlike its modifier change: so no editor's hover range outlives
        // cmd while a panel has focus.
        crate::element::hover_with_modifiers(modifiers, window, cx);
    }

    fn chord_changed(&self, window: &mut Window, cx: &mut App) {
        // The which-key popup lists what goes on from a chord held.
        crate::vim::which_key::chord_changed(window, cx);
    }
}

/// The namespace of vim's key context keys: `vim_mode` is `vim.mode`.
const VIM_NAMESPACE: &str = "vim";

/// The built-in keymap, [`fe_keymap::DEFAULT_MACOS`], alone.
pub fn default_keymap() -> Keymap {
    fe_keymap::load_keymap(fe_keymap::DEFAULT_MACOS, None).0
}

/// Installs the interceptor, matching against `keymap`, with vim as its
/// key owner. Called again, it only swaps the keymap.
pub(crate) fn init(keymap: Arc<Keymap>, cx: &mut App) {
    crate::view::ghost_text::dismiss_ghost_text_on_escape(cx);
    keys::init(cx, keymap);
    keys::set_key_owner(cx, VimKeys);
    keys::register_context_namespace(cx, VIM_NAMESPACE);
}

/// Swaps the keymap, as on hot reload, dropping any chord in progress.
/// `keymap` is the defaults with the user's layer over them, as
/// [`fe_keymap::KeymapStore`] builds it: each layer's last good version.
/// Menus [`crate::menu::install`]ed are reinstalled, so their shortcuts
/// follow; without any, it touches neither GPUI's menus nor its keymap.
/// Windows redraw, so an editor's hover range follows too.
pub fn set_user_keymap(cx: &mut App, keymap: impl Into<Arc<Keymap>>) {
    keys::set_keymap(cx, keymap);
}

/// The keymap keys are matched against: empty before [`crate::init`].
pub(crate) fn current(cx: &App) -> Arc<Keymap> {
    keys::current_keymap(cx)
}

/// The keymap's issues ([`Keymap::report_with`]) in the one context there
/// is so far, `[editor]`.
pub fn keymap_report(cx: &App) -> Vec<KeymapIssue> {
    current(cx).report_with(&[fe_editor::commands::context()])
}

/// Whether a chord is pending, in the adapter or the matcher.
#[cfg(test)]
pub(crate) fn chord_pending(cx: &App) -> bool {
    keys::chord_pending(cx)
}

/// What `keystroke` alone runs in `stack` with the keymap keys are matched
/// against, leaving any pending chord be: for a click bound like a key,
/// such as `cmd-click`.
pub(crate) fn lookup(
    keystroke: &fe_keymap::Keystroke,
    stack: &ContextStack,
    cx: &App,
) -> Option<Invocation> {
    keys::lookup(keystroke, stack, cx)
}

/// GPUI's key contexts, outermost first, as a context stack, with vim's
/// keys read as `vim.` ones: GPUI's identifiers can't hold `.`.
pub(crate) fn context_stack(contexts: &[KeyContext]) -> ContextStack {
    keys::context_stack_with(contexts, &[VIM_NAMESPACE])
}
