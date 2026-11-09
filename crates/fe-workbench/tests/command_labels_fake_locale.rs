//! One `#[kiln::command]` command's title through every surface that shows
//! it (`docs/decisions/xte5e7hqsw-i18n-design.md` § 4, T1506 G-1): the
//! palette, a menu item, which-key, the keymaps picker and the Keyboard
//! Shortcuts editor show it in the locale set, through kiln-command's one
//! resolver, and follow a switch to a fake locale, `qps-ploc`, and back,
//! with nothing reopened but the keymaps picker.
//!
//! The catalogue's locale is the process's, so this is the only test in
//! its binary.

use std::cell::Cell;
use std::rc::Rc;

use fe_buffer::Buffer;
use fe_command::l10n::catalogue::{
    __private::inventory, LanguageIdentifier, Registered, ResourceFile, Resources,
};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{
    EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled, set_vim_remaps,
};
use fe_vim::{Remap, RemapMode, VimKey};
use fe_workbench::{Launch, Workbench};
use gpui::{App, Entity, Focusable as _, OwnedMenuItem, TestAppContext, VisualTestContext};
use kiln::{Config, Contributions, Cx, shell::Workspace};

/// Probe Labels
#[kiln::command(id = "workbench.probe_labels", context = "workbench", menu = "File")]
fn probe_labels(_: &mut Workspace, _: &mut Cx<Workspace>) {}

const ENGLISH: &str = "Probe Labels";
const FAKE: &str = "⟦Pröbe Läbels⟧";
const PROBE: &str = "workbench.probe_labels";

/// The probe's title in en-US and in the fake locale, for this binary.
static PROBE_LOCALES: Resources = Resources {
    crate_name: "command-labels-fake-locale",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[
        ResourceFile {
            locale: "en-US",
            path: "en-US/probe.ftl",
            source: "command-workbench-probe_labels = Probe Labels\n",
        },
        ResourceFile {
            locale: "qps-ploc",
            path: "qps-ploc/probe.ftl",
            source: "command-workbench-probe_labels = ⟦Pröbe Läbels⟧\n",
        },
    ],
    dynamic_prefixes: &[],
};

inventory::submit! { Registered(&PROBE_LOCALES) }

fn set_locale(tag: &str, cx: &mut VisualTestContext) {
    let tag: LanguageIdentifier = tag.parse().unwrap();
    cx.update(|_, cx| kiln::platform::locale::set_locale(cx, &[tag]));
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// The app as Tenmoku starts it, through kiln: the workbench's commands,
/// its keymap and menus, and this binary's `#[kiln::command]`, with vim on
/// and `<leader>p` mapped to the probe; then a workbench showing a file,
/// its editor focused.
fn app(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    for var in [
        "TENMOKU_CONFIG_DIR",
        "XDG_CONFIG_HOME",
        "HOME",
        "APPDATA",
        "USERPROFILE",
    ] {
        // SAFETY: no other thread is running yet to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    let (_sender, batches) = kiln::platform::config::batch_channel();
    let config = Config::new("dev.fastedit.test", "Tenmoku")
        .commands(|cx| {
            fe_gpui::init_with_keymap(cx, fe_keymap::Keymap::default());
            fe_workbench::init(cx);
        })
        .catalog(fe_workbench::command_catalog())
        .keymap(fe_keymap::DEFAULT_KEYMAP_PATH, fe_keymap::DEFAULT)
        .menus(fe_keymap::menu::DEFAULT_MENUS_PATH, fe_keymap::MENUS)
        .menu_shortcut_context(fe_keymap::MENU_SHORTCUT_CONTEXT)
        .watch_with(batches);
    cx.update(|cx| {
        Contributions::add_recorded(cx);
        kiln::init(cx, config);
        set_vim_enabled(cx, true);
        let leader = Remap {
            modes: vec![RemapMode::Normal],
            from: "<leader>p".into(),
            to: format!("<Cmd>fe:{PROBE}<CR>"),
            recursive: false,
        };
        assert_eq!(set_vim_remaps(cx, &[leader], " "), []);
    });
    let dir = tempfile::tempdir().unwrap();
    let launch = Launch {
        root: None,
        file: Some((
            dir.path().join("a.txt"),
            Some(Buffer::new("alpha\n").into()),
        )),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    draw(cx);
    focus_editor(&workbench, cx);
    (workbench, cx)
}

fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane().read(cx).active_editor().cloned()).expect("the file's editor")
    })
}

/// Shows the file's tab, its editor focused.
fn focus_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    let editor = editor(workbench, cx);
    cx.update(|window, cx| window.focus(&editor.read(cx).focus_handle(cx), cx));
    draw(cx);
}

