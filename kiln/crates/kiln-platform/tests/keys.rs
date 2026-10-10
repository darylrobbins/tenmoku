// SPDX-License-Identifier: LicenseRef-Proprietary

//! Key input through kiln's keymap: bindings and chords dispatched to the
//! focused element, failed chords replayed, the chord timer on the test
//! executor's simulated clock, and an app's `KeyOwner` taking part.

use std::{cell::RefCell, path::Path, rc::Rc, time::Duration};

use gpui::{
    AnyWindowHandle, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, KeyContext, ParentElement as _, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div,
};
use kiln_command::{ContextFrame, ContextStack, Invocation, Source};
use kiln_core::{Keymap, KeymapLayer, Keystroke, Os};
use kiln_platform::{
    commands::{self, RunCommand},
    keys::{
        self, CHORD_TIMEOUT, KeyOwner, chord_pending, context_stack, context_stack_with,
        register_context_namespace, set_key_owner,
    },
};

mod pad {
    kiln_platform::commands!(
        pad,
        [
            /// Clear
            Clear,
        ]
    );
}

/// The bindings most tests use. `secondary-k` alone selects all, after the
/// chord timeout: a chord prefix that runs if the chord is ended rather
/// than abandoned. `g g` goes to the top: a prefix that is typed if the
/// chord fails.
const KEYS: &str = r#"
[[bindings]]
keys = "left"
command = "pad.move_left"
context = "pad"

[[bindings]]
keys = "secondary-k"
command = "pad.select_all"
context = "pad"

[[bindings]]
keys = "secondary-k secondary-0"
command = "pad.fold_all"
context = "pad"

[[bindings]]
keys = "g g"
command = "pad.top"
context = "pad"
"#;

fn keymap(source: &str) -> Keymap {
    let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), source, None);
    assert_eq!(diagnostics, []);
    Keymap::from_layers([layer.unwrap()])
}

/// `keys` with this platform's `secondary`, in GPUI's spelling.
fn sec(keys: &str) -> String {
    let secondary = if Os::current() == Os::MacOs {
        "cmd"
    } else {
        "ctrl"
    };
    keys.replace("secondary", secondary)
}

/// What a pad saw: the invocations it ran, and the text typed into it.
#[derive(Default)]
struct Seen {
    ran: Vec<Invocation>,
    typed: String,
}

type Log = Rc<RefCell<Seen>>;

/// The commands run since last asked, by ID.
fn ran(log: &Log) -> Vec<String> {
    std::mem::take(&mut log.borrow_mut().ran)
        .iter()
        .map(|invocation| invocation.command.as_str().to_owned())
        .collect()
}

fn typed(log: &Log) -> String {
    log.borrow().typed.clone()
}

/// A focusable element in context `pad` (and whatever `context` adds) that
/// runs every command it gets, typed or not, and takes text from key
/// presses.
struct Pad {
    focus: FocusHandle,
    context: &'static str,
    log: Log,
}

impl Pad {
    fn new(context: &'static str, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            context,
            log: Log::default(),
        }
    }
}

impl Render for Pad {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let context = KeyContext::parse(self.context).unwrap();
        div()
            .track_focus(&self.focus)
            .key_context(context)
            .size_full()
            .on_action(cx.listener(|this, run: &RunCommand, _, _| {
                this.log.borrow_mut().ran.push(run.0.clone());
            }))
            .on_action(cx.listener(|this, _: &pad::Clear, _, cx| {
                let invocation = kiln_platform::command_actions::current_invocation(cx);
                this.log.borrow_mut().ran.push(invocation.unwrap());
            }))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, _| {
                if let Some(text) = &event.keystroke.key_char {
                    this.log.borrow_mut().typed.push_str(text);
                }
            }))
    }
}

fn init(cx: &mut TestAppContext, source: &str) {
    let keymap = keymap(source);
    cx.update(|cx| {
        commands::init(cx, "Pad");
        pad::register(cx);
        keys::init(cx, keymap);
    });
}

