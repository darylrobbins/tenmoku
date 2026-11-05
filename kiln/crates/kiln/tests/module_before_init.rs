// SPDX-License-Identifier: Apache-2.0

//! A library crate's `kiln::module!` in an app that sets up kiln-platform's
//! commands but never runs `kiln::init`, as an app's test harness may: its
//! `#[kiln::command]` methods are registered at once, so `typed_command`
//! finds them and `dispatch_invocation` runs one; and once `init` runs
//! afterwards, it doesn't register them a second time, while their keys
//! are still resolved then.

use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::{
    App, Context, FocusHandle, Focusable, Global, InteractiveElement as _, IntoElement, Render,
    Styled as _, TestAppContext, Window, div,
};
use kiln::{
    CommandInfo, Config, Contributions, Cx,
    command::{CommandId, Invocation},
    platform::command_actions::typed_command,
};

/// The library crate the commands are compiled into: an integration
/// test's own records are the app's, which only `kiln::app` adds, so the
/// records below are written as `#[kiln::command]` writes them in a
/// library crate of this name.
const LIBRARY: &str = "notes_lib";

/// A note view, whose commands are `#[kiln::command]` methods.
struct NoteView {
    focus: FocusHandle,
}

#[kiln::command(context = "note")]
impl NoteView {
    /// Rename Note
    #[command(id = "notes_lib.rename", key = "f2")]
    fn rename(&mut self, cx: &mut Cx<Self>) {
        log(cx, "rename");
    }

    /// Pin Note
    #[command(id = "notes_lib.pin")]
    fn pin(&mut self, cx: &mut Cx<Self>) {
        log(cx, "pin");
    }
}

impl Render for NoteView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let element = div().track_focus(&self.focus).size_full();
        kiln::__private::attach!(NoteView, element, cx)
    }
}

impl Focusable for NoteView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// How often each command was registered.
static RENAMES: AtomicUsize = AtomicUsize::new(0);
static PINS: AtomicUsize = AtomicUsize::new(0);

fn register_rename(cx: &mut App, info: CommandInfo) {
    RENAMES.fetch_add(1, Ordering::SeqCst);
    kiln::register_command::<Rename>(cx, info);
}

fn register_pin(cx: &mut App, info: CommandInfo) {
    PINS.fetch_add(1, Ordering::SeqCst);
    kiln::register_command::<Pin>(cx, info);
}

kiln::__private::inventory::submit! {
    kiln::__private::CommandRecord {
        id: "notes_lib.rename",
        title: "Rename Note",
        context: Some("note"),
        keys: Some("f2"),
        menu: None,
        order: 0,
        palette: true,
        file: file!(),
        line: line!(),
        krate: LIBRARY,
        bin: None,
        test_dir: None,
        register: register_rename,
        run: kiln::__private::run_in_view,
    }
}

kiln::__private::inventory::submit! {
    kiln::__private::CommandRecord {
        id: "notes_lib.pin",
        title: "Pin Note",
        context: Some("note"),
        keys: None,
        menu: None,
        order: 0,
        palette: true,
        file: file!(),
        line: line!(),
        krate: LIBRARY,
        bin: None,
        test_dir: None,
        register: register_pin,
        run: kiln::__private::run_in_view,
    }
}

/// The library's `kiln::module!(pub fn module);`, as it expands there.
fn module(cx: &mut App) {
    kiln::__private::add_crate_commands(cx, LIBRARY);
}

/// What the commands did, in order.
#[derive(Default)]
struct Log(Vec<&'static str>);

impl Global for Log {}

fn log(cx: &mut App, line: &'static str) {
    cx.default_global::<Log>().0.push(line);
}

fn id(id: &str) -> CommandId {
    CommandId::parse(id).unwrap()
}

#[gpui::test]
fn a_module_registers_its_commands_before_init_and_init_not_again(cx: &mut TestAppContext) {
    cx.update(|cx| {
        // As an app's test harness sets up commands, without kiln::init.
        kiln::commands::init(cx, "Notes");
        module(cx);
    });
    assert_eq!(RENAMES.load(Ordering::SeqCst), 1);
    assert_eq!(PINS.load(Ordering::SeqCst), 1);

    // typed_command finds them, with their titles and context.
    cx.read(|cx| {
        let rename = typed_command(&id("notes_lib.rename"), cx).expect("registered");
        assert_eq!(rename.title, "Rename Note");
        assert_eq!(rename.action_name, "notes_lib::Rename");
        assert!(rename.context.is_some());
        let pin = typed_command(&id("notes_lib.pin"), cx).expect("registered");
        assert_eq!(pin.title, "Pin Note");
    });

    // dispatch_invocation runs one in a focused note.
    let (view, cx) = cx.add_window_view(|_, cx| NoteView {
        focus: cx.focus_handle(),
    });
    cx.update(|window, cx| {
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        kiln::dispatch_invocation(Invocation::new(id("notes_lib.pin")), window, cx).unwrap()
    });
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| cx.global::<Log>().0.clone()), ["pin"]);

    // init afterwards registers neither again, but still resolves the
    // keys they bring.
    cx.update(|_, cx| {
        kiln::init(cx, Config::new("com.example.Notes", "Notes"));
    });
    cx.run_until_parked();
    assert_eq!(RENAMES.load(Ordering::SeqCst), 1);
    assert_eq!(PINS.load(Ordering::SeqCst), 1);
    cx.read(|cx| {
        let ids: Vec<_> = (Contributions::commands(cx).iter())
            .map(|command| command.id.to_string())
            .collect();
        assert_eq!(ids, ["notes_lib.rename", "notes_lib.pin"]);
        assert!(typed_command(&id("notes_lib.rename"), cx).is_some());
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| cx.global::<Log>().0.clone()),
        ["pin", "rename"]
    );
}
