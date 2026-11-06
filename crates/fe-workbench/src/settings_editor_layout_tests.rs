//! The Settings tab's layout: a heading right at the list's top and
//! shorter than a setting, the rows' column capped and centred, a
//! setting's control beside its title or under it, the category
//! navigation, and the hover shade inside the column.

use std::{fs, path::PathBuf};

use fe_config::{SettingKind, SettingsModel};
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, Pixels, TestAppContext, VisualTestContext, px, size,
};

use crate::{
    Launch, Workbench,
    open::ConfigDirOverride,
    settings_editor::{Row, Scope, SettingsEditor},
};

struct Dirs {
    _temp: tempfile::TempDir,
    root: PathBuf,
    config: PathBuf,
}

impl Dirs {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(temp.path()).unwrap();
        let (root, config) = (base.join("project"), base.join("config"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&config).unwrap();
        Self {
            _temp: temp,
            root,
            config,
        }
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn bounds(selector: &str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let found = cx.debug_bounds(selector.to_owned().leak());
    found.unwrap_or_else(|| panic!("{selector} is drawn"))
}

fn drawn(selector: &str, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(selector.to_owned().leak()).is_some()
}

/// A workbench on a folder, `width` wide, with the Settings tab open.
fn open_settings<'a>(
    cx: &'a mut TestAppContext,
    dirs: &Dirs,
    width: f32,
) -> (
    Entity<Workbench>,
    Entity<SettingsEditor>,
    &'a mut VisualTestContext,
) {
    let launch = Launch {
        root: Some(dirs.root.clone()),
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| {
        cx.set_global(ConfigDirOverride(Some(dirs.config.clone())));
        window.activate_window();
    });
    cx.simulate_resize(size(px(width), px(800.)));
    draw(cx);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.open_settings_ui(window, cx)
    });
    draw(cx);
    let editor = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let tab = &pane.tabs()[pane.active().unwrap()];
        tab.item().downcast::<SettingsEditor>().unwrap()
    });
    assert!(editor.read_with(cx, |editor, _| editor.model().is_some()));
    (workbench, editor, cx)
}

fn resize(width: f32, cx: &mut VisualTestContext) {
    cx.simulate_resize(size(px(width), px(800.)));
    draw(cx);
}

/// Shows what searching for `text` finds.
fn search(editor: &Entity<SettingsEditor>, text: &str, cx: &mut VisualTestContext) {
    let search = editor.read_with(cx, |editor, _| editor.search_input().clone());
    search.update(cx, |search, cx| search.clear(cx));
    cx.update(|window, cx| window.focus(&search.focus_handle(cx), cx));
    cx.simulate_input(text);
    draw(cx);
}

/// The dotted key of the first setting the rows show, and the title of
/// their first heading.
fn first_rows(editor: &Entity<SettingsEditor>, cx: &mut VisualTestContext) -> (String, String) {
    editor.read_with(cx, |editor, _| {
        let model = editor.model().unwrap();
        let setting = (editor.rows().iter()).find_map(|row| match row {
            Row::Setting(ix) => Some(model.entries()[*ix].key_path()),
            Row::Group(_) => None,
        });
        let heading = (editor.rows().iter()).find_map(|row| match row {
            Row::Group(title) => Some(title.clone()),
            Row::Setting(_) => None,
        });
        (setting.unwrap(), heading.unwrap())
    })
}

#[gpui::test]
fn first_heading_is_at_the_top(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs, 1200.);
    let (_, heading) = first_rows(&editor, cx);
    assert!(matches!(
        editor.read_with(cx, |editor, _| editor.rows()[0].clone()),
        Row::Group(_)
    ));
    let list = bounds("settings-list", cx);
    let heading = bounds(&format!("settings-group-{heading}"), cx);
    assert!(heading.top() >= list.top(), "{heading:?} in {list:?}");
    assert!(
        heading.top() - list.top() <= px(40.),
        "{heading:?} under {list:?}"
    );
}

#[gpui::test]
fn heading_is_shorter_than_a_setting(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs, 1200.);
    let (setting, heading) = first_rows(&editor, cx);
    let heading = bounds(&format!("settings-group-{heading}"), cx);
    let setting = bounds(&format!("settings-row-{setting}"), cx);
    assert!(heading.size.height < px(48.), "{heading:?}");
    assert!(setting.size.height > heading.size.height, "{setting:?}");
}