/// A focused pad, matching keys against `source`, in a headless window.
fn pad<'a>(cx: &'a mut TestAppContext, source: &str) -> (Log, &'a mut VisualTestContext) {
    init(cx, source);
    let (view, cx) = cx.add_window_view(|_, cx| Pad::new("pad", cx));
    let log = focus_pad(&view, cx);
    (log, cx)
}

/// Focuses `view`, returning its log.
fn focus_pad(view: &Entity<Pad>, cx: &mut VisualTestContext) -> Log {
    let (focus, log) = view.update(cx, |pad, _| (pad.focus.clone(), pad.log.clone()));
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.run_until_parked();
    log
}

/// Waits out the chord timeout, twice over.
fn time_out(cx: &mut TestAppContext) {
    cx.executor().advance_clock(CHORD_TIMEOUT * 2);
    cx.run_until_parked();
}

#[gpui::test]
fn a_binding_dispatches_to_the_focused_element(cx: &mut TestAppContext) {
    let (log, cx) = pad(cx, KEYS);
    cx.simulate_keystrokes("left");
    let ran = std::mem::take(&mut log.borrow_mut().ran);
    let left = Invocation::of("pad.move_left")
        .unwrap()
        .source(Source::Keyboard)
        .context(ContextStack::new().with(ContextFrame::new("pad")));
    assert_eq!(ran, [left]);
    assert_eq!(typed(&log), "", "a key that runs a command isn't typed");

    // A key bound to nothing goes on to the view.
    cx.simulate_keystrokes("x");
    assert_eq!(ran_and_typed(&log), (vec![], "x".into()));
}

fn ran_and_typed(log: &Log) -> (Vec<String>, String) {
    let ran = ran(log);
    let typed = std::mem::take(&mut log.borrow_mut().typed);
    (ran, typed)
}

#[gpui::test]
fn a_chord_waits_then_completes(cx: &mut TestAppContext) {
    let (log, cx) = pad(cx, KEYS);
    cx.simulate_keystrokes(&sec("secondary-k"));
    assert!(cx.update(|_, cx| chord_pending(cx)));
    assert_eq!(ran(&log), Vec::<String>::new());
    cx.simulate_keystrokes(&sec("secondary-0"));
    assert!(!cx.update(|_, cx| chord_pending(cx)));
    assert_eq!(ran(&log), ["pad.fold_all"]);
    time_out(cx);
    assert_eq!(ran_and_typed(&log), (vec![], String::new()));
}

#[gpui::test]
fn a_failed_chord_types_its_keys_and_matches_the_last_afresh(cx: &mut TestAppContext) {
    let (log, cx) = pad(cx, KEYS);
    // Ended by a key it doesn't bind, both are typed, in order.
    cx.simulate_keystrokes("g x");
    assert_eq!(ran_and_typed(&log), (vec![], "gx".into()));
    // A key that ends one is matched afresh: `secondary-k left` moves left,
    // and `secondary-k`, which has no character, types nothing.
    cx.simulate_keystrokes(&sec("secondary-k left"));
    assert_eq!(
        ran_and_typed(&log),
        (vec!["pad.move_left".into()], "".into())
    );
    cx.simulate_keystrokes("g left");
    assert_eq!(
        ran_and_typed(&log),
        (vec!["pad.move_left".into()], "g".into())
    );
    cx.simulate_keystrokes("g g");
    assert_eq!(ran_and_typed(&log), (vec!["pad.top".into()], "".into()));
    time_out(cx);
    assert_eq!(ran_and_typed(&log), (vec![], String::new()));
}

