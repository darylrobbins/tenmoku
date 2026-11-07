//! The which-key popup's row icons ([`super::which_key_icons`]): every icon
//! the tables name is bundled, every command they name exists, each icon
//! is tinted with one of the theme's semantic colours, and a row without an
//! icon keeps its room so the titles line up.

use std::time::Duration;

use fe_buffer::Buffer;
use fe_command::CommandId;
use fe_config::theme::EditorPalette;
use fe_editor::EditorModel;
use fe_keymap::CatalogError;
use fe_vim::{Remap, RemapMode, WhichKey, WhichKeyEntry, WhichKeyGroupName, WhichKeyLabel};
use gpui::{Bounds, Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext};
use serde_json::Value;

use super::which_key_icons::{
    COMMAND_ICONS, ICON_COLOURS, NAMESPACE_ICONS, bundled, command_icon, entry_icon, icon_colour,
};
use super::which_key_layout::entry_colors;
use super::*;
use crate::{EditorTheme, EditorView, init};

const DELAY: Duration = Duration::from_millis(200);

/// A command of each of [`NAMESPACE_ICONS`]'s namespaces that
/// [`COMMAND_ICONS`] doesn't list, so draws its namespace's icon.
const NAMESPACE_EXAMPLES: &[&str] = &[
    "git.push",
    "lsp.restart",
    "terminal.new",
    "tasks.run",
    "chat.new",
    "ai.show_changes",
];

#[test]
fn every_table_icon_is_a_bundled_icon() {
    let named = (COMMAND_ICONS.iter().chain(NAMESPACE_ICONS)).map(|&(_, icon)| icon);
    for icon in named.chain(ICON_COLOURS.iter().map(|&(icon, _)| icon)) {
        assert!(bundled(icon).is_some(), "{icon} isn't a bundled icon");
    }
    // Each icon a command draws has its colour listed, not the fallback's.
    for &(command, icon) in COMMAND_ICONS.iter().chain(NAMESPACE_ICONS) {
        assert!(
            ICON_COLOURS.iter().any(|&(listed, _)| listed == icon),
            "{command}'s {icon} has no colour listed"
        );
    }
    // No icon or command is listed twice.
    let mut icons: Vec<_> = ICON_COLOURS.iter().map(|&(icon, _)| icon).collect();
    icons.sort_unstable();
    icons.dedup();
    assert_eq!(
        icons.len(),
        ICON_COLOURS.len(),
        "an icon's colour is listed twice"
    );
    let mut commands: Vec<_> = COMMAND_ICONS.iter().map(|&(id, _)| id).collect();
    commands.sort_unstable();
    commands.dedup();
    assert_eq!(
        commands.len(),
        COMMAND_ICONS.len(),
        "a command is listed twice"
    );
    // The icons LazyVim's groups draw, for `:fegroup`'s `icon=`.
    for icon in [
        "bug",
        "save",
        "terminal",
        "settings",
        "undo",
        "panels-top-left",
    ] {
        assert!(bundled(icon).is_some(), "{icon}");
    }
    for icon in ["play", "flask-conical", "bot", "log-out", "toggle-right"] {
        assert!(bundled(icon).is_some(), "{icon}");
    }
    assert_eq!(bundled("no-such-icon"), None);
}

#[test]
fn every_table_command_is_a_catalog_id() {
    let catalog = fe_workbench::command_catalog();
    let known = |id: &str| {
        let parsed = CommandId::parse(id).unwrap_or_else(|_| panic!("{id} isn't an ID"));
        !matches!(
            catalog.check(&parsed, &Value::Null),
            Err(CatalogError::UnknownCommand)
        )
    };
    assert!(!known("workbench.no_such_command"), "the catalog tells");
    let unknown: Vec<_> = (COMMAND_ICONS.iter())
        .map(|&(id, _)| id)
        .filter(|id| !known(id))
        .collect();
    assert_eq!(unknown, [""; 0], "not in the host's catalog");
    for &(id, icon) in COMMAND_ICONS {
        assert_eq!(command_icon(id), Some(icon), "{id}");
    }
    // Each namespace has commands, which draw its icon.
    for &(namespace, icon) in NAMESPACE_ICONS {
        let example = (NAMESPACE_EXAMPLES.iter())
            .find(|example| example.split_once('.').map(|(ns, _)| ns) == Some(namespace))
            .unwrap_or_else(|| panic!("no example of {namespace}"));
        assert!(known(example), "{example} isn't in the host's catalog");
        assert_eq!(command_icon(example), Some(icon), "{example}");
    }
    // Commands of neither draw none.
    for id in [
        "editor.select_all",
        "editor.undo",
        "workbench.harpoon_jump1",
    ] {
        assert!(known(id), "{id}");
        assert_eq!(command_icon(id), None, "{id}");
    }
    // A namespace alone, or a longer one sharing its start, isn't a command.
    assert_eq!(command_icon("git"), None);
    assert_eq!(command_icon("gitx.push"), None);
}

