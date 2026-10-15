// SPDX-License-Identifier: LicenseRef-Proprietary

//! `kiln::testing`: an app opened headless with no user files, driven by
//! keys, commands and menus, its workspace and active item reached typed;
//! and a config checked on every platform.

use std::panic::{AssertUnwindSafe, catch_unwind};

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, Render, SharedString, Styled as _, Task, TestAppContext, Window, div,
};
use kiln::{
    Config,
    command::Value,
    shell::{PaneItem, command_palette::COMMAND_PALETTE},
    testing::{TestApp, assert_clean_config},
};

mod notes {
    kiln::commands!(
        notes,
        [
            /// New Note
            NewNote,
        ]
    );
}

struct Note(FocusHandle);

impl Render for Note {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.0).size_full()
    }
}

impl Focusable for Note {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.0.clone()
    }
}

impl PaneItem for Note {
    const KIND: &'static str = "test.note";

    fn tab_title(&self, _: &App) -> SharedString {
        "note".into()
    }

    fn restore(_: Value, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        Task::ready(Some(cx.new(|cx| Note(cx.focus_handle()))))
    }
}

fn add_note(
    workspace: &mut kiln::shell::Workspace,
    window: &mut Window,
    cx: &mut Context<kiln::shell::Workspace>,
) {
    let note = cx.new(|cx| Note(cx.focus_handle()));
    workspace.add_item(Box::new(note), window, cx);
}

fn builder() -> kiln::AppBuilder {
    kiln::app("dev.kiln.Notes", "Kiln Notes")
        .keymap(
            "keymap.toml",
            "[[bindings]]\nkeys = \"secondary-shift-w\"\ncommand = \"window.close\"\n",
        )
        .commands(|cx| {
            notes::register(cx);
            cx.on_action(|_: &notes::NewNote, cx: &mut App| {
                cx.default_global::<Made>().0 += 1;
            });
        })
        .quit_on_last_window(false)
        .item::<Note>()
        .workspace(|workspace, window, cx| {
            if workspace.active_item(cx).is_none() {
                add_note(workspace, window, cx);
            }
        })
}

#[derive(Default)]
struct Made(usize);

impl gpui::Global for Made {}

/// The modal the shell shows, if any.
fn modal(app: &mut TestApp) -> Option<kiln::shell::ModalId> {
    let shell = app
        .workspace()
        .read_with(app.cx(), |w, _| w.shell().clone());
    shell.read_with(app.cx(), |shell, cx| shell.modal(cx))
}

#[gpui::test]
fn open_starts_headless_and_the_palette_opens_by_key(cx: &mut TestAppContext) {
    let mut app = TestApp::open(builder(), cx);
    assert_eq!(modal(&mut app), None);
    app.press("secondary-shift-p");
    assert_eq!(modal(&mut app), Some(COMMAND_PALETTE));
}

#[gpui::test]
fn commands_menus_workspace_and_active_item(cx: &mut TestAppContext) {
    let mut app = TestApp::open(builder(), cx);
    assert!(app.active_item::<Note>().is_some());
    app.run_command("notes.new_note");
    assert_eq!(app.cx().read(|cx| cx.global::<Made>().0), 1);
    app.menu("View", "Command Palette");
    assert_eq!(modal(&mut app), Some(COMMAND_PALETTE));
}

fn config(keymap: &str) -> Config {
    Config::new("dev.kiln.T", "T")
        .commands(notes::register)
        .keymap("keymap.toml", keymap.to_owned())
        .default_keys(true)
        .default_menus(true)
}

const CLEAN: &str = "[[bindings]]\nkeys = \"secondary-n\"\ncommand = \"notes.new_note\"\n";

#[gpui::test]
fn a_clean_config_passes(cx: &mut TestAppContext) {
    cx.update(|cx| {
        assert_eq!(kiln::init(cx, config(CLEAN)), []);
        assert_clean_config(&config(CLEAN), cx);
    });
}

#[gpui::test]
fn a_binding_wrong_on_one_platform_panics_listing_it(cx: &mut TestAppContext) {
    // `notes.nope` is known to no platform, but only Windows is asked for it.
    let bad = format!(
        "{CLEAN}\n[[bindings]]\nkeys = \"secondary-m\"\ncommand = \"notes.nope\"\nplatform = \"windows\"\n"
    );
    cx.update(|cx| {
        kiln::init(cx, config(CLEAN));
        let result = catch_unwind(AssertUnwindSafe(|| assert_clean_config(&config(&bad), cx)));
        let message = *result.expect_err("panics").downcast::<String>().unwrap();
        assert!(message.contains("Windows"), "{message}");
        assert!(message.contains("keymap.toml"), "{message}");
        assert!(message.contains("notes.nope"), "{message}");
        assert!(
            !message.contains("MacOs") && !message.contains("Linux"),
            "{message}"
        );
    });
}

/// A catalog of the commands a view runs, which no registry holds.
struct ViewCommands;

impl kiln::core::CommandCatalog for ViewCommands {
    fn check(
        &self,
        id: &kiln::command::CommandId,
        _: &Value,
    ) -> Result<(), kiln::core::CatalogError> {
        if id.as_str() == "view.zoom" {
            Ok(())
        } else {
            Err(kiln::core::CatalogError::UnknownCommand)
        }
    }
}

#[gpui::test]
fn a_command_only_a_config_catalog_knows_is_clean(cx: &mut TestAppContext) {
    let keymap =
        format!("{CLEAN}\n[[bindings]]\nkeys = \"secondary-m\"\ncommand = \"view.zoom\"\n");
    let make = || config(&keymap).catalog(ViewCommands);
    cx.update(|cx| {
        assert_eq!(kiln::init(cx, make()), []);
        assert_clean_config(&make(), cx);
    });
}

#[gpui::test]
fn chrome_is_linux_s_and_no_user_file_is_read(cx: &mut TestAppContext) {
    let dir = tempfile::TempDir::new().unwrap();
    let keymap = dir.path().join("keymap.toml");
    std::fs::write(&keymap, "not [valid").unwrap();
    let mut app = TestApp::open(
        builder().config(move |config| config.user_keymap(keymap)),
        cx,
    );
    let shell = app
        .workspace()
        .read_with(app.cx(), |w, _| w.shell().clone());
    let menus = shell.read_with(app.cx(), |shell, _| shell.menu_bar().is_some());
    assert!(menus, "menus in the window, as on Linux");
}

#[gpui::test]
fn closing_the_window_by_command_menu_or_key_leaves_nothing_to_draw(cx: &mut TestAppContext) {
    let closers: [fn(&mut TestApp); 3] = [
        |app| app.run_command("window.close"),
        |app| app.menu("Window", "Close Window"),
        |app| app.press("secondary-shift-w"),
    ];
    for (ix, close) in closers.into_iter().enumerate() {
        let mut app = TestApp::open(builder(), cx);
        close(&mut app);
        let open = app.cx().windows().len();
        assert_eq!(open, 0, "windows left after closer {ix}");
        app.draw();
    }
}