#[gpui::test]
fn the_timeout_runs_a_prefix_or_types_it(cx: &mut TestAppContext) {
    let (log, cx) = pad(cx, KEYS);
    // Alone, secondary-k waits for the timeout, then runs.
    cx.simulate_keystrokes(&sec("secondary-k"));
    cx.executor()
        .advance_clock(CHORD_TIMEOUT - Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(ran(&log), Vec::<String>::new());
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(ran(&log), ["pad.select_all"]);

    // A prefix bound to nothing alone is typed.
    cx.simulate_keystrokes("g");
    assert_eq!(typed(&log), "");
    time_out(cx);
    assert_eq!(ran_and_typed(&log), (vec![], "g".into()));

    // A chord ended in time cancels the timer.
    cx.simulate_keystrokes(&sec("secondary-k x"));
    time_out(cx);
    assert_eq!(ran_and_typed(&log), (vec![], "x".into()));
}

/// A focused pad in a window of its own.
fn pad_window(cx: &mut TestAppContext) -> (AnyWindowHandle, Log) {
    let window = cx.add_window(|_, cx| Pad::new("pad", cx));
    let log = window
        .update(cx, |pad, window, cx| {
            window.focus(&pad.focus, cx);
            pad.log.clone()
        })
        .unwrap();
    cx.run_until_parked();
    (window.into(), log)
}

#[gpui::test]
fn a_chord_belongs_to_its_window(cx: &mut TestAppContext) {
    init(cx, KEYS);
    let (a, a_log) = pad_window(cx);
    let (b, b_log) = pad_window(cx);

    // secondary-0 in b doesn't complete a's secondary-k, and a's never runs.
    cx.simulate_keystrokes(a, &sec("secondary-k"));
    cx.simulate_keystrokes(b, &sec("secondary-0"));
    time_out(cx);
    assert_eq!(ran(&a_log), Vec::<String>::new());
    assert_eq!(ran(&b_log), Vec::<String>::new());

    // A prefix in b isn't typed into a.
    cx.simulate_keystrokes(b, "g");
    cx.simulate_keystrokes(a, "g");
    time_out(cx);
    assert_eq!(typed(&a_log), "g");
    assert_eq!(typed(&b_log), "");

    // Closing a abandons its chord: secondary-0 in b doesn't complete it.
    cx.simulate_keystrokes(a, &sec("secondary-k"));
    assert!(cx.update(|cx| chord_pending(cx)));
    cx.update_window(a, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert!(!cx.update(|cx| chord_pending(cx)));
    cx.simulate_keystrokes(b, &sec("secondary-0"));
    time_out(cx);
    assert_eq!(ran(&b_log), Vec::<String>::new());

    // b's own chord still runs.
    cx.simulate_keystrokes(b, &sec("secondary-k secondary-0"));
    assert_eq!(ran(&b_log), ["pad.fold_all"]);
}

/// Two pads side by side, in one window.
struct Pair {
    left: Entity<Pad>,
    right: Entity<Pad>,
}

impl Render for Pair {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .child(self.left.clone())
            .child(self.right.clone())
    }
}

#[gpui::test]
fn focus_leaving_abandons_a_chord(cx: &mut TestAppContext) {
    init(cx, KEYS);
    let (pair, cx) = cx.add_window_view(|_, cx| Pair {
        left: cx.new(|cx| Pad::new("pad", cx)),
        right: cx.new(|cx| Pad::new("pad", cx)),
    });
    let (left, right) = pair.update(cx, |pair, _| (pair.left.clone(), pair.right.clone()));
    let left_log = left.update(cx, |pad, _| pad.log.clone());
    let right_log = right.update(cx, |pad, _| pad.log.clone());
    // Then draws, as the next frame would, which reports the focus change.
    let focus = |pad: &Entity<Pad>, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let handle = pad.read(cx).focus.clone();
            window.focus(&handle, cx);
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    };
    // Focus events are only reported in the active window.
    cx.update(|window, _| window.activate_window());
    focus(&left, cx);

    // Started in left, secondary-k is neither completed in right nor run
    // there on timeout.
    cx.simulate_keystrokes(&sec("secondary-k"));
    focus(&right, cx);
    cx.simulate_keystrokes(&sec("secondary-0"));
    time_out(cx);
    focus(&left, cx);
    cx.simulate_keystrokes(&sec("secondary-k"));
    focus(&right, cx);
    time_out(cx);
    assert_eq!(ran(&left_log), Vec::<String>::new());
    assert_eq!(ran(&right_log), Vec::<String>::new());

    // A prefix isn't typed into right.
    focus(&left, cx);
    cx.simulate_keystrokes("g");
    focus(&right, cx);
    time_out(cx);
    assert_eq!(typed(&left_log), "");
    assert_eq!(typed(&right_log), "");

    // Focus that leaves and comes back abandons the chord too.
    focus(&left, cx);
    cx.simulate_keystrokes(&sec("secondary-k"));
    assert!(cx.update(|_, cx| chord_pending(cx)));
    focus(&right, cx);
    assert!(!cx.update(|_, cx| chord_pending(cx)));
    focus(&left, cx);
    cx.simulate_keystrokes(&sec("secondary-0"));
    time_out(cx);
    assert_eq!(ran(&left_log), Vec::<String>::new());

    // Kept in one element, it runs.
    cx.simulate_keystrokes(&sec("secondary-k secondary-0"));
    assert_eq!(ran(&left_log), ["pad.fold_all"]);
}

