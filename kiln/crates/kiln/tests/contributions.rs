// SPDX-License-Identifier: Apache-2.0

//! `kiln::Contributions`: commands added with their default keys and menu
//! items, before `init` or after it, which resolves the keymap and menus
//! again with them.

use std::{
    cell::RefCell,
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

use gpui::{
    App, Context, FocusHandle, InteractiveElement as _, IntoElement, Render, TestAppContext,
    Window, div,
};
use kiln::{
    CommandSpec, Config, Contributions, Keys,
    command::{CommandId, ContextStack},
    core::{KeySequence, Keystroke, Severity},
    platform::{
        config::{BatchSender, batch_channel, diagnostics},
        keys, menus,
    },
    shell::command_palette::commands_in,
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

mod extra {
    kiln::commands!(
        extra,
        [
            /// Archive
            Archive,
            /// Pin
            Pin,
            /// Share
            Share,
        ]
    );
}

const KEYMAP: &str = r#"
[[bindings]]
keys = "secondary-n"
command = "notes.new_note"
"#;

const MENUS: &str = r#"
[[menus]]
name = "File"

[[menus.items]]
title = "New Note"
command = "notes.new_note"

[[menus]]
name = "Edit"

[[menus.items]]
title = "New Note Too"
id = "again"
command = "notes.new_note"
"#;

/// The app's config, its user files not watched: a test sends what the
/// watcher would.
fn config(keymap: &str, menus: &str) -> (Config, BatchSender) {
    let (sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .keymap("keymap.toml", keymap.to_owned())
        .menus("menus.toml", menus.to_owned())
        .watch_with(batches);
    (config, sender)
}

/// A directory under the system's temporary one, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "kiln-contributions-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn id(id: &str) -> CommandId {
    CommandId::parse(id).unwrap()
}

/// The command `keys` runs outside any context, in the keymap in use.
fn bound(keys: &str, cx: &mut TestAppContext) -> Option<String> {
    let keystroke = Keystroke::parse(keys).unwrap();
    cx.update(|cx| {
        let invocation = keys::current_keymap(cx).lookup(&keystroke, &ContextStack::new())?;
        Some(invocation.command.to_string())
    })
}

/// The keys bound to `command`, in the keymap in use.
fn keys_of(command: &str, cx: &mut TestAppContext) -> Vec<String> {
    let command = id(command);
    cx.update(|cx| {
        keys::current_keymap(cx)
            .bindings_for(&command)
            .iter()
            .map(|binding| binding.keys.to_string())
            .collect()
    })
}

/// `keys` as the keymap shows them on this platform.
fn shown(keys: &str) -> String {
    KeySequence::parse(keys).unwrap().to_string()
}

/// The command and shortcut of each item in the installed menu `name`.
fn menu_items(name: &str, cx: &App) -> Vec<(String, Option<String>)> {
    let keymap = keys::current_keymap(cx);
    let bar = menus::installed(cx).expect("the menus are installed");
    let menu = bar.menu(name).unwrap_or_else(|| panic!("no menu {name}"));
    menu.items
        .iter()
        .filter_map(|item| item.command())
        .map(|item| {
            let shortcut = item.shortcut(&keymap).map(|keys| keys.to_string());
            (item.command.to_string(), shortcut)
        })
        .collect()
}

/// What `run` panicked with.
fn panic_message(run: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(run)).expect_err("it panics");
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => (*payload.downcast::<&str>().unwrap()).to_owned(),
    }
}

struct Notes {
    focus: FocusHandle,
    archived: Rc<RefCell<usize>>,
}

impl Render for Notes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &extra::Archive, _, _| {
                *this.archived.borrow_mut() += 1;
            }))
    }
}

