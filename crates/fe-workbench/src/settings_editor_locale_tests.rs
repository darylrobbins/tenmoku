//! The Settings tab follows a change of language while it's open (T569
//! F7): the Common section's heading, a setting's help, an open
//! dropdown's descriptions and a row's message for a value it refused
//! show in the fake locale, `qps-ploc`, once it's set, and in English
//! again after, with what was typed kept.

use std::{fs, path::Path};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{Editing, Row, SettingsEditor};
use crate::{Launch, Workbench, open::ConfigDirOverride};

/// The fake locale's text for what the test looks at: the Common heading,
/// the message for a number that isn't whole, and `editor.line_numbers`'s
/// help and its variants'.
static FAKE_LOCALE: kiln_l10n::Resources = kiln_l10n::Resources {
    crate_name: "fe-workbench-settings-fake-locale",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[kiln_l10n::ResourceFile {
        locale: "qps-ploc",
        path: "qps-ploc/settings-editor.ftl",
        source: "workbench-settings-common = ⟦Çómmóñ⟧\n\
                 workbench-settings-not-whole-number = ⟦ñót á whólé ñúmbér⟧\n\
                 settings-editor-line_numbers = ⟦Thé gúttér's líñé ñúmbérs⟧\n    \
                 .on = ⟦Éách rów's ñúmbér⟧\n    \
                 .off = ⟦Ñó ñúmbérs⟧\n    \
                 .relative = ⟦Rélátívé⟧\n    \
                 .hybrid = ⟦Hýbríd⟧\n",
    }],
    dynamic_prefixes: &[],
};

kiln_l10n::__private::inventory::submit! { kiln_l10n::Registered(&FAKE_LOCALE) }

const TAB_SIZE: &str = "editor.tab_size";
const LINE_NUMBERS: &str = "editor.line_numbers";

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn set_locale(tag: Option<&str>, cx: &mut VisualTestContext) {
    let tags: Vec<kiln_l10n::LanguageIdentifier> =
        tag.into_iter().map(|tag| tag.parse().unwrap()).collect();
    cx.update(|_, cx| kiln_platform::locale::set_locale(cx, &tags));
    draw(cx);
}

/// A workbench on a folder, its user settings in `config`, with the
/// Settings tab open: the tab, its files read.
fn settings<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    config: &Path,
) -> (Entity<SettingsEditor>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| {
        cx.set_global(ConfigDirOverride(Some(config.to_owned())));
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(cx);
    let run = RunCommand(
        Invocation::of("workbench.open_settings_ui")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let editor = tab(&workbench, cx);
    assert!(editor.read_with(cx, |editor, _| editor.model().is_some()));
    (editor, cx)
}

fn tab(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<SettingsEditor> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.panes(cx).iter())
            .find_map(|pane| {
                (pane.read(cx).tabs().iter())
                    .find_map(|tab| tab.item().downcast::<SettingsEditor>())
            })
            .expect("a Settings tab")
    })
}

/// Searches for `text` from the search field, in place of what's there.
fn search(editor: &Entity<SettingsEditor>, text: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let search = editor.read(cx).search_input().clone();
        search.update(cx, |search, cx| search.clear(cx));
        window.focus(&search.focus_handle(cx), cx);
    });
    cx.simulate_input(text);
    draw(cx);
}

/// The first row's heading, if it's one.
fn first_heading(editor: &Entity<SettingsEditor>, cx: &mut VisualTestContext) -> Option<String> {
    editor.read_with(cx, |editor, _| match editor.rows().first()? {
        Row::Group(title) => Some(title.clone()),
        Row::Setting(_) => None,
    })
}

/// `editor.line_numbers`'s help, and the open dropdown's descriptions.
fn line_numbers_text(
    editor: &Entity<SettingsEditor>,
    cx: &mut VisualTestContext,
) -> (String, Vec<String>) {
    editor.read_with(cx, |editor, _| {
        let entry = editor.entry(LINE_NUMBERS).expect("listed");
        let (key, options, _) = editor.open_select().expect("its dropdown is open");
        assert_eq!(key, LINE_NUMBERS);
        let descriptions = (options.iter().flatten())
            .map(|variant| variant.description.clone())
            .collect();
        (editor.help(entry), descriptions)
    })
}

#[gpui::test]
fn an_open_tab_follows_a_change_of_language(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(temp.path()).unwrap();
    let (root, config) = (base.join("project"), base.join("config"));
    fs::create_dir_all(root.join(".tenmoku")).unwrap();
    fs::create_dir_all(&config).unwrap();
    let (editor, cx) = settings(cx, &root, &config);
    assert_eq!(first_heading(&editor, cx).as_deref(), Some("Common"));

    // A value the setting refuses, its message under the input.
    search(&editor, "tab_size", cx);
    let input = editor
        .read_with(cx, |editor, _| {
            editor.input_of(&Editing::Field(TAB_SIZE.to_owned()))
        })
        .expect("the row's input is drawn");
    cx.update(|window, cx| window.focus(&input.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input("4.5");
    cx.simulate_keystrokes("enter");
    draw(cx);
    let field = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, cx| editor.field(TAB_SIZE, cx).unwrap())
    };
    assert_eq!(
        field(cx),
        ("4.5".to_owned(), Some("must be a whole number".to_owned()))
    );

    // A dropdown open, its variants described.
    search(&editor, "line_numbers", cx);
    cx.simulate_keystrokes("down enter");
    draw(cx);
    let (help, descriptions) = line_numbers_text(&editor, cx);
    assert!(help.starts_with("The gutter's line numbers"), "{help}");
    assert!(
        descriptions.contains(&"Each row's number.".to_owned()),
        "{descriptions:?}"
    );

    // In the fake locale: each as it says there, what was typed kept.
    set_locale(Some("qps-ploc"), cx);
    let (help, descriptions) = line_numbers_text(&editor, cx);
    assert_eq!(help, "⟦Thé gúttér's líñé ñúmbérs⟧");
    assert!(
        descriptions.contains(&"⟦Éách rów's ñúmbér⟧".to_owned()),
        "{descriptions:?}"
    );
    assert_eq!(
        field(cx),
        ("4.5".to_owned(), Some("⟦ñót á whólé ñúmbér⟧".to_owned()))
    );
    cx.simulate_keystrokes("escape");
    search(&editor, "", cx);
    assert_eq!(first_heading(&editor, cx).as_deref(), Some("⟦Çómmóñ⟧"));

    // And back.
    set_locale(None, cx);
    assert_eq!(first_heading(&editor, cx).as_deref(), Some("Common"));
    assert_eq!(
        field(cx),
        ("4.5".to_owned(), Some("must be a whole number".to_owned()))
    );
}