#[gpui::test]
fn unnamed_keys_fail_a_chord_and_modifiers_leave_it(cx: &mut TestAppContext) {
    let (log, cx) = pad(cx, KEYS);
    // f21, which kiln has no name for, ends secondary-k: secondary-0 after
    // it is a fresh keystroke, and secondary-k doesn't run on timeout.
    cx.simulate_keystrokes(&sec("secondary-k f21 secondary-0"));
    cx.simulate_keystrokes(&sec("secondary-k f21"));
    time_out(cx);
    assert_eq!(ran(&log), Vec::<String>::new());

    // A prefix it ends is typed at once.
    cx.simulate_keystrokes("g f21");
    assert_eq!(typed(&log), "g");
    time_out(cx);
    assert_eq!(typed(&log), "g");

    // A modifier pressed alone leaves the chord be.
    cx.simulate_keystrokes(&sec("secondary-k"));
    cx.simulate_modifiers_change(gpui::Modifiers::shift());
    cx.simulate_modifiers_change(gpui::Modifiers::none());
    cx.simulate_keystrokes(&sec("secondary-0"));
    assert_eq!(ran(&log), ["pad.fold_all"]);
}

#[gpui::test]
fn a_typed_command_bound_to_a_key_dispatches_as_its_action(cx: &mut TestAppContext) {
    let (log, cx) = pad(
        cx,
        r#"
        [[bindings]]
        keys = "ctrl-l"
        command = "pad.clear"
        "#,
    );
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        commands::observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        })
    });
    cx.simulate_keystrokes("ctrl-l");
    // The `pad::Clear` handler ran, synchronously, under the invocation.
    let ran = std::mem::take(&mut log.borrow_mut().ran);
    let clear = Invocation::of("pad.clear")
        .unwrap()
        .source(Source::Keyboard)
        .context(ContextStack::new().with(ContextFrame::new("pad")));
    assert_eq!(ran, std::slice::from_ref(&clear));
    assert_eq!(*seen.borrow(), [clear]);
}

#[gpui::test]
fn set_keymap_swaps_the_keymap_and_drops_a_chord(cx: &mut TestAppContext) {
    let (log, cx) = pad(cx, KEYS);
    cx.simulate_keystrokes(&sec("secondary-k"));
    let swapped = keymap(
        r#"
        [[bindings]]
        keys = "left"
        command = "pad.move_right"
        "#,
    );
    cx.update(|_, cx| keys::set_keymap(cx, swapped));
    assert!(!cx.update(|_, cx| chord_pending(cx)));
    time_out(cx);
    assert_eq!(ran(&log), Vec::<String>::new());
    cx.simulate_keystrokes("left");
    assert_eq!(ran(&log), ["pad.move_right"]);

    let left = Keystroke::parse("left").unwrap();
    let found = cx.update(|_, cx| keys::lookup(&left, &ContextStack::new(), cx));
    assert_eq!(found.unwrap().command.as_str(), "pad.move_right");
    let bindings = cx.update(|_, cx| keys::current_keymap(cx).bindings().len());
    assert_eq!(bindings, 1);
}

