// SPDX-License-Identifier: LicenseRef-Proprietary

//! Key input through kiln's own keymap. GPUI's keymap is unused: a
//! keystroke interceptor feeds every keystroke, with the focused element's
//! context, to one [`Matcher`], and dispatches what it matches to the
//! focused element, through [`dispatch_invocation`]: as its typed command's
//! action if one is registered, as a [`RunCommand`] otherwise.
//!
//! A keystroke that matches nothing propagates as usual, so text input
//! reaches the view. Keystrokes swallowed by a chord that then fails are
//! re-delivered as input (those without a character, such as `cmd-k`,
//! deliver nothing), and then the keystroke that failed it is matched
//! again alone, as a fresh one, before it propagates: `secondary-k left`
//! moves left. A key kiln has no name for, such as `f21`, fails a chord the
//! same way, then propagates unmatched.
//!
//! A chord belongs to the window and focused element it started in. It is
//! abandoned, neither run nor typed, once focus leaves that element, the
//! window closes, or a keystroke comes from anywhere else. Focus that
//! leaves and comes back before the next frame is drawn goes unseen; see
//! `HeldChord::start`.
//!
//! An app can take part through a [`KeyOwner`], such as a modal editing
//! layer that owns the focused editor's keys: it stamps the context each
//! keystroke is matched in, can hold every keystroke while its own command
//! is pending, can have matches to its own commands handed back rather
//! than dispatched, and can take the keystrokes a failed chord swallowed.
//! Without one, all of that is the plain behaviour above. An element can
//! also keep keystrokes from the keymap while it reads them itself: see
//! [`HOLDS_KEYS`], or [`HOLDS_ALL_KEYS`] to keep the platform modifier's
//! keystrokes too. What goes on from a chord held, for a which-key popup,
//! is [`pending_continuations`]; the owner hears of each change to it
//! ([`KeyOwner::chord_changed`]). [`hold_chord`] stops it timing out.
//!
//! [`RunCommand`]: crate::commands::RunCommand

use std::{rc::Rc, sync::Arc, time::Duration};

use gpui::{
    AnyWindowHandle, App, Global, KeyContext, KeystrokeEvent, SharedString, Subscription, Task,
    WeakFocusHandle, Window,
};
use kiln_command::{ContextFrame, ContextStack, Invocation};
use kiln_core::{Continuation, Key, KeyOutcome, Keymap, Keystroke, Matcher, Modifiers};

use crate::command_actions::dispatch_invocation;

/// How long a chord prefix waits for its next keystroke, after which
/// [`Matcher::flush`] ends the chord.
pub const CHORD_TIMEOUT: Duration = Duration::from_secs(1);

/// A key context identifier: while the focused element's own context
/// carries it, the keymap isn't consulted for keystrokes without the
/// platform modifier, which go on to the element's key listener. For a
/// mode that reads keys itself, such as a terminal's vi copy mode; the
/// platform modifier's bindings stay the app's.
pub const HOLDS_KEYS: &str = "holds_keys";

/// A key context identifier: while the focused element's own context
/// carries it, the keymap isn't consulted for any keystroke, the platform
/// modifier's included, and a pending chord is abandoned; every keystroke
/// goes on to the element's key listener. For a view that records keys,
/// where `cmd-w` must not close the tab. Beside [`HOLDS_KEYS`], which
/// leaves the platform modifier's bindings to the app.
pub const HOLDS_ALL_KEYS: &str = "holds_all_keys";