#[gpui::test]
fn content_column_is_capped_and_centred(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs, 1600.);
    let (setting, heading) = first_rows(&editor, cx);
    let (row, heading) = (
        format!("settings-row-{setting}"),
        format!("settings-group-{heading}"),
    );
    let list = bounds("settings-list", cx);
    let column = bounds(&row, cx);
    assert!(column.size.width <= px(760.), "{column:?}");
    assert!(list.size.width > px(1000.), "{list:?}");
    let off_centre = (column.center().x - list.center().x).abs();
    assert!(off_centre <= px(1.), "{column:?} centred in {list:?}");
    let title = bounds(&heading, cx);
    assert_eq!(
        (title.left(), title.right()),
        (column.left(), column.right()),
        "the heading's column is the row's"
    );

    // Narrow: no navigation, and the column fills the tab but its gutters.
    resize(500., cx);
    let tab = bounds("settings-editor", cx);
    let list = bounds("settings-list", cx);
    assert_eq!((list.left(), list.right()), (tab.left(), tab.right()));
    let column = bounds(&row, cx);
    assert_eq!(
        column.left(),
        list.left() + px(24.),
        "{column:?} in {list:?}"
    );
    assert_eq!(
        column.right(),
        list.right() - px(24.),
        "{column:?} in {list:?}"
    );
    let window = cx.update(|window, _| window.viewport_size());
    assert!(column.right() <= window.width, "no horizontal scroll");
}

/// The first setting of each of `kinds`, by dotted key.
fn keys_of_kinds(kinds: &[fn(&SettingKind) -> bool]) -> Vec<String> {
    let model = SettingsModel::new(None, None);
    (kinds.iter())
        .map(|kind| {
            let entry = model.entries().iter().find(|entry| kind(&entry.kind));
            entry.expect("a setting of the kind").key_path()
        })
        .collect()
}

#[gpui::test]
fn title_and_control_share_a_line(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs, 1000.);
    let keys = keys_of_kinds(&[
        |kind| *kind == SettingKind::Bool,
        |kind| matches!(kind, SettingKind::Enum(_)),
        |kind| matches!(kind, SettingKind::Int { .. }),
    ]);
    for key in &keys {
        search(&editor, key, cx);
        let title = bounds(&format!("settings-description-{key}"), cx);
        let control = bounds(&format!("settings-value-{key}"), cx);
        assert!(
            control.left() > title.left() + px(200.),
            "{key}: {control:?} right of {title:?}"
        );
        assert!(control.top() < title.bottom(), "{key}: on its line");
    }

    resize(480., cx);
    for key in &keys {
        search(&editor, key, cx);
        let title = bounds(&format!("settings-description-{key}"), cx);
        let control = bounds(&format!("settings-value-{key}"), cx);
        assert!(
            control.top() >= title.bottom(),
            "{key}: {control:?} under {title:?}"
        );
    }
}

#[gpui::test]
fn category_nav_scrolls_and_follows(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs, 1200.);
    let sections = editor.read_with(cx, |editor, _| editor.sections().to_vec());
    let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
    for title in ["Editor", "Chat", "Terminal"] {
        assert!(titles.contains(&title), "{titles:?}");
    }
    // Each top-level group once, with how many settings it has.
    let model = SettingsModel::new(None, None);
    for section in &sections {
        assert!(drawn(&format!("settings-nav-{}", section.title), cx));
        let top = model.groups().iter().find(|g| g.title == section.title);
        let top = top.unwrap_or_else(|| panic!("{} is a group", section.title));
        let count = (model.entries().iter())
            .filter(|entry| entry.key.len() > 1 && entry.key[0] == top.key[0])
            .count();
        assert_eq!(section.count, count, "{}", section.title);
    }
    let active = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.active_section());
    assert_eq!(active(cx), Some(0));

    // A click scrolls its heading to the top, and highlights it.
    let ix = 2;
    let section = &sections[ix];
    let entry = bounds(&format!("settings-nav-{}", section.title), cx);
    cx.simulate_click(entry.center(), Modifiers::none());
    draw(cx);
    let heading = editor.read_with(cx, |editor, _| match &editor.rows()[section.row] {
        Row::Group(title) => title.clone(),
        Row::Setting(_) => panic!("a section starts at its heading"),
    });
    let list = bounds("settings-list", cx);
    let top = bounds(&format!("settings-group-{heading}"), cx).top();
    assert_eq!(top, list.top(), "{heading} at the top");
    assert_eq!(active(cx), Some(ix));

    // The wheel, up a little: the section before is in view, highlighted.
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: list.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(30.))),
        ..Default::default()
    });
    draw(cx);
    assert_eq!(active(cx), Some(ix - 1));
    // Back to the top.
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: list.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(100_000.))),
        ..Default::default()
    });
    draw(cx);
    assert_eq!(active(cx), Some(0));

    // Searching, only the groups found in are listed.
    search(&editor, "minimap", cx);
    let sections = editor.read_with(cx, |editor, _| editor.sections().to_vec());
    let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, ["Editor"]);
    let found = model.search("minimap").len();
    assert_eq!(sections[0].count, found);
    assert!(drawn("settings-nav-Editor", cx));
    assert!(!drawn("settings-nav-Terminal", cx));
}

