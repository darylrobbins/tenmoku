//! Key input through `fe-keymap`. GPUI's own keymap is unused: a keystroke
//! interceptor feeds every keystroke, with the focused element's context, to
//! one [`Matcher`], and dispatches what it matches as a
//! [`RunCommand`] to the focused element.
//!
//! A keystroke that matches nothing propagates as usual, so text input
//! reaches the view. Keystrokes swallowed by a chord that then fails are
//! re-delivered as input (those without a character, such as `cmd-k`,
//! deliver nothing), and the keystroke that failed it is matched again
//! alone, as a fresh one, before it propagates: `cmd-k left` moves left.

use std::{sync::Arc, time::Duration};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_keymap::{Key, KeyOutcome, Keymap, KeymapIssue, Keystroke, Matcher, Modifiers};
use gpui::{App, Global, KeyContext, KeystrokeEvent, Subscription, Task, Window};

use crate::commands::RunCommand;

/// How long a chord prefix waits for its next keystroke, after which
/// [`Matcher::flush`] ends the chord.
pub const CHORD_TIMEOUT: Duration = Duration::from_secs(1);

struct KeyInput {
    matcher: Matcher,
    /// The GPUI keystrokes of the chord the matcher holds pending, to
    /// re-deliver if it fails.
    swallowed: Vec<gpui::Keystroke>,
    /// Ends the pending chord; dropping it cancels that.
    timeout: Option<Task<()>>,
    /// Set while swallowed keystrokes are re-delivered, which the matcher
    /// has seen already.
    redelivering: bool,
    _interceptor: Subscription,
}

impl Global for KeyInput {}

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
    cx.set_global(KeyInput {
        matcher: Matcher::new(keymap),
        swallowed: Vec::new(),
        timeout: None,
        redelivering: false,
        _interceptor: interceptor,
    });
}

/// Swaps the keymap, as on hot reload, dropping any chord in progress.
/// `keymap` is the defaults with the user's layer over them, as
/// [`fe_keymap::KeymapStore`] builds it: each layer's last good version.
pub fn set_user_keymap(cx: &mut App, keymap: impl Into<Arc<Keymap>>) {
    let input = cx.global_mut::<KeyInput>();
    input.matcher.set_keymap(keymap);
    input.swallowed.clear();
    input.timeout = None;
}

/// The keymap's issues ([`Keymap::report_with`]) in the one context there
/// is so far, `[editor]`.
pub fn keymap_report(cx: &App) -> Vec<KeymapIssue> {
    let keymap = cx.global::<KeyInput>().matcher.keymap();
    keymap.report_with(&[fe_editor::commands::context()])
}

fn on_keystroke(event: &KeystrokeEvent, window: &mut Window, cx: &mut App) {
    // Standalone modifiers and keys `fe-keymap` has no name for aren't
    // bound: they go on as GPUI sends them.
    let Some(keystroke) = keystroke(&event.keystroke) else {
        return;
    };
    if !cx.has_global::<KeyInput>() {
        return;
    }
    let input = cx.global_mut::<KeyInput>();
    if input.redelivering {
        return;
    }
    input.timeout = None;
    let stack = context_stack(&event.context_stack);
    let mut outcome = input.matcher.push(keystroke, &stack);
    input.swallowed.push(event.keystroke.clone());
    let mut failed_chord = Vec::new();
    if let KeyOutcome::Unmatched(keys) = &outcome
        && keys.len() > 1
    {
        // This keystroke ended a chord that matched nothing: match it again
        // as a fresh one.
        failed_chord = std::mem::take(&mut input.swallowed);
        failed_chord.pop();
        outcome = input.matcher.push(keystroke, &stack);
        input.swallowed.push(event.keystroke.clone());
    }
    if !matches!(outcome, KeyOutcome::Pending) {
        input.swallowed.clear();
    }
    redeliver(failed_chord, window, cx);
    match outcome {
        KeyOutcome::Dispatch(invocation) => {
            dispatch(invocation, window, cx);
            cx.stop_propagation();
        }
        KeyOutcome::Pending => {
            let timeout = window.spawn(cx, async |cx| {
                cx.background_executor().timer(CHORD_TIMEOUT).await;
                cx.update(on_timeout).ok();
            });
            cx.global_mut::<KeyInput>().timeout = Some(timeout);
            cx.stop_propagation();
        }
        // Only this keystroke, which goes on to the view.
        KeyOutcome::Unmatched(_) => {}
    }
}

/// Ends the pending chord when no keystroke came in time.
fn on_timeout(window: &mut Window, cx: &mut App) {
    let input = cx.global_mut::<KeyInput>();
    let swallowed = std::mem::take(&mut input.swallowed);
    match input.matcher.flush() {
        KeyOutcome::Dispatch(invocation) => dispatch(invocation, window, cx),
        KeyOutcome::Unmatched(_) => redeliver(swallowed, window, cx),
        KeyOutcome::Pending => unreachable!("flush ends the chord"),
    }
}

/// Runs `invocation` on the focused element, synchronously, as GPUI's own
/// keymap did; without one, on the window.
fn dispatch(invocation: Invocation, window: &mut Window, cx: &mut App) {
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
