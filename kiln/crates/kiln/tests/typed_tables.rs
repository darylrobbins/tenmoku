// SPDX-License-Identifier: LicenseRef-Proprietary

//! `kiln::keymap!` and `kiln::menus!`: typed tables that name commands by
//! type. `.keys(..)` binds its keys, per platform and in a context too, in
//! place of the commands' attribute keys, under the app's TOML and the
//! user's file; `.menu_bar(..)` installs its menus, with each command's
//! title and separators, in place of the contributed placement and any
//! `.menus(path, toml)`. A misspelt command type is a compile error
//! (`tables.rs`'s doctests); a menu named twice, or a command twice in a
//! menu, is an error as in a menu table, the table left out.

use std::{
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use gpui::{
    AnyView, App, AppContext as _, Context, IntoElement, Render, TestAppContext, Window, div,
};
use kiln::{
    CommandSpec, Config, Contributions, Cx,
    command::{CommandId, ContextFrame, ContextStack},
    core::{Diagnostic, KeySequence, Keystroke, Severity},
    platform::{config::batch_channel, keys, menus},
    shell::Workspace,
    testing::TestApp,
};

/// New Note
#[kiln::command(key = "secondary-n", menu = "File")]
fn new_note(_: &mut Workspace, _: &mut Cx<Workspace>) {}

/// Rename Note
#[kiln::command(key = "f6", menu = "Edit")]
fn rename_note(_: &mut Workspace, _: &mut Cx<Workspace>) {}

/// Archive
#[kiln::command(key = "secondary-e", menu = "Edit")]
fn archive(_: &mut Workspace, _: &mut Cx<Workspace>) {}

struct Empty;

impl Render for Empty {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn builder() -> kiln::AppBuilder {
    kiln::app("dev.kiln.TypedTables", "Typed Tables")
        .restore_layout(false)
        .root(|_, cx: &mut App| AnyView::from(cx.new(|_| Empty)))
}

/// The keys the keymap in use binds to `command`, with their contexts.
fn bindings(command: &str, app: &mut TestApp) -> Vec<(String, Option<String>)> {
    let command = CommandId::parse(command).unwrap();
    app.cx().read(|cx| {
        keys::current_keymap(cx)
            .bindings_for(&command)
            .iter()
            .map(|binding| {
                (
                    binding.keys.to_string(),
                    binding.context.as_ref().map(ToString::to_string),
                )
            })
            .collect()
    })
}

/// The command `keys` runs in `frames`, in the keymap in use.
fn bound(keys: &str, frames: &[&str], app: &mut TestApp) -> Option<String> {
    let keystroke = Keystroke::parse(keys).unwrap();
    let stack: ContextStack = frames.iter().map(|name| ContextFrame::new(*name)).collect();
    app.cx().read(|cx| {
        let invocation = keys::current_keymap(cx).lookup(&keystroke, &stack)?;
        Some(invocation.command.to_string())
    })
}

fn shown(keys: &str) -> String {
    KeySequence::parse(keys).unwrap().to_string()
}

/// A menu's items: each command's title, `-` for a separator.
fn menu(name: &str, app: &mut TestApp) -> Vec<String> {
    app.cx().read(|cx| {
        let bar = menus::installed(cx).expect("the menus are installed");
        let menu = bar.menu(name).unwrap_or_else(|| panic!("no menu {name}"));
        (menu.items.iter())
            .map(|item| {
                item.command()
                    .map_or("-".to_owned(), |item| item.title.clone())
            })
            .collect()
    })
}

fn menu_names(app: &mut TestApp) -> Vec<String> {
    app.cx().read(|cx| {
        let bar = menus::installed(cx).expect("the menus are installed");
        bar.menus.iter().map(|menu| menu.name.clone()).collect()
    })
}

#[gpui::test]
fn keys_bind_by_platform_and_in_a_context_in_place_of_the_attribute_keys(cx: &mut TestAppContext) {
    let table = kiln::keymap! {
        "secondary-shift-n" => NewNote,
        { macos = "cmd-r", linux = "f2", default = "ctrl-r" } => RenameNote in "note",
        "secondary-e" => kiln::app::Quit,
    };
    assert_eq!(table.bindings().len(), 3);
    let mut app = TestApp::open(builder().keys(table), cx);

    // The table's keys, on this (Linux) platform and in the context.
    assert_eq!(
        bindings("typed_tables.new_note", &mut app),
        [(shown("secondary-shift-n"), None)]
    );
    assert_eq!(
        bindings("typed_tables.rename_note", &mut app),
        [("f2".to_owned(), Some("note".to_owned()))]
    );
    assert_eq!(
        bound("f2", &["note"], &mut app).as_deref(),
        Some("typed_tables.rename_note")
    );
    assert_eq!(bound("f2", &[], &mut app), None);

    // The attribute keys are gone, even those of a command the table
    // doesn't name.
    assert_eq!(bound("secondary-n", &[], &mut app), None);
    assert_eq!(bound("f6", &[], &mut app), None);
    assert_eq!(bindings("typed_tables.archive", &mut app), []);
    // `secondary-e` is Quit's now.
    assert_eq!(
        bound("secondary-e", &[], &mut app).as_deref(),
        Some("app.quit")
    );
}

#[gpui::test]
fn the_apps_keymap_goes_over_the_table(cx: &mut TestAppContext) {
    let table = kiln::keymap! { "secondary-shift-n" => NewNote };
    let keymap = "[[bindings]]\nkeys = \"secondary-shift-n\"\ncommand = \"typed_tables.archive\"\n";
    let mut app = TestApp::open(builder().keys(table).keymap("keymap.toml", keymap), cx);
    assert_eq!(
        bound("secondary-shift-n", &[], &mut app).as_deref(),
        Some("typed_tables.archive")
    );
    assert_eq!(bound("secondary-n", &[], &mut app), None);
}

mod notes {
    kiln::commands!(
        notes,
        [
            /// New Note
            NewNote,
        ]
    );
}

/// A file under the system's temporary directory, removed when dropped.
struct TempFile(PathBuf);

impl TempFile {
    fn new(text: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "kiln-typed-tables-{}-{}.toml",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::write(&path, text).unwrap();
        Self(path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        fs::remove_file(&self.0).ok();
    }
}

/// `kiln::init` with the table over a command whose attribute key is
/// `secondary-n`, and the user's file `user`.
fn init_with_table(
    table: kiln::KeyTable,
    user: &TempFile,
    cx: &mut TestAppContext,
) -> Vec<kiln::core::Diagnostic> {
    // The user's file isn't watched: no thread but the test's.
    let (_sender, batches) = batch_channel();
    cx.update(|cx| {
        Contributions::add_command(
            cx,
            CommandSpec::new::<notes::NewNote>("New Note").keys("secondary-n"),
        );
        Contributions::set_key_table(cx, table);
        kiln::init(
            cx,
            Config::new("com.example.Notes", "Notes")
                .commands(notes::register)
                .user_keymap(&user.0)
                .watch_with(batches),
        )
    })
}

fn lookup(keys: &str, cx: &mut TestAppContext) -> Option<String> {
    let keystroke = Keystroke::parse(keys).unwrap();
    cx.update(|cx| {
        let keymap = keys::current_keymap(cx);
        Some(
            keymap
                .lookup(&keystroke, &ContextStack::new())?
                .command
                .to_string(),
        )
    })
}

#[gpui::test]
fn the_users_file_goes_over_the_table(cx: &mut TestAppContext) {
    let user = TempFile::new("[[bindings]]\nkeys = \"f3\"\ncommand = \"notes.new_note\"\n");
    let table = kiln::keymap! { "f2" => notes::NewNote };
    let diagnostics = init_with_table(table, &user, cx);
    assert_eq!(diagnostics, []);
    assert_eq!(lookup("f2", cx).as_deref(), Some("notes.new_note"));
    assert_eq!(lookup("f3", cx).as_deref(), Some("notes.new_note"));
    // Not the attribute key.
    assert_eq!(lookup("ctrl-n", cx), None);
}

#[gpui::test]
fn an_unknown_context_is_a_diagnostic(cx: &mut TestAppContext) {
    let user = TempFile::new("");
    let table = kiln::keymap! { "f2" => notes::NewNote in "((" };
    let diagnostics = init_with_table(table, &user, cx);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error
                && d.key.as_deref() == Some("notes.new_note.context")),
        "{diagnostics:?}"
    );
}

#[gpui::test]
fn menus_install_with_the_commands_titles_and_separators(cx: &mut TestAppContext) {
    let table = kiln::menus! {
        "File" => [NewNote, Separator, kiln::app::Quit],
        "Edit" => [RenameNote],
    };
    assert_eq!(table.menus().len(), 2);
    let mut app = TestApp::open(builder().menu_bar(table), cx);
    assert_eq!(menu_names(&mut app), ["File", "Edit"]);
    assert_eq!(menu("File", &mut app), ["New Note", "-", "Quit"]);
    // Archive's attribute menu placement is gone: not in Edit.
    assert_eq!(menu("Edit", &mut app), ["Rename Note"]);
}

#[gpui::test]
fn menus_replace_the_apps_menu_table(cx: &mut TestAppContext) {
    let toml = "[[menus]]\nname = \"Window\"\n\n[[menus.items]]\ntitle = \"Archive\"\n\
                command = \"typed_tables.archive\"\n";
    let table = kiln::menus! { "File" => [NewNote] };
    let mut app = TestApp::open(builder().menus("menus.toml", toml).menu_bar(table), cx);
    assert_eq!(menu_names(&mut app), ["File"]);
    assert_eq!(menu("File", &mut app), ["New Note"]);
}

#[gpui::test]
fn menu_items_show_the_shortcuts_the_keys_table_gives(cx: &mut TestAppContext) {
    let keys = kiln::keymap! { "secondary-shift-n" => NewNote };
    let menus = kiln::menus! { "File" => [NewNote] };
    let mut app = TestApp::open(builder().keys(keys).menu_bar(menus), cx);
    let shortcut = app.cx().read(|cx| {
        let keymap = keys::current_keymap(cx);
        let bar = menus::installed(cx).unwrap();
        let item = bar.menu("File").unwrap().items[0]
            .command()
            .unwrap()
            .clone();
        item.shortcut(&keymap).map(|keys| keys.to_string())
    });
    assert_eq!(shortcut, Some(shown("secondary-shift-n")));
}

mod drafts {
    kiln::commands!(
        drafts,
        [
            /// Rename Draft
            RenameDraft,
        ]
    );
}

/// `kiln::init` with the menu table over the notes and drafts commands,
/// failing fast if `fail_fast`.
fn init_with_menus(table: kiln::MenuTable, fail_fast: bool, cx: &mut App) -> Vec<Diagnostic> {
    let (_sender, batches) = batch_channel();
    Contributions::set_menu_table(cx, table);
    kiln::init(
        cx,
        Config::new("com.example.Notes", "Notes")
            .commands(|cx| {
                notes::register(cx);
                drafts::register(cx);
            })
            .watch_with(batches)
            .fail_fast(fail_fast),
    )
}

/// What `init` panics with over the menu table, failing fast.
fn menus_panic(table: kiln::MenuTable, cx: &mut TestAppContext) -> String {
    let panicked = catch_unwind(AssertUnwindSafe(|| {
        cx.update(|cx| init_with_menus(table, true, cx));
    }));
    let payload = panicked.expect_err("init panics");
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => (*payload.downcast::<&str>().unwrap()).to_owned(),
    }
}

