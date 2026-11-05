// SPDX-License-Identifier: Apache-2.0

//! The command palette: the commands that apply where focus was, with
//! their keys, filtered by what's typed; a pick runs its command there,
//! from the palette, in that context; Escape gives focus back.

use std::path::Path;

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window, div,
};
use kiln_command::{
    Actor, CommandId, CommandMeta, ContextFrame, ContextPredicate, ContextStack, Dispatched,
    Effect, Invocation, Registry, Source, Value,
};
use kiln_core::{Keymap, KeymapLayer, shortcut_label};
use kiln_platform::{
    commands::{self, AppCommands, RunCommand},
    keys,
};
use kiln_shell::{
    Picker, Shell,
    command_palette::{self, COMMAND_PALETTE, CommandPalette},
};

const KEYS: &str = r#"
[[bindings]]
keys = "f2"
command = "command_palette.toggle"

[[bindings]]
keys = "f3"
command = "notes.archive"
context = "notes"

[[bindings]]
keys = "f4"
command = "notes.rename"
context = "elsewhere"

[[bindings]]
keys = "enter"
command = "picker.confirm"
context = "picker"

[[bindings]]
keys = "escape"
command = "picker.dismiss"
context = "picker"
"#;

/// The notes view: the commands it ran.
struct Notes {
    focus: FocusHandle,
    ran: Vec<Invocation>,
}

impl Focusable for Notes {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Notes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("notes")
            .track_focus(&self.focus)
            .on_action(cx.listener(|notes, run: &RunCommand, _, cx| {
                if run.0.command.as_str().starts_with("notes.") {
                    notes.ran.push(run.0.clone());
                } else {
                    cx.propagate();
                }
            }))
            .size_full()
    }
}

struct Host {
    shell: Entity<Shell>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.shell.clone())
    }
}

/// The notes' commands: Archive in their context, Rename elsewhere.
fn registry() -> Registry<AppCommands> {
    let mut registry = Registry::new();
    let done = |_: &mut &mut gpui::App, _: &Actor| Ok(Dispatched::new(Effect::Nothing));
    (registry
        .register("notes.archive", "Archive Note", done)
        .unwrap())
    .context(ContextPredicate::identifier("notes"));
    (registry
        .register("notes.rename", "Rename Note", done)
        .unwrap())
    .context(ContextPredicate::identifier("elsewhere"));
    registry
}

/// Tag Note, which takes args.
fn tag() -> CommandMeta {
    CommandMeta {
        id: CommandId::parse("notes.tag").unwrap(),
        title: "Tag Note".into(),
        description: String::new(),
        args_schema: Some(Value::Bool(true)),
        context: None,
        mutates_buffer: false,
    }
}

