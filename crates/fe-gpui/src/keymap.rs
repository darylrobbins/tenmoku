//! Key input through `fe-keymap`. GPUI's own keymap is unused: a keystroke
//! interceptor feeds every keystroke, with the focused element's context, to
//! one [`Matcher`], and dispatches what it matches as a
//! [`RunCommand`] to the focused element.
//!
//! A keystroke that matches nothing propagates as usual, so text input
//! reaches the view. Keystrokes swallowed by a chord that then fails are
//! re-delivered as input (those without a character, such as `cmd-k`,
//! deliver nothing), and the keystroke that failed it is matched again
//! alone, as a fresh one, before it propagates: `cmd-k left` moves left. A
//! key `fe-keymap` has no name for, such as `f21`, fails a chord the same
//! way, then propagates unmatched.
//!
//! A chord belongs to the window and focused element it started in. It is
//! abandoned, neither run nor typed, once focus leaves that element, the
//! window closes, or a keystroke comes from anywhere else. Focus that
//! leaves and comes back before the next frame is drawn goes unseen; see
//! `PendingChord::start`.

use std::{sync::Arc, time::Duration};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_keymap::{Key, KeyOutcome, Keymap, KeymapIssue, Keystroke, Matcher, Modifiers};
use gpui::{
    AnyWindowHandle, App, Global, KeyContext, KeystrokeEvent, Subscription, Task, WeakFocusHandle,
    Window,
};

use crate::commands::RunCommand;

/// How long a chord prefix waits for its next keystroke, after which
/// [`Matcher::flush`] ends the chord.
pub const CHORD_TIMEOUT: Duration = Duration::from_secs(1);

struct KeyInput {
    matcher: Matcher,
    /// The chord the matcher holds pending, if any.
    pending: Option<PendingChord>,
    /// Set while swallowed keystrokes are re-delivered, which the matcher
    /// has seen already.
    redelivering: bool,
    _interceptor: Subscription,
    _window_closed: Subscription,
}

impl Global for KeyInput {}

impl KeyInput {
    /// Ends the pending chord without running or re-delivering any of it.
    fn abandon(&mut self) {
        self.matcher.flush();
        self.pending = None;
    }
}

/// Where keystrokes go: a window and its focused element, if any.
#[derive(Clone, PartialEq)]
struct Owner {
    window: AnyWindowHandle,
    focus: Option<WeakFocusHandle>,
}

impl Owner {
    fn of(window: &Window, cx: &App) -> Self {
        Self {
            window: window.window_handle(),
            focus: window.focused(cx).map(|focus| focus.downgrade()),
        }
    }
}

struct PendingChord {
    owner: Owner,
    /// The GPUI keystrokes of the chord, to re-deliver if it fails.
    swallowed: Vec<gpui::Keystroke>,
    /// Ends the chord; dropping it cancels that.
    _timeout: Task<()>,
    /// Abandons the chord when focus leaves its element.
    _focus_out: Option<Subscription>,
}

impl PendingChord {
    fn start(
        owner: Owner,
        swallowed: Vec<gpui::Keystroke>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let handle = owner.window;
        let timeout = cx.spawn(async move |cx| {
            cx.background_executor().timer(CHORD_TIMEOUT).await;
            cx.update(|cx| on_timeout(handle, cx));
        });
        // Known limit: focus that moves away and back with no draw in
        // between, A -> B -> A, isn't seen, and the chord carries on in A.
        // GPUI reports focus changes only at draw time, by comparing the
        // focus path of the frame drawn before with the new one; that's
        // when `on_focus_out`, `on_focus` and `on_focus_lost` fire. The
        // generation `Window::focus` bumps synchronously, `focus_generation`,
        // is `pub(crate)` in gpui-pre 0.3.7, so nothing here sees each move
        // as it happens.
        //
        // No keystroke can make that round trip while a chord is pending:
        // each one continues the chord, swallowed, or ends it before
        // anything runs or propagates. A modifier pressed alone is the one
        // exception, and nothing in fastedit moves focus on one. What's left
        // is a mouse, timer or menu moving focus away and back within one
        // frame.
        //
        // The fix is a focus generation checked on continuation, timeout
        // and replay. Lift this limit once GPUI exposes a synchronous focus
        // signal, or once every focus move goes through a wrapper of ours.
        let focus_out = window.focused(cx).map(|focus| {
            // Dropped with the chord, so it fires only for this one.
            window.on_focus_out(&focus, cx, |_, _, cx| {
                cx.global_mut::<KeyInput>().abandon();
            })
        });
        Self {
            owner,
            swallowed,
            _timeout: timeout,
            _focus_out: focus_out,
        }
    }
}