/// A focused editor in a window of its own, one column wide, vim on with
/// `remaps` (`,` the leader) and `groups`.
fn vim_editor<'a>(
    cx: &'a mut TestAppContext,
    remaps: &[(&str, &str)],
    groups: Vec<WhichKeyGroupName>,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new("one\ntwo\nthree"), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.update(|_, cx| {
        set_vim_enabled(cx, true);
        set_vim_which_key(cx, true, DELAY);
    });
    let remaps: Vec<Remap> = (remaps.iter())
        .map(|&(from, to)| noremap(from, to))
        .collect();
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &remaps, ","));
    assert_eq!(issues, []);
    cx.update(|_, cx| set_vim_which_key_groups(cx, groups));
    // One column: each key on a row of its own.
    cx.simulate_resize(gpui::size(gpui::px(320.), gpui::px(480.)));
    cx.run_until_parked();
    (view, cx)
}

fn noremap(from: &str, to: &str) -> Remap {
    Remap {
        modes: vec![RemapMode::Normal],
        from: from.to_string(),
        to: to.to_string(),
        recursive: false,
    }
}

/// Holds `,` past the delay and draws the popup it shows.
fn leader_popup(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> WhichKey {
    cx.simulate_keystrokes(",");
    cx.executor().advance_clock(DELAY);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| view.which_key_shown().cloned())
        .expect("the leader's popup")
}

fn bounds(selector: String, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
}

fn icon_of<'a>(which: &'a WhichKey, key: &str) -> &'a WhichKeyEntry {
    (which.entries.iter())
        .find(|entry| entry.key == key)
        .unwrap_or_else(|| panic!("{key} listed"))
}

/// Rows with an icon (a command's, a group's), without one (a command
/// with none, a map's keys, an unnamed group, an icon not bundled) all
/// start their titles at one x, past an icon box 1.2em wide; a popup with
/// no icon at all keeps no room for one.
#[gpui::test]
fn a_row_without_an_icon_keeps_its_title_aligned(cx: &mut TestAppContext) {
    let groups = [
        WhichKeyGroupName::parse_with_icon(&[RemapMode::Normal], ",d", "debug", Some("bug"), ","),
        WhichKeyGroupName::parse_with_icon(
            &[RemapMode::Normal],
            ",r",
            "rockets",
            Some("rocket"),
            ",",
        ),
    ];
    let groups = groups.into_iter().flat_map(Result::unwrap).collect();
    let (view, cx) = vim_editor(
        cx,
        &[
            ("<leader>a", "<Cmd>fe:search.show<CR>"),
            ("<leader>b", "<Cmd>fe:editor.select_all<CR>"),
            ("<leader>c", "x"),
            ("<leader>dd", "x"),
            ("<leader>ee", "x"),
            ("<leader>rr", "x"),
        ],
        groups,
    );
    let which = leader_popup(&view, cx);
    let keys: Vec<_> = which
        .entries
        .iter()
        .map(|entry| entry.key.as_str())
        .collect();
    assert_eq!(keys, ["a", "b", "c", "d", "e", "r"]);
    assert_eq!(icon_of(&which, "a").icon.as_deref(), Some("search"));
    assert_eq!(icon_of(&which, "b").icon, None);
    assert_eq!(icon_of(&which, "c").icon, None);
    assert_eq!(icon_of(&which, "d").icon.as_deref(), Some("bug"));
    assert_eq!(icon_of(&which, "e").icon, None);
    assert_eq!(icon_of(&which, "r").icon.as_deref(), Some("rocket"));
    assert!(matches!(
        icon_of(&which, "d").label,
        WhichKeyLabel::Group { .. }
    ));

    let font_size = view.read_with(cx, |view, _| view.theme.font_size);
    let mut lefts = Vec::new();
    for key in keys {
        let arrow = bounds(format!("which-key-arrow-{key}"), cx).expect("an arrow");
        let icon = bounds(format!("which-key-icon-{key}"), cx)
            .unwrap_or_else(|| panic!("{key} keeps an icon's room"));
        let title = bounds(format!("which-key-title-{key}"), cx).expect("a title");
        assert!(arrow.right() <= icon.left(), "{key}: {arrow:?} {icon:?}");
        assert!(icon.right() <= title.left(), "{key}: {icon:?} {title:?}");
        assert!(
            // Laid out at whole pixels.
            (icon.size.width - font_size * 1.2).abs() <= gpui::px(0.5),
            "{key}: {icon:?}, 1.2em of {font_size:?}"
        );
        lefts.push(title.left());
    }
    assert!(lefts.iter().all(|left| *left == lefts[0]), "{lefts:?}");

    // No icon in the list: no room kept, the title right after the arrow,
    // as before icons.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.which_key_shown().cloned()),
        None
    );
    let plain = [
        noremap("<leader>b", "<Cmd>fe:editor.select_all<CR>"),
        noremap("<leader>c", "x"),
    ];
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &plain, ","));
    assert_eq!(issues, []);
    cx.update(|_, cx| set_vim_which_key_groups(cx, Vec::new()));
    cx.run_until_parked();
    let which = leader_popup(&view, cx);
    assert!(which.entries.iter().all(|entry| entry.icon.is_none()));
    for key in ["b", "c"] {
        assert_eq!(bounds(format!("which-key-icon-{key}"), cx), None, "{key}");
        let arrow = bounds(format!("which-key-arrow-{key}"), cx).expect("an arrow");
        let title = bounds(format!("which-key-title-{key}"), cx).expect("a title");
        assert!(
            title.left() - arrow.right() < font_size,
            "{arrow:?} {title:?}"
        );
    }
}