/// What a [`FakeOwner`] is told to do, and what it was fed.
#[derive(Default)]
struct OwnerLog {
    /// Holds every keystroke, as a modal layer with a command pending.
    holds: bool,
    /// The command it handles itself.
    owns: Option<&'static str>,
    /// Whether it takes replayed keystrokes.
    takes_replay: bool,
    /// The keystrokes replayed to it.
    fed: Vec<String>,
    /// How many times it stamped a stack.
    stamped: usize,
    /// How many modifier releases it heard of.
    released: usize,
}

/// A key owner that stamps an `owner.live` flag on the innermost frame and
/// records what it's given.
struct FakeOwner(Rc<RefCell<OwnerLog>>);

impl KeyOwner for FakeOwner {
    fn live_stack(
        &self,
        stack: ContextStack,
        _: &mut Window,
        _: &mut gpui::App,
    ) -> (ContextStack, bool) {
        let mut log = self.0.borrow_mut();
        log.stamped += 1;
        let mut frames = stack.frames().to_vec();
        if let Some(last) = frames.pop() {
            frames.push(last.flag("owner.live"));
        }
        (frames.into_iter().collect(), log.holds)
    }

    fn owns(&self, invocation: &Invocation) -> bool {
        self.0.borrow().owns == Some(invocation.command.as_str())
    }

    fn replay(&self, keystrokes: &[gpui::Keystroke], _: &mut Window, _: &mut gpui::App) -> bool {
        let mut log = self.0.borrow_mut();
        if log.takes_replay {
            log.fed
                .extend(keystrokes.iter().map(|keystroke| keystroke.unparse()));
        }
        log.takes_replay
    }

    fn modifiers_released(&self, _: &gpui::Modifiers, _: &mut Window, _: &mut gpui::App) {
        self.0.borrow_mut().released += 1;
    }
}

/// `g g` as in [`KEYS`], `d d` for the owner's command, and `s` bound only
/// where the owner's stamp is.
const OWNED_KEYS: &str = r#"
[[bindings]]
keys = "g g"
command = "pad.top"
context = "pad"

[[bindings]]
keys = "d d"
command = "owner.key"
context = "pad"

[[bindings]]
keys = "s"
command = "pad.stamped"
context = "pad && owner.live"
"#;

/// A focused pad with a [`FakeOwner`].
fn owned_pad(cx: &mut TestAppContext) -> (Log, Rc<RefCell<OwnerLog>>, &mut VisualTestContext) {
    let owner = Rc::new(RefCell::new(OwnerLog::default()));
    cx.update(|cx| set_key_owner(cx, FakeOwner(owner.clone())));
    let (log, cx) = pad(cx, OWNED_KEYS);
    (log, owner, cx)
}

#[gpui::test]
fn the_owner_stamps_the_context(cx: &mut TestAppContext) {
    let (log, owner, cx) = owned_pad(cx);
    cx.simulate_keystrokes("s");
    let ran = std::mem::take(&mut log.borrow_mut().ran);
    assert_eq!(ran.len(), 1);
    let stamped = ContextStack::new().with(ContextFrame::new("pad").flag("owner.live"));
    assert_eq!(ran[0].context, stamped);
    assert_eq!(owner.borrow().stamped, 1);

    // The keystroke that fails a chord is matched in a fresh stamp, after
    // the chord's keystrokes are replayed.
    cx.simulate_keystrokes("g s");
    assert_eq!(
        ran_and_typed(&log),
        (vec!["pad.stamped".into()], "g".into())
    );
    assert_eq!(owner.borrow().stamped, 4);

    // A modifier released alone is the owner's to hear of.
    cx.simulate_modifiers_change(gpui::Modifiers::shift());
    cx.simulate_modifiers_change(gpui::Modifiers::none());
    assert_eq!(owner.borrow().released, 1);
}