fn open(cx: &mut TestAppContext) -> (Entity<Notes>, Entity<Shell>, &mut VisualTestContext) {
    cx.update(|cx| {
        commands::init(cx, "Notes");
        kiln_shell::register(cx);
        let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), KEYS, None);
        assert_eq!(diagnostics, []);
        keys::init(cx, Keymap::from_layers([layer.unwrap()]));
        let added = registry()
            .commands()
            .cloned()
            .chain([tag()])
            .collect::<Vec<_>>();
        command_palette::add_commands(cx, added);
    });
    let (host, cx) = cx.add_window_view(|window, cx| {
        let notes = cx.new(|cx| Notes {
            focus: cx.focus_handle(),
            ran: Vec::new(),
        });
        let shell = cx.new(|cx| {
            let mut shell = Shell::new("notes_app", window, cx);
            shell.set_center(notes, cx);
            shell
        });
        Host { shell }
    });
    let shell = host.read_with(cx, |host, _| host.shell.clone());
    let notes = shell.read_with(cx, |shell, _| {
        shell.center().unwrap().clone().downcast::<Notes>().unwrap()
    });
    cx.update(|window, cx| window.focus(&notes.focus_handle(cx), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (notes, shell, cx)
}

fn palette(
    shell: &Entity<Shell>,
    cx: &mut VisualTestContext,
) -> Option<Entity<Picker<CommandPalette>>> {
    shell.read_with(cx, |shell, cx| {
        let layer = shell.modals().read(cx);
        (layer.modal() == Some(COMMAND_PALETTE))
            .then(|| layer.modal_view().unwrap().clone().downcast().unwrap())
    })
}

/// The palette's rows: each title, and its key.
fn rows(
    picker: &Entity<Picker<CommandPalette>>,
    cx: &mut VisualTestContext,
) -> Vec<(String, Option<String>)> {
    picker.read_with(cx, |picker, cx| {
        let palette = &picker.delegate;
        (palette.matches().iter().enumerate())
            .map(|(ix, entry)| {
                let key = palette.key_label(ix, cx).map(|key| key.to_string());
                (entry.title.to_string(), key)
            })
            .collect()
    })
}

/// Presses `keys`, then draws a frame, as the window would.
fn press(cx: &mut VisualTestContext, keys: &str) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn focused(notes: &Entity<Notes>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| notes.focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn lists_what_applies_with_keys_and_runs_a_pick_there(cx: &mut TestAppContext) {
    let (notes, shell, cx) = open(cx);
    press(cx, "f2");
    let picker = palette(&shell, cx).expect("it shows");
    let stack = ContextStack::new()
        .with(ContextFrame::new("notes_app"))
        .with(ContextFrame::new("notes"));
    picker.read_with(cx, |picker, _| {
        assert_eq!(*picker.delegate.context_stack(), stack)
    });
    let rows = rows(&picker, cx);
    let f3 = shortcut_label(&"f3".parse().unwrap());
    assert!(
        rows.contains(&("Archive Note".into(), Some(f3.clone()))),
        "{rows:?}"
    );
    assert!(rows.contains(&("Quit".into(), None)), "{rows:?}");
    for left_out in ["Rename Note", "Tag Note", "Show All Commands"] {
        assert!(
            !rows.iter().any(|(title, _)| title == left_out),
            "{left_out}: {rows:?}"
        );
    }
    let mut titles: Vec<_> = rows.iter().map(|(title, _)| title.clone()).collect();
    titles.sort();
    assert_eq!(
        rows.iter()
            .map(|(title, _)| title.clone())
            .collect::<Vec<_>>(),
        titles
    );

    // Typed letters filter, in order, ignoring case.
    cx.simulate_input("arch");
    cx.run_until_parked();
    assert_eq!(super_rows(&picker, cx), ["Archive Note"]);

    press(cx, "enter");
    cx.run_until_parked();
    assert_eq!(palette(&shell, cx).map(|_| ()), None);
    assert!(focused(&notes, cx));
    let ran = notes.read_with(cx, |notes, _| notes.ran.clone());
    let archive = Invocation::of("notes.archive")
        .unwrap()
        .source(Source::Palette)
        .context(stack);
    assert_eq!(ran, [archive]);
}

fn super_rows(picker: &Entity<Picker<CommandPalette>>, cx: &mut VisualTestContext) -> Vec<String> {
    rows(picker, cx)
        .into_iter()
        .map(|(title, _)| title)
        .collect()
}

#[gpui::test]
fn escape_and_the_toggle_give_focus_back(cx: &mut TestAppContext) {
    let (notes, shell, cx) = open(cx);
    press(cx, "f2");
    assert!(palette(&shell, cx).is_some());
    assert!(!focused(&notes, cx));
    press(cx, "escape");
    assert!(palette(&shell, cx).is_none());
    assert!(focused(&notes, cx));

    press(cx, "f2");
    assert!(palette(&shell, cx).is_some());
    press(cx, "f2");
    assert!(palette(&shell, cx).is_none());
    assert!(focused(&notes, cx));
    assert_eq!(notes.read_with(cx, |notes, _| notes.ran.len()), 0);
}