/// Every icon is tinted with one of the theme's semantic colours, by its
/// table colour, in every bundled theme; a group's title shares its icon's
/// colour, an item's stays the text's.
#[gpui::test]
fn icons_use_theme_colours_only(cx: &mut TestAppContext) {
    cx.update(init);
    let themes: Vec<EditorTheme> = cx.update(|cx| {
        let palettes = [
            EditorPalette::fastedit_dark(),
            EditorPalette::fastedit_light(),
        ];
        let mut themes = vec![EditorTheme::default_dark(cx)];
        for palette in palettes {
            themes.push(EditorTheme::from_palette(palette, &Default::default(), cx));
        }
        themes
    });
    let mut unlisted = (fe_ui::Icon::ALL.iter())
        .map(|icon| icon.name())
        .filter(|name| ICON_COLOURS.iter().all(|&(listed, _)| listed != *name));
    let unlisted = unlisted.next().expect("an icon with no colour listed");
    let names = (ICON_COLOURS.iter().map(|&(name, _)| name)).chain([unlisted]);
    for theme in &themes {
        let semantic = [
            theme.information,
            theme.warning,
            theme.error,
            theme.hint,
            theme.change_added,
        ];
        for name in names.clone() {
            let entry = |group: bool| WhichKeyEntry {
                key: "x".into(),
                label: match group {
                    true => WhichKeyLabel::Group {
                        count: 1,
                        name: Some("group".into()),
                    },
                    false => WhichKeyLabel::Command("search.show".into()),
                },
                group,
                icon: Some(name.into()),
            };
            let (icon, colour) = entry_icon(&entry(false), theme).expect("bundled");
            assert_eq!(icon.name(), name);
            assert!(semantic.contains(&colour), "{name}: {colour:?}");
            assert_eq!(colour, icon_colour(name).of(theme), "{name}");
            assert_eq!(entry_colors(&entry(true), theme).title, colour, "{name}");
            assert_eq!(entry_colors(&entry(false), theme).title, theme.foreground);
        }
        assert_eq!(icon_colour(unlisted).of(theme), theme.information);
    }
    // The table uses more than one colour: icons tell their kinds apart.
    let mut colours: Vec<_> = ICON_COLOURS.iter().map(|&(_, colour)| colour).collect();
    colours.dedup();
    assert!(colours.len() > 1);
}