/// The built-in keymap, [`fe_keymap::DEFAULT_MACOS`], alone.
pub fn default_keymap() -> Keymap {
    fe_keymap::load_keymap(fe_keymap::DEFAULT_MACOS, None).0
}

/// Installs the interceptor, matching against `keymap`. Called again, it
/// only swaps the keymap.
pub(crate) fn init(keymap: Arc<Keymap>, cx: &mut App) {
    if cx.has_global::<KeyInput>() {
        set_user_keymap(cx, keymap);
        return;
    }
    let interceptor = cx.intercept_keystrokes(on_keystroke);
    let window_closed = cx.on_window_closed(|cx, closed| {
        let input = cx.global_mut::<KeyInput>();
        if let Some(chord) = &input.pending
            && chord.owner.window.window_id() == closed
        {
            input.abandon();
        }
    });
    cx.set_global(KeyInput {
        matcher: Matcher::new(keymap),
        pending: None,
        redelivering: false,
        _interceptor: interceptor,
        _window_closed: window_closed,
    });
}

/// Swaps the keymap, as on hot reload, dropping any chord in progress.
/// `keymap` is the defaults with the user's layer over them, as
/// [`fe_keymap::KeymapStore`] builds it: each layer's last good version.
/// Menus [`crate::menu::install`]ed are reinstalled, so their shortcuts
/// follow; without any, it touches neither GPUI's menus nor its keymap.
pub fn set_user_keymap(cx: &mut App, keymap: impl Into<Arc<Keymap>>) {
    let input = cx.global_mut::<KeyInput>();
    input.matcher.set_keymap(keymap);
    input.pending = None;
    crate::menu::refresh(cx);
}

/// The keymap keys are matched against: empty before [`crate::init`].
pub(crate) fn current(cx: &App) -> Arc<Keymap> {
    cx.try_global::<KeyInput>()
        .map(|input| input.matcher.keymap().clone())
        .unwrap_or_default()
}

/// The keymap's issues ([`Keymap::report_with`]) in the one context there
/// is so far, `[editor]`.
pub fn keymap_report(cx: &App) -> Vec<KeymapIssue> {
    let keymap = cx.global::<KeyInput>().matcher.keymap();
    keymap.report_with(&[fe_editor::commands::context()])
}

/// Whether a chord is pending, in the adapter or the matcher.
#[cfg(test)]
pub(crate) fn chord_pending(cx: &App) -> bool {
    let input = cx.global::<KeyInput>();
    input.pending.is_some() || !input.matcher.pending().is_empty()
}

fn on_keystroke(event: &KeystrokeEvent, window: &mut Window, cx: &mut App) {
    if cx
        .try_global::<KeyInput>()
        .is_none_or(|input| input.redelivering)
    {
        return;
    }
    let owner = Owner::of(window, cx);
    let input = cx.global_mut::<KeyInput>();
    // A chord started in another window or element doesn't continue here.
    if input
        .pending
        .as_ref()
        .is_some_and(|chord| chord.owner != owner)
    {
        input.abandon();
    }
    let Some(keystroke) = keystroke(&event.keystroke) else {
        // A standalone modifier goes on as GPUI sends it, leaving the chord
        // be. Any other key `fe-keymap` has no name for fails the chord, as
        // one that matches nothing would, then goes on unmatched.
        if !is_modifier(&event.keystroke) {
            let failed_chord = input.pending.take().map(|chord| chord.swallowed);
            input.matcher.flush();
            redeliver(failed_chord.unwrap_or_default(), window, cx);
        }
        return;
    };
    // Taking the chord cancels its timer.
    let mut swallowed = input
        .pending
        .take()
        .map_or_else(Vec::new, |chord| chord.swallowed);
    let stack = context_stack(&event.context_stack);
    let mut outcome = input.matcher.push(keystroke, &stack);
    let mut failed_chord = Vec::new();
    if let KeyOutcome::Unmatched(keys) = &outcome
        && keys.len() > 1
    {
        // This keystroke ended a chord that matched nothing: match it again
        // as a fresh one.
        failed_chord = std::mem::take(&mut swallowed);
        outcome = input.matcher.push(keystroke, &stack);
    }
    redeliver(failed_chord, window, cx);
    match outcome {
        KeyOutcome::Dispatch(invocation) => {
            dispatch(invocation, window, cx);
            cx.stop_propagation();
        }
        KeyOutcome::Pending => {
            swallowed.push(event.keystroke.clone());
            let chord = PendingChord::start(owner, swallowed, window, cx);
            cx.global_mut::<KeyInput>().pending = Some(chord);
            cx.stop_propagation();
        }
        // Only this keystroke, which goes on to the view.
        KeyOutcome::Unmatched(_) => {}
    }
}