#[gpui::test]
fn hover_shade_stays_inside_the_column(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs, 1600.);
    let (setting, _) = first_rows(&editor, cx);
    let row = bounds(&format!("settings-row-{setting}"), cx);
    let shade = format!("settings-shade-{setting}");
    assert!(!drawn(&shade, cx));
    cx.simulate_mouse_move(row.center(), None, Modifiers::none());
    draw(cx);
    let shaded = bounds(&shade, cx);
    assert_eq!(shaded, row, "the shade is the column's");
    let list = bounds("settings-list", cx);
    assert!(shaded.size.width <= px(760.) && list.size.width > px(1000.));
}

/// A description's lines, wrapped where its doc comment wrapped, show as
/// one paragraph, its blank lines as breaks.
#[test]
fn descriptions_unwrap_their_doc_comments() {
    let unwrap = super::layout::unwrap;
    assert_eq!(
        unwrap("The most tokens\none message sends."),
        "The most tokens one message sends."
    );
    assert_eq!(unwrap("One.\n\nTwo\nlines."), "One.\nTwo lines.");
}

/// Whether `inner` lies across within `outer`.
fn across_within(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.left() >= outer.left() && inner.right() <= outer.right()
}

/// A modified setting with a long name and key, in the narrowest column
/// the navigation shows beside: its Reset to Default is in its title
/// block, hovered or not, and a click on it resets the setting.
#[gpui::test]
fn reset_shows_beside_a_long_title(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let key = "editor.cursor_surrounding_lines";
    let file = fe_config::project_settings_path(&dirs.root);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "[editor]\ncursor_surrounding_lines = 5\n").unwrap();
    let (_workbench, editor, cx) = open_settings(cx, &dirs, 1000.);
    editor.update(cx, |editor, cx| editor.set_scope(Scope::Workspace, cx));
    draw(cx);
    search(&editor, "cursor surrounding", cx);
    let tab = bounds("settings-editor", cx);
    assert!(drawn("settings-nav-Editor", cx));
    let nav = bounds("settings-nav", cx);
    assert!(
        nav.top() < tab.bottom(),
        "the navigation shows: {nav:?} in {tab:?}"
    );
    let reset = format!("setting-reset-{key}");
    let title = bounds(&format!("settings-description-{key}"), cx);
    let row = format!("settings-row-{key}");
    for hovered in [false, true] {
        if hovered {
            let at = bounds(&row, cx).center();
            cx.simulate_mouse_move(at, None, Modifiers::none());
            draw(cx);
        }
        let button = bounds(&reset, cx);
        assert!(
            across_within(button, title),
            "hovered {hovered}: {button:?} in {title:?}"
        );
    }
    let at = bounds(&reset, cx).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    let written = fs::read_to_string(&file).unwrap();
    assert!(!written.contains("cursor_surrounding_lines"), "{written}");
}

/// In a column narrower than a text input or a dropdown, either one under
/// its title is no wider than the column.
#[gpui::test]
fn controls_fit_a_narrow_column(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs, 500.);
    let keys = keys_of_kinds(&[
        |kind| matches!(kind, SettingKind::Text { .. }),
        |kind| matches!(kind, SettingKind::Choice(_)),
        |kind| matches!(kind, SettingKind::Enum(_)),
    ]);
    for key in &keys {
        search(&editor, key, cx);
        let row = bounds(&format!("settings-row-{key}"), cx);
        assert!(row.size.width < px(260.), "narrower than an input: {row:?}");
        for control in ["settings-value", "setting-input", "setting-select"] {
            let selector = format!("{control}-{key}");
            if drawn(&selector, cx) {
                let control = bounds(&selector, cx);
                assert!(
                    across_within(control, row),
                    "{selector}: {control:?} in {row:?}"
                );
            }
        }
    }
}

/// Each frame the app asks for, as the platform would deliver it, and
/// nothing more: a test window draws itself only when something changed
/// (GPUI draws a dirty one as its effects flush), so a frame no one asks
/// for isn't drawn.
fn frames(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    for _ in 0..10 {
        if cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
            return;
        }
        cx.run_until_parked();
    }
    panic!("the app asks for frames without end");
}

/// The title of the heading at row `row`.
fn heading_at(editor: &Entity<SettingsEditor>, row: usize, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| match &editor.rows()[row] {
        Row::Group(title) => title.clone(),
        Row::Setting(_) => panic!("a section starts at its heading"),
    })
}