/// An app's say in key input, registered with [`set_key_owner`]. Each hook
/// is called at the point its doc names, in the order the module doc
/// describes, and its default is the plain behaviour, as with no owner.
///
/// For a modal editing layer such as vim, the owner is the focused
/// editor's live mode: while a command of its own is pending it holds
/// every keystroke, and a binding to its "hand the key over" command isn't
/// dispatched but fed to it.
pub trait KeyOwner: 'static {
    /// The context a keystroke is matched in: `stack`, the focused
    /// element's as last drawn, made current (say, with the live state of
    /// the focused element, which may have changed since that frame). Also
    /// whether the owner holds every keystroke there, in which case the
    /// matcher isn't consulted and the keystroke propagates, to the
    /// owner's own key listener.
    ///
    /// Called for each keystroke, and again for the one that fails a chord
    /// once the chord's keystrokes are replayed, since they may have
    /// changed the context.
    fn live_stack(
        &self,
        stack: ContextStack,
        window: &mut Window,
        cx: &mut App,
    ) -> (ContextStack, bool) {
        let _ = (window, cx);
        (stack, false)
    }

    /// Whether `invocation`, which a binding matched, is the owner's to
    /// handle rather than dispatched. If so the keystrokes its chord
    /// swallowed are replayed ([`KeyOwner::replay`]) and the last one
    /// propagates, so each reaches the owner once, in order.
    fn owns(&self, invocation: &Invocation) -> bool {
        let _ = invocation;
        false
    }

    /// Takes the keystrokes a failed or handed-over chord swallowed, all of
    /// them, with or without a character, and returns true; or returns
    /// false to have them re-delivered as input. Called with at least one
    /// keystroke. Take them without dispatching an event: GPUI's
    /// propagation flag is shared by every event, and is left as it was.
    fn replay(&self, keystrokes: &[gpui::Keystroke], window: &mut Window, cx: &mut App) -> bool {
        let _ = (keystrokes, window, cx);
        false
    }

    /// A modifier was pressed and released alone, leaving `modifiers`
    /// held. GPUI sends this wherever focus is, unlike its modifier change
    /// event, which reaches only the focused element's path: so an owner
    /// can, say, clear a hover state that a held modifier showed.
    fn modifiers_released(&self, modifiers: &gpui::Modifiers, window: &mut Window, cx: &mut App) {
        let _ = (modifiers, window, cx);
    }

    /// The chord held in `window` may have changed: it started, went on or
    /// ended. Called after each keystroke the keymap sees, its effects
    /// over but for the element's own key listeners, which come after;
    /// after a chord times out; and once one is abandoned as focus leaves
    /// its element. [`pending_continuations`] says what's held now.
    fn chord_changed(&self, window: &mut Window, cx: &mut App) {
        let _ = (window, cx);
    }
}

/// The plain behaviour, for when no owner is set.
struct NoOwner;

impl KeyOwner for NoOwner {}

/// The registered [`KeyOwner`] and context namespaces.
#[derive(Default)]
struct KeyHooks {
    owner: Option<Rc<dyn KeyOwner>>,
    namespaces: Vec<SharedString>,
}

impl Global for KeyHooks {}

/// Makes `owner` the app's [`KeyOwner`], replacing any before it. It can be
/// set before or after [`init`].
pub fn set_key_owner(cx: &mut App, owner: impl KeyOwner) {
    cx.default_global::<KeyHooks>().owner = Some(Rc::new(owner));
}

/// The owner to call, cloned out so it can be given the app; `None` for
/// the plain behaviour, [`NoOwner`].
fn owner(cx: &App) -> Option<Rc<dyn KeyOwner>> {
    cx.try_global::<KeyHooks>()
        .and_then(|hooks| hooks.owner.clone())
}

/// Has [`context_stack`] read GPUI keys starting `{namespace}_` as starting
/// `{namespace}.`: GPUI's key context identifiers can't hold `.`, so an
/// element sets `vim_mode` for a binding's `vim.mode`. Registering one
/// twice changes nothing.
pub fn register_context_namespace(cx: &mut App, namespace: impl Into<SharedString>) {
    let namespace = namespace.into();
    let hooks = cx.default_global::<KeyHooks>();
    if !hooks.namespaces.contains(&namespace) {
        hooks.namespaces.push(namespace);
    }
}