/// Ends the pending chord, started in `handle`, when no keystroke came in
/// time. Nothing runs if its element lost focus. If its window closed, the
/// chord was abandoned then.
fn on_timeout(handle: AnyWindowHandle, cx: &mut App) {
    handle
        .update(cx, |_, window, cx| {
            let owner = Owner::of(window, cx);
            let input = cx.global_mut::<KeyInput>();
            let Some(chord) = input.pending.take() else {
                return;
            };
            let outcome = input.matcher.flush();
            if chord.owner != owner {
                return;
            }
            match outcome {
                KeyOutcome::Dispatch(invocation) => dispatch(invocation, window, cx),
                KeyOutcome::Unmatched(_) => redeliver(chord.swallowed, window, cx),
                KeyOutcome::Pending => unreachable!("flush ends the chord"),
            }
        })
        .ok();
}

/// Runs `invocation` on the focused element, synchronously, as GPUI's own
/// keymap did; without one, on the window. Menu items run through it too.
pub(crate) fn dispatch(invocation: Invocation, window: &mut Window, cx: &mut App) {
    let action = RunCommand(invocation);
    match window.focused(cx) {
        Some(focus) => focus.dispatch_action(&action, window, cx),
        None => window.dispatch_action(Box::new(action), cx),
    }
}

/// Delivers swallowed keystrokes as input, past the matcher. Those without
/// a character deliver nothing.
fn redeliver(keystrokes: Vec<gpui::Keystroke>, window: &mut Window, cx: &mut App) {
    let typed: Vec<_> = keystrokes
        .into_iter()
        .filter(|keystroke| keystroke.key_char.is_some())
        .collect();
    if typed.is_empty() {
        return;
    }
    cx.global_mut::<KeyInput>().redelivering = true;
    for keystroke in typed {
        window.dispatch_keystroke(keystroke, cx);
    }
    cx.global_mut::<KeyInput>().redelivering = false;
}

/// Whether `keystroke` is a modifier pressed and released alone, which GPUI
/// sends named after it.
fn is_modifier(keystroke: &gpui::Keystroke) -> bool {
    matches!(
        keystroke.key.as_str(),
        "shift" | "control" | "alt" | "platform" | "function"
    )
}

/// `keystroke` in `fe-keymap`'s terms, if it names the key.
fn keystroke(keystroke: &gpui::Keystroke) -> Option<Keystroke> {
    let held = &keystroke.modifiers;
    let modifiers = Modifiers {
        cmd: held.platform,
        ctrl: held.control,
        alt: held.alt,
        shift: held.shift,
        function: held.function,
    };
    Some(Keystroke::new(modifiers, Key::parse(&keystroke.key)?))
}

/// GPUI's key contexts, outermost first, as a context stack: each one's
/// primary identifier names its frame, its other identifiers are flags, and
/// its `key=value` pairs carry over.
pub(crate) fn context_stack(contexts: &[KeyContext]) -> ContextStack {
    contexts
        .iter()
        .map(|context| {
            let name = context.primary().map_or("", |entry| entry.key.as_ref());
            context
                .secondary()
                .fold(ContextFrame::new(name), |frame, entry| match &entry.value {
                    None => frame.flag(entry.key.as_ref()),
                    Some(value) => frame.set(entry.key.as_ref(), value.as_ref()),
                })
        })
        .collect()
}