/// The errors among `diagnostics`, by key and message.
fn errors(diagnostics: &[Diagnostic]) -> Vec<(String, String)> {
    (diagnostics.iter())
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .map(|diagnostic| {
            let key = diagnostic.key.clone().unwrap_or_default();
            (key, diagnostic.message.clone())
        })
        .collect()
}

#[gpui::test]
fn a_menu_named_twice_is_an_error_and_the_table_left_out(cx: &mut TestAppContext) {
    let table = kiln::menus! {
        "File" => [notes::NewNote],
        "File" => [drafts::RenameDraft],
    };
    let diagnostics = cx.update(|cx| init_with_menus(table, false, cx));
    assert_eq!(
        errors(&diagnostics),
        [("File".to_owned(), "menu \"File\" is repeated".to_owned())]
    );
    // As a menu table with errors is: no menu shows, so no menu named
    // File hides the second's items.
    let names = cx.read(|cx| {
        let bar = menus::installed(cx).expect("the menus are installed");
        bar.menus
            .iter()
            .map(|menu| menu.name.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(names, Vec::<String>::new());
}

#[gpui::test]
fn a_command_twice_in_a_menu_is_an_error_and_the_table_left_out(cx: &mut TestAppContext) {
    let table = kiln::menus! { "File" => [notes::NewNote, Separator, notes::NewNote] };
    let diagnostics = cx.update(|cx| init_with_menus(table, false, cx));
    assert_eq!(
        errors(&diagnostics),
        [(
            "File.items[2]".to_owned(),
            "id \"notes.new_note\" is repeated in menu \"File\"".to_owned()
        )]
    );
    let installed = cx.read(|cx| menus::installed(cx).map(|bar| bar.menus.len()));
    assert_eq!(installed, Some(0));
}

#[gpui::test]
fn a_repeated_menu_fails_fast(cx: &mut TestAppContext) {
    let table = kiln::menus! {
        "File" => [notes::NewNote],
        "File" => [drafts::RenameDraft],
    };
    let message = menus_panic(table, cx);
    assert!(message.contains("menu \"File\" is repeated"), "{message}");
}

#[gpui::test]
fn a_repeated_item_fails_fast(cx: &mut TestAppContext) {
    let table = kiln::menus! { "File" => [notes::NewNote, notes::NewNote] };
    let message = menus_panic(table, cx);
    assert!(
        message.contains("id \"notes.new_note\" is repeated in menu \"File\""),
        "{message}"
    );
}