struct KeyInput {
    matcher: Matcher,
    /// The chord the matcher holds pending, if any.
    pending: Option<HeldChord>,
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
struct Target {
    window: AnyWindowHandle,
    focus: Option<WeakFocusHandle>,
}

impl Target {
    fn of(window: &Window, cx: &App) -> Self {
        Self {
            window: window.window_handle(),
            focus: window.focused(cx).map(|focus| focus.downgrade()),
        }
    }
}

/// The keys that go on from the chord held for the focused element
/// ([`pending_continuations`]).
#[derive(Clone, Debug, PartialEq)]
pub struct PendingChord {
    /// The chord's keystrokes so far.
    pub held: Vec<Keystroke>,
    /// What goes on from them ([`Keymap::continuations`]).
    pub next: Vec<Continuation>,
    /// The context its last keystroke was matched in, which the next is
    /// expected in.
    pub stack: ContextStack,
}

struct HeldChord {
    target: Target,
    /// The GPUI keystrokes of the chord, to re-deliver if it fails.
    swallowed: Vec<gpui::Keystroke>,
    /// The context its last keystroke was matched in.
    stack: ContextStack,
    /// Ends the chord; dropping it cancels that.
    _timeout: Task<()>,
    /// Abandons the chord when focus leaves its element.
    _focus_out: Option<Subscription>,
}

impl HeldChord {
    fn start(
        target: Target,
        swallowed: Vec<gpui::Keystroke>,
        stack: ContextStack,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let handle = target.window;
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
        // exception, and an app shouldn't move focus on one. What's left is
        // a mouse, timer or menu moving focus away and back within one
        // frame.
        //
        // The fix is a focus generation checked on continuation, timeout
        // and replay. Lift this limit once GPUI exposes a synchronous focus
        // signal, or once every focus move goes through a wrapper of ours.
        let focus_out = window.focused(cx).map(|focus| {
            // Dropped with the chord, so it fires only for this one.
            window.on_focus_out(&focus, cx, |_, window, cx| {
                cx.global_mut::<KeyInput>().abandon();
                chord_changed(window, cx);
            })
        });
        Self {
            target,
            swallowed,
            stack,
            _timeout: timeout,
            _focus_out: focus_out,
        }
    }
}

/// Installs the interceptor, matching against `keymap`. Called again, it
/// only swaps the keymap, as [`set_keymap`] does.
pub fn init(cx: &mut App, keymap: impl Into<Arc<Keymap>>) {
    if cx.has_global::<KeyInput>() {
        set_keymap(cx, keymap);
        return;
    }
    let interceptor = cx.intercept_keystrokes(on_keystroke);
    let window_closed = cx.on_window_closed(|cx, closed| {
        let input = cx.global_mut::<KeyInput>();
        if let Some(chord) = &input.pending
            && chord.target.window.window_id() == closed
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

/// Swaps the keymap, as on hot reload, dropping any chord in progress
/// ([`KeyOwner::chord_changed`]). Menus [`crate::menus::install`]ed are reinstalled, so their shortcuts
/// follow; without any, it touches neither GPUI's menus nor its keymap.
/// Windows redraw, so anything drawn from the keymap follows too. Call
/// after [`init`].
pub fn set_keymap(cx: &mut App, keymap: impl Into<Arc<Keymap>>) {
    let input = cx.global_mut::<KeyInput>();
    input.matcher.set_keymap(keymap);
    if let Some(chord) = input.pending.take() {
        // Unless the swap comes from inside that window's own update.
        (chord.target.window)
            .update(cx, |_, window, cx| chord_changed(window, cx))
            .ok();
    }
    crate::menus::refresh(cx);
    cx.refresh_windows();
}

/// The keymap keys are matched against: empty before [`init`].
pub fn current_keymap(cx: &App) -> Arc<Keymap> {
    cx.try_global::<KeyInput>()
        .map(|input| input.matcher.keymap().clone())
        .unwrap_or_default()
}

/// Whether a chord is pending, in the interceptor or the matcher.
pub fn chord_pending(cx: &App) -> bool {
    cx.try_global::<KeyInput>()
        .is_some_and(|input| input.pending.is_some() || !input.matcher.pending().is_empty())
}

/// The chord held for `window`'s focused element and what goes on from it,
/// in the context its last keystroke was matched in; `None` if none is,
/// or one is for another window or element.
pub fn pending_continuations(window: &Window, cx: &App) -> Option<PendingChord> {
    let input = cx.try_global::<KeyInput>()?;
    let chord = input.pending.as_ref()?;
    if chord.target != Target::of(window, cx) {
        return None;
    }
    let held = input.matcher.pending().to_vec();
    let next = input.matcher.keymap().continuations(&held, &chord.stack);
    Some(PendingChord {
        held,
        next,
        stack: chord.stack.clone(),
    })
}

/// Stops the chord held for `window`'s focused element from timing out,
/// as a shown which-key popup does: it then waits for the next key, a
/// focus change or a keymap reload. The next keystroke starts a chord
/// with a fresh timeout. Returns whether one was held.
pub fn hold_chord(window: &Window, cx: &mut App) -> bool {
    let target = Target::of(window, cx);
    if !cx.has_global::<KeyInput>() {
        return false;
    }
    let Some(chord) =
        (cx.global_mut::<KeyInput>().pending.as_mut()).filter(|chord| chord.target == target)
    else {
        return false;
    };
    chord._timeout = Task::ready(());
    true
}

/// Tells the owner the chord held may have changed
/// ([`KeyOwner::chord_changed`]).
fn chord_changed(window: &mut Window, cx: &mut App) {
    if let Some(owner) = owner(cx) {
        owner.chord_changed(window, cx);
    }
}

fn on_keystroke(event: &KeystrokeEvent, window: &mut Window, cx: &mut App) {
    if cx
        .try_global::<KeyInput>()
        .is_none_or(|input| input.redelivering)
    {
        return;
    }
    // A prompt drawn in the window has its keys (Enter, Escape) to itself:
    // GPUI puts it in the dispatch tree under whatever drew last, so the
    // keymap would see that element's context (an editor's, say) and run
    // its bindings.
    if window.has_active_prompt() {
        return;
    }
    match_keystroke(event, window, cx);
    chord_changed(window, cx);
}

/// Matches `event`'s keystroke, for [`on_keystroke`].
fn match_keystroke(event: &KeystrokeEvent, window: &mut Window, cx: &mut App) {
    crate::focus::watch(window, cx);
    let target = Target::of(window, cx);
    let input = cx.global_mut::<KeyInput>();
    // A chord started in another window or element doesn't continue here.
    if input
        .pending
        .as_ref()
        .is_some_and(|chord| chord.target != target)
    {
        input.abandon();
    }
    let owner = owner(cx);
    let owner = owner.as_deref().unwrap_or(&NoOwner);
    let gpui_stack = context_stack(&event.context_stack, cx);
    let (mut stack, held) = owner.live_stack(gpui_stack.clone(), window, cx);
    if held {
        // The owner's key listener has it.
        return;
    }
    if holds_keys(event) {
        // The focused element's key listener has it.
        cx.global_mut::<KeyInput>().abandon();
        return;
    }
    let input = cx.global_mut::<KeyInput>();
    let Some(keystroke) = keystroke(&event.keystroke) else {
        // A standalone modifier goes on as GPUI sends it, leaving the chord
        // be. Any other key kiln has no name for fails the chord, as one
        // that matches nothing would, then goes on unmatched.
        if !is_modifier(&event.keystroke) {
            let failed_chord = input.pending.take().map(|chord| chord.swallowed);
            input.matcher.flush();
            replay(owner, failed_chord.unwrap_or_default(), window, cx);
        } else {
            owner.modifiers_released(&window.modifiers(), window, cx);
        }
        return;
    };
    // Taking the chord cancels its timer.
    let mut swallowed = input
        .pending
        .take()
        .map_or_else(Vec::new, |chord| chord.swallowed);
    let mut outcome = input.matcher.push(keystroke, &stack);
    if let KeyOutcome::Unmatched(keys) = &outcome
        && keys.len() > 1
    {
        // This keystroke ended a chord that matched nothing. The chord's
        // keystrokes go first, then this one is matched again, as a fresh
        // one, in the context they leave: they may have started one of the
        // owner's commands, which then holds it.
        replay(owner, std::mem::take(&mut swallowed), window, cx);
        let held;
        (stack, held) = owner.live_stack(gpui_stack, window, cx);
        if held {
            return;
        }
        outcome = cx.global_mut::<KeyInput>().matcher.push(keystroke, &stack);
    }
    match outcome {
        // The owner's: the chord's keystrokes go to it, and this one on to
        // its key listener, so each is fed once, in order.
        KeyOutcome::Dispatch(invocation) if owner.owns(&invocation) => {
            replay(owner, swallowed, window, cx);
        }
        KeyOutcome::Dispatch(invocation) => {
            dispatch(invocation, window, cx);
            cx.stop_propagation();
        }
        // A prefix with a binding of its own runs at once, and the chord
        // carries on: the prefix is spent, so a failed chord or a timeout
        // never replays it. The owner's takes the key, which goes on to
        // its key listener.
        KeyOutcome::DispatchPending(invocation) => {
            let owned = owner.owns(&invocation);
            if owned {
                replay(owner, swallowed, window, cx);
            } else {
                dispatch(invocation, window, cx);
                cx.stop_propagation();
            }
            // The prefix's action may have moved focus (`escape` closing
            // Find): the chord carries on where focus is now, and ends if
            // it leaves that.
            let target = Target::of(window, cx);
            let chord = HeldChord::start(target, Vec::new(), stack, window, cx);
            cx.global_mut::<KeyInput>().pending = Some(chord);
        }
        KeyOutcome::Pending => {
            swallowed.push(event.keystroke.clone());
            let chord = HeldChord::start(target, swallowed, stack, window, cx);
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
            end_chord(window, cx);
            chord_changed(window, cx);
        })
        .ok();
}

/// Ends the pending chord as its timeout does, for [`on_timeout`].
fn end_chord(window: &mut Window, cx: &mut App) {
    let target = Target::of(window, cx);
    let input = cx.global_mut::<KeyInput>();
    let Some(chord) = input.pending.take() else {
        return;
    };
    let outcome = input.matcher.flush();
    if chord.target != target {
        return;
    }
    let owner = owner(cx);
    let owner = owner.as_deref().unwrap_or(&NoOwner);
    match outcome {
        // The owner's: the keys go to it.
        KeyOutcome::Dispatch(invocation) if owner.owns(&invocation) => {
            replay(owner, chord.swallowed, window, cx)
        }
        KeyOutcome::Dispatch(invocation) => dispatch(invocation, window, cx),
        KeyOutcome::Unmatched(_) => replay(owner, chord.swallowed, window, cx),
        KeyOutcome::Pending | KeyOutcome::DispatchPending(_) => {
            unreachable!("flush ends the chord")
        }
    }
}

/// Runs `invocation` on the focused element, synchronously, as GPUI's own
/// keymap does; without one, or if it wasn't drawn, on the window, once
/// the current dispatch is over (see [`dispatch_invocation`]). Menu items
/// run through it too.
pub(crate) fn dispatch(invocation: Invocation, window: &mut Window, cx: &mut App) {
    if let Err(error) = dispatch_invocation(invocation, window, cx) {
        log::warn!("{error}");
    }
}

/// Re-delivers swallowed keystrokes, past the matcher: to the owner, if it
/// takes them, or else dispatched as input, those with a character; those
/// without deliver nothing.
///
/// Either way GPUI's propagation flag is left on, as it is when the
/// interceptor runs: it's shared by every event, so a re-delivered
/// keystroke's handler stopping it would otherwise swallow the keystroke
/// being handled.
fn replay(
    owner: &dyn KeyOwner,
    keystrokes: Vec<gpui::Keystroke>,
    window: &mut Window,
    cx: &mut App,
) {
    if keystrokes.is_empty() || owner.replay(&keystrokes, window, cx) {
        return;
    }
    cx.global_mut::<KeyInput>().redelivering = true;
    for keystroke in keystrokes {
        if keystroke.key_char.is_some() {
            window.dispatch_keystroke(keystroke, cx);
        }
    }
    cx.global_mut::<KeyInput>().redelivering = false;
    cx.propagate();
}

/// Whether the focused element holds `event`'s keystroke ([`HOLDS_KEYS`],
/// [`HOLDS_ALL_KEYS`]).
fn holds_keys(event: &KeystrokeEvent) -> bool {
    let own = event.context_stack.last();
    own.is_some_and(|context| context.contains(HOLDS_ALL_KEYS))
        || (!event.keystroke.modifiers.platform
            && own.is_some_and(|context| context.contains(HOLDS_KEYS)))
}

/// Whether `keystroke` is a modifier pressed and released alone, which GPUI
/// sends named after it.
fn is_modifier(keystroke: &gpui::Keystroke) -> bool {
    matches!(
        keystroke.key.as_str(),
        "shift" | "control" | "alt" | "platform" | "function"
    )
}

/// `keystroke` in kiln's terms, if it names the key.
pub fn keystroke(keystroke: &gpui::Keystroke) -> Option<Keystroke> {
    let modifiers = modifiers(&keystroke.modifiers);
    Some(Keystroke::new(modifiers, Key::parse(&keystroke.key)?))
}

/// GPUI's modifiers in kiln's terms.
pub fn modifiers(held: &gpui::Modifiers) -> Modifiers {
    Modifiers {
        cmd: held.platform,
        ctrl: held.control,
        alt: held.alt,
        shift: held.shift,
        function: held.function,
    }
}

/// What `keystroke` alone runs in `stack` with the keymap keys are matched
/// against ([`Keymap::lookup`]), leaving any pending chord be: for a click
/// bound like a key, such as `cmd-click`.
pub fn lookup(keystroke: &Keystroke, stack: &ContextStack, cx: &App) -> Option<Invocation> {
    current_keymap(cx).lookup(keystroke, stack)
}

/// GPUI's key contexts, outermost first, as a context stack, with the
/// namespaces [`register_context_namespace`] registered: see
/// [`context_stack_with`].
pub fn context_stack(contexts: &[KeyContext], cx: &App) -> ContextStack {
    let namespaces = cx
        .try_global::<KeyHooks>()
        .map_or(&[][..], |hooks| &hooks.namespaces[..]);
    context_stack_with(contexts, namespaces)
}

/// GPUI's key contexts, outermost first, as a context stack: each one's
/// primary identifier names its frame, its other identifiers are flags, and
/// its `key=value` pairs carry over. A key starting with one of
/// `namespaces` and `_` starts with it and `.` instead (with `vim`,
/// `vim_mode` is `vim.mode`): GPUI's identifiers can't hold `.`.
pub fn context_stack_with(contexts: &[KeyContext], namespaces: &[impl AsRef<str>]) -> ContextStack {
    let key = |key: &str| {
        for namespace in namespaces {
            let namespace = namespace.as_ref();
            if let Some(rest) = key
                .strip_prefix(namespace)
                .and_then(|rest| rest.strip_prefix('_'))
            {
                return format!("{namespace}.{rest}");
            }
        }
        key.to_owned()
    };
    contexts
        .iter()
        .map(|context| {
            let name = context.primary().map_or("", |entry| entry.key.as_ref());
            context
                .secondary()
                .fold(ContextFrame::new(name), |frame, entry| match &entry.value {
                    None => frame.flag(key(&entry.key)),
                    Some(value) => frame.set(key(&entry.key), value.as_ref()),
                })
        })
        .collect()
}