#[gpui::test]
fn a_command_added_after_init_runs_from_its_key_and_shows_in_the_palette_and_menus(
    cx: &mut TestAppContext,
) {
    let (config, _sender) = config(KEYMAP, MENUS);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert_eq!(diagnostics, []);
    let archived = Rc::new(RefCell::new(0));
    let (view, cx) = cx.add_window_view({
        let archived = archived.clone();
        move |_, cx| Notes {
            focus: cx.focus_handle(),
            archived,
        }
    });
    cx.update(|window, cx| {
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    cx.run_until_parked();

    cx.update(|_, cx| {
        Contributions::add_command(
            cx,
            CommandSpec::new::<extra::Archive>("Archive Note")
                .keys("secondary-e")
                .menu("File", 0),
        );
        Contributions::add_command(
            cx,
            CommandSpec::new::<extra::Share>("Share Note")
                .keys(Keys::ByPlatform(vec![
                    ("macos".into(), "cmd-shift-s".into()),
                    ("default".into(), "ctrl-shift-s".into()),
                ]))
                .menu("Sharing", 0),
        );
    });
    cx.run_until_parked();

    let secondary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.simulate_keystrokes(&format!("{secondary}-e"));
    assert_eq!(*archived.borrow(), 1, "its key runs it");

    cx.update(|_, cx| {
        let palette = commands_in(&ContextStack::new(), cx);
        let titles: Vec<&str> = palette.iter().map(|entry| entry.title.as_ref()).collect();
        assert!(titles.contains(&"Archive Note"), "{titles:?}");
        assert!(titles.contains(&"Share Note"), "{titles:?}");
        let names: Vec<&str> = menus::installed(cx)
            .unwrap()
            .menus
            .iter()
            .map(|menu| menu.name.as_str())
            .collect();
        assert_eq!(names, ["File", "Edit", "Sharing"], "a new menu goes last");
        assert_eq!(
            menu_items("File", cx),
            [
                ("notes.new_note".into(), Some(shown("secondary-n"))),
                ("extra.archive".into(), Some(shown("secondary-e"))),
            ],
            "after the app's items, with its shortcut"
        );
        assert_eq!(
            menu_items("Sharing", cx),
            [("extra.share".into(), Some(shown("secondary-shift-s")))]
        );
    });
}

#[gpui::test]
fn contributed_items_take_their_order_in_their_menu(cx: &mut TestAppContext) {
    let (config, _sender) = config(KEYMAP, MENUS);
    cx.update(|cx| {
        kiln::init(cx, config);
        Contributions::add_commands(
            cx,
            [
                CommandSpec::new::<extra::Archive>("Archive").menu("Edit", 2),
                CommandSpec::new::<extra::Pin>("Pin").menu("Edit", 1),
            ],
        );
    });
    cx.run_until_parked();
    let commands: Vec<String> = cx
        .update(|cx| menu_items("Edit", cx))
        .into_iter()
        .map(|(command, _)| command)
        .collect();
    assert_eq!(commands, ["notes.new_note", "extra.pin", "extra.archive"]);
}

#[gpui::test]
fn bindings_and_items_naming_a_command_registered_later_take_it_up(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let user = dir.path("keymap.toml");
    fs::write(
        &user,
        "[[bindings]]\nkeys = \"secondary-j\"\ncommand = \"extra.pin\"\n",
    )
    .unwrap();
    let keymap =
        format!("{KEYMAP}\n[[bindings]]\nkeys = \"secondary-k\"\ncommand = \"extra.pin\"\n");
    let menus = format!("{MENUS}\n[[menus.items]]\ntitle = \"Pin It\"\ncommand = \"extra.pin\"\n");
    let (config, _sender) = config(&keymap, &menus);
    let found = cx.update(|cx| kiln::init(cx, config.user_keymap(&user)));
    assert_eq!(found.len(), 3, "unknown until registered: {found:?}");
    assert_eq!(keys_of("extra.pin", cx), Vec::<String>::new());

    cx.update(|cx| Contributions::add_command(cx, CommandSpec::new::<extra::Pin>("Pin")));
    cx.run_until_parked();
    assert_eq!(
        keys_of("extra.pin", cx),
        [shown("secondary-k"), shown("secondary-j")],
        "the app's and the user's bindings take it up"
    );
    let edit = cx.update(|cx| menu_items("Edit", cx));
    assert_eq!(
        edit.last(),
        Some(&("extra.pin".into(), Some(shown("secondary-j")))),
        "{edit:?}"
    );
    cx.update(|cx| assert_eq!(diagnostics(cx), [], "the warnings are gone"));
}

#[gpui::test]
fn two_contributed_commands_with_the_same_keys_are_reported(cx: &mut TestAppContext) {
    let (config, _sender) = config(KEYMAP, MENUS);
    let found = cx.update(|cx| {
        Contributions::add_commands(
            cx,
            [
                CommandSpec::new::<extra::Archive>("Archive").keys("secondary-e"),
                CommandSpec::new::<extra::Share>("Share").keys("secondary-e"),
                // Another context: no clash.
                CommandSpec::new::<extra::Pin>("Pin")
                    .context("list")
                    .keys("secondary-e"),
            ],
        );
        kiln::init(cx, config)
    });
    let expected = format!(
        "{} is bound to both extra.archive and extra.share",
        shown("secondary-e")
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(found[0].message, expected);
    assert_eq!(
        bound("secondary-e", cx).as_deref(),
        Some("extra.share"),
        "the later one wins"
    );
}

#[gpui::test]
fn the_same_keys_twice_panic_under_fail_fast(cx: &mut TestAppContext) {
    let (config, _sender) = config(KEYMAP, MENUS);
    let message = panic_message(|| {
        cx.update(|cx| {
            Contributions::add_commands(
                cx,
                [
                    CommandSpec::new::<extra::Archive>("Archive").keys("secondary-e"),
                    CommandSpec::new::<extra::Share>("Share").keys("secondary-e"),
                ],
            );
            kiln::init(cx, config.fail_fast(true));
        });
    });
    assert!(message.contains("1 problem"), "{message}");
    assert!(
        message.contains("is bound to both extra.archive and extra.share"),
        "{message}"
    );
}

#[gpui::test]
fn the_same_keys_added_after_init_panic_under_fail_fast(cx: &mut TestAppContext) {
    let (config, _sender) = config(KEYMAP, MENUS);
    cx.update(|cx| kiln::init(cx, config.fail_fast(true)));
    let message = panic_message(|| {
        cx.update(|cx| {
            Contributions::add_commands(
                cx,
                [
                    CommandSpec::new::<extra::Archive>("Archive").keys("secondary-e"),
                    CommandSpec::new::<extra::Share>("Share").keys("secondary-e"),
                ],
            );
        });
        cx.run_until_parked();
    });
    assert!(
        message.contains("is bound to both extra.archive and extra.share"),
        "{message}"
    );
}

/// Writes `text` to `path` and sends a batch naming it.
fn save(path: &Path, text: &str, sender: &BatchSender, cx: &mut TestAppContext) {
    fs::write(path, text).unwrap();
    sender.send(vec![path.to_owned()]);
    cx.run_until_parked();
}

#[gpui::test]
fn the_apps_and_users_keymaps_override_contributed_keys_across_reloads(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let user = dir.path("keymap.toml");
    fs::write(
        &user,
        "[[bindings]]\nkeys = \"secondary-r\"\ncommand = \"notes.new_note\"\n",
    )
    .unwrap();
    let keymap =
        format!("{KEYMAP}\n[[bindings]]\nkeys = \"secondary-e\"\ncommand = \"notes.new_note\"\n");
    let (config, sender) = config(&keymap, MENUS);
    cx.update(|cx| kiln::init(cx, config.user_keymap(&user)));
    cx.update(|cx| {
        Contributions::add_commands(
            cx,
            [
                CommandSpec::new::<extra::Archive>("Archive").keys("secondary-e"),
                CommandSpec::new::<extra::Share>("Share").keys("secondary-r"),
                CommandSpec::new::<extra::Pin>("Pin").keys("secondary-p"),
            ],
        );
    });
    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| Contributions::resolutions(cx)),
        1,
        "three additions in one update resolve once"
    );
    assert_eq!(bound("secondary-e", cx).as_deref(), Some("notes.new_note"));
    assert_eq!(bound("secondary-r", cx).as_deref(), Some("notes.new_note"));
    assert_eq!(bound("secondary-p", cx).as_deref(), Some("extra.pin"));

    save(
        &user,
        "[[bindings]]\nkeys = \"secondary-y\"\ncommand = \"extra.share\"\n",
        &sender,
        cx,
    );
    assert_eq!(
        bound("secondary-p", cx).as_deref(),
        Some("extra.pin"),
        "a reload keeps the contributed keys"
    );
    assert_eq!(bound("secondary-r", cx).as_deref(), Some("extra.share"));
    assert_eq!(bound("secondary-y", cx).as_deref(), Some("extra.share"));

    cx.update(|cx| {
        Contributions::add_command(
            cx,
            CommandSpec::new::<extra::Pin>("Pin").keys("secondary-o"),
        );
    });
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| Contributions::resolutions(cx)), 2);
    assert_eq!(keys_of("extra.pin", cx), [shown("secondary-o")], "replaced");
    assert_eq!(bound("secondary-y", cx).as_deref(), Some("extra.share"));
}