#[gpui::test]
fn an_owner_holding_every_keystroke_bypasses_the_matcher(cx: &mut TestAppContext) {
    let (log, owner, cx) = owned_pad(cx);
    owner.borrow_mut().holds = true;
    cx.simulate_keystrokes("s g g");
    assert_eq!(ran_and_typed(&log), (vec![], "sgg".into()));
    assert!(!cx.update(|_, cx| chord_pending(cx)));

    owner.borrow_mut().holds = false;
    cx.simulate_keystrokes("g g");
    assert_eq!(ran_and_typed(&log), (vec!["pad.top".into()], "".into()));
}

#[gpui::test]
fn the_owner_gets_its_command_and_the_replay(cx: &mut TestAppContext) {
    let (log, owner, cx) = owned_pad(cx);
    owner.borrow_mut().owns = Some("owner.key");
    owner.borrow_mut().takes_replay = true;

    // Its command isn't dispatched: the chord's first keystroke is fed to
    // it, and the last goes on to the element's key listener.
    cx.simulate_keystrokes("d d");
    assert_eq!(owner.borrow().fed, ["d"]);
    assert_eq!(ran_and_typed(&log), (vec![], "d".into()));

    // A failed chord's keystrokes go to it too, every one, rather than
    // being typed; the keystroke that failed it is matched afresh.
    owner.borrow_mut().fed.clear();
    let keys = sec("g secondary-k");
    cx.simulate_keystrokes(&keys);
    let gpui_keys: Vec<_> = keys
        .split(' ')
        .map(|keys| gpui::Keystroke::parse(keys).unwrap().unparse())
        .collect();
    assert_eq!(owner.borrow().fed, gpui_keys[..1]);
    assert_eq!(typed(&log), "");

    // And so do a chord's keystrokes on timeout.
    owner.borrow_mut().fed.clear();
    cx.simulate_keystrokes("d");
    time_out(cx);
    assert_eq!(owner.borrow().fed, ["d"]);

    // Without taking the replay, it's typed as usual.
    owner.borrow_mut().fed.clear();
    owner.borrow_mut().takes_replay = false;
    cx.simulate_keystrokes("g x");
    assert_eq!(ran_and_typed(&log), (vec![], "gx".into()));
    assert_eq!(owner.borrow().fed, Vec::<String>::new());
}

#[gpui::test]
fn namespaces_read_gpui_keys_as_dotted(cx: &mut TestAppContext) {
    let contexts = [
        KeyContext::parse("pad vim_mode=normal vim_pending").unwrap(),
        KeyContext::parse("pad vimish_x=1").unwrap(),
    ];
    let mapped = context_stack_with(&contexts, &["vim"]);
    let expected = ContextStack::new()
        .with(
            ContextFrame::new("pad")
                .set("vim.mode", "normal")
                .flag("vim.pending"),
        )
        .with(ContextFrame::new("pad").set("vimish_x", "1"));
    assert_eq!(mapped, expected);
    let none: &[&str] = &[];
    let plain = context_stack_with(&contexts, none);
    assert_eq!(plain.frames()[0].values.get("vim_mode").unwrap(), "normal");

    // Registered, the namespace applies to every keystroke.
    init(
        cx,
        r#"
        [[bindings]]
        keys = "x"
        command = "pad.normal_x"
        context = "pad && vim.mode == normal"
        "#,
    );
    cx.update(|cx| register_context_namespace(cx, "vim"));
    assert_eq!(cx.update(|cx| context_stack(&contexts, cx)), expected);
    let (view, cx) = cx.add_window_view(|_, cx| Pad::new("pad vim_mode=normal", cx));
    let log = focus_pad(&view, cx);
    cx.simulate_keystrokes("x");
    assert_eq!(
        ran_and_typed(&log),
        (vec!["pad.normal_x".into()], "".into())
    );
}
