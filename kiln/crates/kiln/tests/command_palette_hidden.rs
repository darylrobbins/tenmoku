// SPDX-License-Identifier: Apache-2.0

//! `#[kiln::command(palette = false)]` keeps a command out of the command
//! palette, and nothing else: it's registered, and runs by ID and by key,
//! while a command without the option is listed as before.

use gpui::TestAppContext;
use kiln::{
    Command as _, Cx,
    command::ContextStack,
    platform::command_actions::typed_command,
    shell::{Workspace, command_palette::commands_in},
    testing::TestApp,
};

/// Shown Note
#[kiln::command(key = "secondary-alt-1")]
fn shown_note(_: &mut Workspace, cx: &mut Cx<Workspace>) {
    cx.default_global::<Ran>().0.push("shown");
}

/// Hidden Note
#[kiln::command(key = "secondary-alt-2", palette = false)]
fn hidden_note(_: &mut Workspace, cx: &mut Cx<Workspace>) {
    cx.default_global::<Ran>().0.push("hidden");
}

#[derive(Default)]
struct Ran(Vec<&'static str>);

impl gpui::Global for Ran {}

fn open(cx: &mut TestAppContext) -> TestApp {
    TestApp::open(
        kiln::app("dev.kiln.PaletteHidden", "Kiln Palette Hidden")
            .restore_layout(false)
            .workspace(|_, _, _| {}),
        cx,
    )
}

fn ran(app: &mut TestApp) -> Vec<&'static str> {
    app.cx()
        .update(|_, cx| std::mem::take(&mut cx.default_global::<Ran>().0))
}

#[gpui::test]
fn a_palette_false_command_is_registered_and_runs_but_the_palette_leaves_it_out(
    cx: &mut TestAppContext,
) {
    let mut app = open(cx);
    let (shown, hidden) = (ShownNote::id(), HiddenNote::id());
    app.cx().update(|_, cx| {
        assert!(typed_command(&shown, cx).is_some_and(|command| !command.unlisted));
        assert!(typed_command(&hidden, cx).is_some_and(|command| command.unlisted));
        let listed = commands_in(&ContextStack::new(), cx);
        assert!(listed.iter().any(|entry| entry.id == shown));
        assert!(!listed.iter().any(|entry| entry.id == hidden));
    });

    app.run_command(hidden.as_str());
    assert_eq!(ran(&mut app), ["hidden"]);
    app.press("secondary-alt-2");
    assert_eq!(ran(&mut app), ["hidden"]);
    app.press("secondary-alt-1");
    assert_eq!(ran(&mut app), ["shown"]);
}