/// A command that takes args, which a key or menu item can't give it.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = extra, no_json)]
struct Open {
    index: usize,
}

impl kiln::Command for Open {}

#[gpui::test]
fn contributed_keys_and_items_are_checked_as_a_keymap_and_menu_tables_are(cx: &mut TestAppContext) {
    let (config, _sender) = config(KEYMAP, MENUS);
    let found = cx.update(|cx| {
        Contributions::add_commands(
            cx,
            [
                CommandSpec::new::<Open>("Open")
                    .keys("secondary-o")
                    .menu("File", 0),
                // Every platform's keys are checked, wherever it runs.
                CommandSpec::new::<extra::Archive>("Archive").keys(Keys::ByPlatform(vec![
                    ("macos".into(), "cmd-nope-e".into()),
                    ("default".into(), "ctrl-nope-e".into()),
                ])),
                // A chord a click can't finish.
                CommandSpec::new::<extra::Pin>("Pin").keys("secondary-k secondary-click"),
            ],
        );
        kiln::init(cx, config)
    });
    let found: Vec<(Severity, String, String)> = found
        .into_iter()
        .map(|found| (found.severity, found.key.unwrap(), found.message))
        .collect();
    assert_eq!(found.len(), 5, "{found:#?}");
    assert_eq!(found[0].0, Severity::Error);
    assert_eq!(found[0].1, "extra.open.command");
    assert!(
        found[0].2.starts_with("bad args for extra.open"),
        "{found:#?}"
    );
    assert_eq!((found[1].0, found[2].0), (Severity::Error, Severity::Error));
    assert_eq!(found[1].1, "extra.archive.keys");
    assert_eq!(found[3].0, Severity::Warning);
    assert_eq!(found[3].1, "extra.pin.keys");
    assert!(
        found[3].2.contains("a click binds only alone"),
        "{found:#?}"
    );
    assert_eq!(found[4].1, "extra.open.menu");
    assert!(
        found[4].2.starts_with("bad args for extra.open"),
        "{found:#?}"
    );
    for command in ["extra.open", "extra.archive", "extra.pin"] {
        assert_eq!(keys_of(command, cx), Vec::<String>::new(), "{command}");
    }
    let file = cx.update(|cx| menu_items("File", cx));
    assert_eq!(file.len(), 1, "{file:?}");
}