/// Runs command `id` where focus is, in the workbench's context, as the
/// palette does.
fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// The palette's title for the probe, as it lists it when it opens.
fn palette_title(cx: &mut VisualTestContext) -> Option<String> {
    let stack = ContextStack::new().with(ContextFrame::new("workbench"));
    cx.update(|_, cx| {
        (kiln::shell::command_palette::commands_in(&stack, cx).into_iter())
            .find(|entry| entry.id.as_str() == PROBE)
            .map(|entry| entry.title.to_string())
    })
}

/// Every action item's name in the native menu bar, as the platform has it.
fn menu_items(cx: &App) -> Vec<String> {
    fn walk(items: &[OwnedMenuItem], out: &mut Vec<String>) {
        for item in items {
            match item {
                OwnedMenuItem::Action { name, .. } => out.push(name.clone()),
                OwnedMenuItem::Submenu(menu) => walk(&menu.items, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for menu in cx.get_menus().expect("menus installed") {
        walk(&menu.items, &mut out);
    }
    out
}

/// The which-key popup's title for `p` after `<leader>`, then the held
/// leader let go.
fn which_key_title(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    focus_editor(workbench, cx);
    let editor = editor(workbench, cx);
    cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.show_which_key_for(&[VimKey::Char(' ')], window, cx)
        })
    });
    draw(cx);
    let rows = editor.read_with(cx, |view, cx| view.which_key_rows(cx));
    cx.simulate_keystrokes("escape");
    draw(cx);
    let rows = rows.expect("the leader's popup");
    (rows.into_iter())
        .find(|(key, _)| key == "p")
        .map(|(_, title)| title)
}

/// Opens the keymaps picker from the file's editor, types `title` and
/// picks the row it finds: whether that ran the probe, its row titled
/// `title`, the `<Space>p` map's.
fn keymaps_picker_runs(
    workbench: &Entity<Workbench>,
    title: &str,
    ran: &Rc<Cell<usize>>,
    cx: &mut VisualTestContext,
) -> bool {
    focus_editor(workbench, cx);
    let before = ran.get();
    run("keymaps.search", cx);
    cx.simulate_input(title);
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    ran.get() > before
}

/// The open Keyboard Shortcuts tab's title for the probe.
fn shortcuts_title(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let rows = workbench.read_with(cx, |workbench, cx| workbench.shortcuts_editor_rows(cx));
    (rows
        .expect("the Keyboard Shortcuts tab is open")
        .into_iter())
    .find(|(command, _)| command == PROBE)
    .map(|(_, title)| title)
}

/// Checks that every surface shows the probe as `title`.
fn assert_everywhere(
    workbench: &Entity<Workbench>,
    title: &str,
    ran: &Rc<Cell<usize>>,
    cx: &mut VisualTestContext,
) {
    assert_eq!(palette_title(cx).as_deref(), Some(title), "the palette");
    let items = cx.update(|_, cx| menu_items(cx));
    assert!(
        items.iter().any(|item| item == title),
        "the menu bar: {items:?}"
    );
    assert_eq!(
        shortcuts_title(workbench, cx).as_deref(),
        Some(title),
        "the shortcuts editor"
    );
    assert!(
        keymaps_picker_runs(workbench, title, ran, cx),
        "the keymaps picker has no row titled {title}"
    );
    assert_eq!(
        which_key_title(workbench, cx).as_deref(),
        Some(title),
        "which-key"
    );
}

#[gpui::test]
fn macro_command_label_follows_the_locale_everywhere(cx: &mut TestAppContext) {
    // Whatever the system's languages, the test starts in en-US.
    cx.update(|cx| {
        kiln::platform::locale::set_locale(cx, &["en-US".parse().unwrap()]);
    });
    let (workbench, cx) = app(cx);
    let ran = Rc::new(Cell::new(0));
    let counted = ran.clone();
    cx.update(|_, cx| {
        cx.on_action(move |_: &ProbeLabels, _| counted.set(counted.get() + 1));
    });
    // Open from before the switch, in the background, and never reopened.
    run("workbench.open_keyboard_shortcuts", cx);
    focus_editor(&workbench, cx);

    assert_everywhere(&workbench, ENGLISH, &ran, cx);

    set_locale("qps-ploc", cx);
    assert_everywhere(&workbench, FAKE, &ran, cx);

    set_locale("en-US", cx);
    assert_everywhere(&workbench, ENGLISH, &ran, cx);
    assert_eq!(ran.get(), 3);
}