/// A section too short to fill the list below its heading still comes to
/// the top when clicked, in the frames the app asks for, and is the one
/// highlighted: the last one, and one of a few the search finds. A
/// shorter window then keeps it there, its rows drawn and clickable.
#[gpui::test]
fn a_short_last_section_comes_to_the_top(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs, 1200.);
    for query in ["", "shell"] {
        if !query.is_empty() {
            search(&editor, query, cx);
        }
        let sections = editor.read_with(cx, |editor, _| editor.sections().to_vec());
        assert!(sections.len() > 1, "{query}: {sections:?}");
        let last = sections.len() - 1;
        let entry = bounds(&format!("settings-nav-{}", sections[last].title), cx);
        // Focus is in the list already: the click moves it nowhere, which
        // would draw another frame.
        editor.update_in(cx, |editor, window, cx| {
            window.focus(&editor.list_focus().clone(), cx)
        });
        draw(cx);
        cx.simulate_click(entry.center(), Modifiers::none());
        frames(cx);
        let heading = heading_at(&editor, sections[last].row, cx);
        let list = bounds("settings-list", cx);
        let top = bounds(&format!("settings-group-{heading}"), cx).top();
        assert_eq!(top, list.top(), "{query}: {heading} at the top");
        let active = editor.read_with(cx, |editor, _| editor.active_section());
        assert_eq!(active, Some(last), "{query}");
    }

    // A shorter window: the heading stays at the top, its rows drawn.
    cx.simulate_resize(size(px(1200.), px(320.)));
    frames(cx);
    let row = editor.read_with(cx, |editor, _| editor.sections().last().unwrap().row);
    let heading = heading_at(&editor, row, cx);
    let list = bounds("settings-list", cx);
    let top = bounds(&format!("settings-group-{heading}"), cx).top();
    assert_eq!(top, list.top(), "{heading} at the top");
    let key = editor.read_with(cx, |editor, _| match &editor.rows()[row + 1] {
        Row::Setting(ix) => editor.model().unwrap().entries()[*ix].key_path(),
        Row::Group(_) => unreachable!(),
    });
    let setting = bounds(&format!("settings-row-{key}"), cx);
    assert!(setting.top() < list.bottom(), "{setting:?} in {list:?}");
    let at = gpui::point(setting.center().x, setting.top() + px(10.));
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    assert_eq!(editor.read_with(cx, |e, _| e.selected()), Some(row + 1));

    // The space after the last row shrank with the window: at the end of
    // the list, the last row is in view, not blank space.
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: list.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-100_000.))),
        ..Default::default()
    });
    draw(cx);
    let key = editor.read_with(cx, |editor, _| match editor.rows().last() {
        Some(Row::Setting(ix)) => editor.model().unwrap().entries()[*ix].key_path(),
        _ => unreachable!(),
    });
    let end = bounds(&format!("settings-row-{key}"), cx);
    assert!(
        end.bottom() > list.top() && end.top() < list.bottom(),
        "{key}: {end:?} in {list:?}"
    );
}

/// The space after the last row follows the window: navigating to a
/// section in a tall window, then narrowing and shortening it, leaves
/// setting rows drawn at the end of the list, not blank space.
#[gpui::test]
fn the_spacer_follows_a_narrow_short_window(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs, 1200.);
    cx.simulate_resize(size(px(1200.), px(2400.)));
    frames(cx);
    let sections = editor.read_with(cx, |editor, _| editor.sections().to_vec());
    // The second to last: short enough that the spacer is needed.
    let section = &sections[sections.len() - 2];
    let entry = bounds(&format!("settings-nav-{}", section.title), cx);
    editor.update_in(cx, |editor, window, cx| {
        window.focus(&editor.list_focus().clone(), cx)
    });
    draw(cx);
    cx.simulate_click(entry.center(), Modifiers::none());
    frames(cx);

    cx.simulate_resize(size(px(360.), px(400.)));
    frames(cx);
    let list = bounds("settings-list", cx);
    // Wheel by wheel, as a user would, each in the frames it asks for.
    for _ in 0..30 {
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: list.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-100_000.))),
            ..Default::default()
        });
        frames(cx);
    }
    let key = editor.read_with(cx, |editor, _| match editor.rows().last() {
        Some(Row::Setting(ix)) => editor.model().unwrap().entries()[*ix].key_path(),
        _ => unreachable!(),
    });
    let end = bounds(&format!("settings-row-{key}"), cx);
    assert!(
        end.bottom() > list.top() && end.top() < list.bottom(),
        "{key}: {end:?} in {list:?}"
    );
}