#[gpui::test]
fn a_contributed_items_shortcut_resolves_in_its_commands_context(cx: &mut TestAppContext) {
    let (config, _sender) = config(KEYMAP, MENUS);
    cx.update(|cx| {
        Contributions::add_command(
            cx,
            CommandSpec::new::<extra::Pin>("Pin")
                .context("notes.list")
                .keys("secondary-l")
                .menu("File", 0),
        );
        kiln::init(cx, config)
    });
    let file = cx.update(|cx| menu_items("File", cx));
    assert_eq!(
        file.last(),
        Some(&("extra.pin".into(), Some(shown("secondary-l"))))
    );
}

/// Registers `extra.archive`, contributing `extra.pin` as it does.
fn register_archive_and_pin(cx: &mut App, info: kiln::CommandInfo) {
    kiln::register_command::<extra::Archive>(cx, info);
    Contributions::add_command(
        cx,
        CommandSpec::new::<extra::Pin>("Pin").keys("secondary-p"),
    );
}

#[gpui::test]
fn a_command_contributed_while_registering_another_is_registered(cx: &mut TestAppContext) {
    let (config, _sender) = config(KEYMAP, MENUS);
    let found = cx.update(|cx| {
        let mut archive = CommandSpec::new::<extra::Archive>("Archive");
        archive.register = register_archive_and_pin;
        Contributions::add_command(cx, archive);
        kiln::init(cx, config)
    });
    assert_eq!(found, []);
    assert_eq!(bound("secondary-p", cx).as_deref(), Some("extra.pin"));
    cx.update(|cx| {
        let palette = commands_in(&ContextStack::new(), cx);
        assert!(palette.iter().any(|entry| entry.title == "Pin"));
    });
}

/// Registers `extra.archive`, contributing a replacement of itself bound
/// to `secondary-q` as it does.
fn register_archive_and_replace_itself(cx: &mut App, info: kiln::CommandInfo) {
    kiln::register_command::<extra::Archive>(cx, info);
    Contributions::add_command(
        cx,
        CommandSpec::new::<extra::Archive>("Archive").keys("secondary-q"),
    );
}

#[gpui::test]
fn a_replacement_contributed_while_registering_after_init_is_kept(cx: &mut TestAppContext) {
    let (config, _sender) = config(KEYMAP, MENUS);
    cx.update(|cx| kiln::init(cx, config));
    cx.update(|cx| {
        let mut archive = CommandSpec::new::<extra::Archive>("Archive").keys("secondary-w");
        archive.register = register_archive_and_replace_itself;
        Contributions::add_command(cx, archive);
    });
    cx.run_until_parked();
    assert_eq!(bound("secondary-q", cx).as_deref(), Some("extra.archive"));
    assert_eq!(bound("secondary-w", cx), None, "the outer spec is replaced");
}

#[gpui::test]
fn diagnostics_follow_contributions_once_nothing_is_watched(cx: &mut TestAppContext) {
    let keymap =
        format!("{KEYMAP}\n[[bindings]]\nkeys = \"secondary-k\"\ncommand = \"extra.pin\"\n");
    // No user files and no batches: the service's task is over at once.
    let config = Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .keymap("keymap.toml", keymap);
    cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| diagnostics(cx).len()), 1);
    cx.update(|cx| Contributions::add_command(cx, CommandSpec::new::<extra::Pin>("Pin")));
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| diagnostics(cx).to_vec()), []);
    assert_eq!(bound("secondary-k", cx).as_deref(), Some("extra.pin"));
}
