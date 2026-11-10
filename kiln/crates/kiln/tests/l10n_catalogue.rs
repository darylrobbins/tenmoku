// SPDX-License-Identifier: Apache-2.0

//! kiln's catalogue root: every `.ftl` under `kiln/crates/*/l10n/`, against
//! every kiln crate's non-test source, read from disk (section 8 of
//! `docs/decisions/xte5e7hqsw-i18n-design.md`), so kiln keeps its catalogue
//! whole without the app's root. kiln's own commands and its `command-*`
//! keys match, Hide's title taking the app's name; and menu items show each command's label in the active
//! locale, both where `#[kiln::command(menu = ..)]` places them and from a
//! menu table, a command with no key showing its title. The process
//! catalogue is global: each test here that reads or switches it holds one
//! lock, and switches back when done.

use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

use gpui::{
    AnyView, App, AppContext as _, Context, IntoElement, Render, TestAppContext, Window, div,
};
use kiln::{
    Cx,
    command::ContextStack,
    platform::{command_actions::typed_commands, menus},
    shell::{Workspace, command_palette::commands_in},
    testing::TestApp,
};
use kiln_l10n::{LanguageIdentifier, Registered, ResourceFile, Resources, set_locales};

/// kiln's root check: no key unused (kiln's `## public` keys exempt, as
/// apps may be their only users), every reference resolving in its own
/// locale, every message formatting, no duplicates, no stale translation.
#[test]
fn kiln_catalogue_is_whole() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kiln/crates");
    let menus = include_str!("../src/setup/menus.toml");
    let title_keys = menus.lines().filter_map(|line| {
        let value = line.trim().strip_prefix("title_key")?.trim();
        let value = value.strip_prefix('=')?.trim().trim_matches('"');
        Some(value.to_owned())
    });
    kiln_l10n::lint::catalogue()
        .crates_under(crates)
        .data_keys(title_keys)
        .exempt_public()
        .check();
}

/// Every typed command kiln declares (`kiln::app`, `kiln::window`) has a
/// `command-<id>` title in kiln's catalogue, and every `command-*` key there
/// names one. Panel toggles have none of their own: their label is derived,
/// `kiln-command-toggle`.
#[gpui::test]
fn kiln_commands_and_command_keys_match(cx: &mut TestAppContext) {
    let _locale = locale_guard();
    let typed = cx.update(|cx| {
        kiln::app::register(cx);
        kiln::window::register(cx);
        typed_commands(cx)
    });
    let ids = typed.iter().map(|typed| typed.id.to_string());
    let errors = kiln_command::l10n::check_command_keys(&kiln::l10n::RESOURCES, ids);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

/// Hide's title takes the app's name, which kiln gives command titles when
/// it starts: the same English as the platform's registry title, in the
/// palette and in a menu table that names `kiln::app::Hide`.
#[gpui::test]
fn hide_is_titled_for_the_app(cx: &mut TestAppContext) {
    let _locale = locale_guard();
    let table = kiln::menus! { "File" => [kiln::app::Hide, ProbeLabels] };
    let mut app = TestApp::open(builder().menu_bar(table), cx);
    assert_eq!(file_menu(&mut app), ["Hide L10n Catalogue", "Probe Labels"]);
    let titles = app.cx().read(|cx| {
        commands_in(&ContextStack::new(), cx)
            .into_iter()
            .map(|entry| entry.title.to_string())
            .collect::<Vec<_>>()
    });
    assert!(
        titles.iter().any(|title| title == "Hide L10n Catalogue"),
        "{titles:?}"
    );
    assert!(!titles.iter().any(|title| title == "Hide"), "{titles:?}");
}

/// Probe Labels
#[kiln::command(menu = "File")]
fn probe_labels(_: &mut Workspace, _: &mut Cx<Workspace>) {}

/// Plain Probe
#[kiln::command(menu = "File")]
fn plain_probe(_: &mut Workspace, _: &mut Cx<Workspace>) {}

/// A fake locale, `qps-ploc`, for this binary: the probe and kiln's Close
/// Window get labels; Plain Probe, as a plugin's command, has none.
static FAKE_LOCALE: Resources = Resources {
    crate_name: "kiln-menu-fake-locale",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[ResourceFile {
        locale: "qps-ploc",
        path: "qps-ploc/menus.ftl",
        source: "command-l10n_catalogue-probe_labels = ⟦Pröbé Läbéls⟧\n\
                 command-window-close = ⟦Clösé Wïndöw⟧\n",
    }],
    dynamic_prefixes: &[],
};

inventory::submit! { Registered(&FAKE_LOCALE) }

struct Empty;

impl Render for Empty {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn builder() -> kiln::AppBuilder {
    kiln::app("dev.kiln.L10nCatalogue", "L10n Catalogue")
        .restore_layout(false)
        .root(|_, cx: &mut App| AnyView::from(cx.new(|_| Empty)))
}

/// The process catalogue is global and `cargo test` runs these tests as
/// threads of one process, so each one that reads or sets the locale holds
/// this for its whole body.
static LOCALE: Mutex<()> = Mutex::new(());

/// Holds `LOCALE`, and puts the default locale back on drop, even when an
/// assertion has failed.
struct LocaleGuard(#[expect(dead_code, reason = "held for its drop")] MutexGuard<'static, ()>);

fn locale_guard() -> LocaleGuard {
    LocaleGuard(
        LOCALE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

impl Drop for LocaleGuard {
    fn drop(&mut self) {
        set_locales(&[]);
    }
}

fn fake_locale() {
    let fake: Vec<LanguageIdentifier> = vec!["qps-ploc".parse().unwrap()];
    set_locales(&fake);
}

/// The File menu's command items, by title.
fn file_menu(app: &mut TestApp) -> Vec<String> {
    app.cx().read(|cx| {
        let bar = menus::installed(cx).expect("menus installed");
        let file = bar.menu("File").expect("a File menu");
        (file.items.iter())
            .filter_map(|item| item.command().map(|command| command.title.clone()))
            .collect()
    })
}

#[gpui::test]
fn placed_menu_items_show_labels(cx: &mut TestAppContext) {
    let _locale = locale_guard();
    fake_locale();
    let mut app = TestApp::open(builder(), cx);
    let items = file_menu(&mut app);
    assert!(
        items.iter().any(|item| item == "⟦Pröbé Läbéls⟧"),
        "{items:?}"
    );
    assert!(items.iter().any(|item| item == "Plain Probe"), "{items:?}");
    assert!(
        !items.iter().any(|item| item == "Probe Labels"),
        "{items:?}"
    );
}

#[gpui::test]
fn menu_table_items_show_labels(cx: &mut TestAppContext) {
    let _locale = locale_guard();
    fake_locale();
    let table = kiln::menus! { "File" => [ProbeLabels, PlainProbe, kiln::window::Close] };
    let mut app = TestApp::open(builder().menu_bar(table), cx);
    let items = file_menu(&mut app);
    assert_eq!(items, ["⟦Pröbé Läbéls⟧", "Plain Probe", "⟦Clösé Wïndöw⟧"]);
}
