//! The sidebar in B3 (`docs/design/design-system.md`, Workbench anatomy):
//! one heading per tool window, the dock's, ending in the pane controls
//! (the view's own actions, then ⋯, then —); a lone view, or the view the
//! heading names, without a header of its own; the project tree's root row
//! in the folder's own case, 24 px rows, Git status in the `vcs.*` colours
//! with a folder's 6 px dot in the letter column; and Structure, the
//! outline, collapsed at the foot of Project, its header still showing the
//! symbol under the caret.

use std::{fs, path::Path};

use fe_buffer::Buffer;
use fe_git::{ChangeKind, FileStatus};
use fe_ui::{ActiveTheme as _, ROW_HEIGHT, Theme, ThemeData};
use gpui::{
    AppContext as _, Bounds, Context, Entity, Focusable as _, IntoElement, Modifiers, Pixels,
    Render, TestAppContext, VisualTestContext, Window, div, px,
};
use kiln::core::theme::ThemeFile;
use kiln_shell::{
    DockPosition, DockView as _, StackedView, ViewId, ViewStack,
    dock::{HIDE_BUTTON, MIN_DOCK_WIDTH, MORE_BUTTON},
};

use super::open;
use crate::{Dock, Launch, Workbench, outline_panel::VIEW as STRUCTURE, project_panel::FOLDERS};

/// Yohen, Tenmoku's dark theme, whose accent (hare's fur) and
/// `vcs.modified` (ruri) differ, unlike kiln's own dark theme's.
fn use_yohen(cx: &mut VisualTestContext) {
    let source = include_str!("../../../fe-config/themes/yohen.toml");
    let (file, _) = ThemeFile::parse(Path::new("yohen.toml"), source, &[]);
    let data = ThemeData::from_file(&file.expect("Yohen parses"));
    cx.update(|_, cx| fe_ui::set_theme(cx, Theme::new(data)));
    draw(cx);
}

/// A workbench on `root`, a folder named in mixed case holding `a.rs` and
/// `src/b.rs`, showing `a.rs` (`text`), focused as the app focuses it.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    text: &str,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("a.rs"), text).unwrap();
    fs::write(root.join("src/b.rs"), "fn b() {}\n").unwrap();
    let launch = Launch {
        root: Some(root.to_owned()),
        file: Some((root.join("a.rs"), Some(Buffer::new(text).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
}

/// A folder named `MixedCase` in a temporary one.
fn mixed_case() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("MixedCase");
    fs::create_dir(&root).unwrap();
    (dir, root)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn dock(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Dock> {
    workbench.read_with(cx, |workbench, _| workbench.dock().clone())
}

fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    (cx.debug_bounds(selector)).unwrap_or_else(|| panic!("{selector} drawn"))
}

fn button(id: &str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    bounds(format!("icon-button:{id}").leak(), cx)
}

#[gpui::test]
fn one_heading_above_the_first_tree_row_ending_in_the_pane_controls(cx: &mut TestAppContext) {
    let (_dir, root) = mixed_case();
    let (workbench, cx) = opened(cx, &root, "fn a() {}\n");
    let dock = dock(&workbench, cx);
    let (title, heading, controls) = dock.read_with(cx, |dock, cx| {
        let drawn = dock.state().drawn();
        let title = dock.state().title(cx).unwrap_or_default().to_string();
        (
            title,
            drawn.title_bounds().expect("drawn"),
            drawn.controls(),
        )
    });
    assert_eq!(title, "Project");
    assert_eq!(heading.size.height, fe_ui::TAB_HEIGHT);

    // The tree is the view the heading names: no header of its own, its
    // root row right under the heading.
    let (headless, header) = workbench.read_with(cx, |workbench, cx| {
        let stack = workbench
            .dock()
            .read(cx)
            .explorer()
            .read(cx)
            .stack()
            .read(cx);
        (stack.is_headless(FOLDERS), stack.header_bounds(FOLDERS))
    });
    assert_eq!((headless, header), (Some(true), None));
    let root_row = bounds("project-root", cx);
    assert_eq!(root_row.top(), heading.bottom(), "{root_row:?} {heading:?}");

    // The view's actions, `+` first, then ⋯, named, then —, right-aligned
    // in that order.
    let ids: Vec<_> = controls.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        ids,
        [
            crate::project_panel::NEW_BUTTON,
            "project-reveal",
            "project-collapse",
            MORE_BUTTON,
            HIDE_BUTTON
        ]
    );
    let names: Vec<_> = controls.iter().map(|(_, name)| name.to_string()).collect();
    assert_eq!(
        names,
        [
            "New…",
            "Reveal Active File in Project",
            "Collapse All Folders",
            "More actions",
            "Hide",
        ]
    );
    let drawn: Vec<_> = ids.iter().map(|id| button(id, cx)).collect();
    for pair in drawn.windows(2) {
        assert!(pair[0].right() <= pair[1].left(), "{drawn:?}");
    }
    for button in &drawn {
        assert!(
            heading.contains(&button.center()),
            "{button:?} in {heading:?}"
        );
    }
    let last = drawn.last().unwrap();
    assert!(
        heading.right() - last.right() <= px(8.),
        "{last:?} {heading:?}"
    );
}

#[gpui::test]
fn more_opens_the_docks_menu_and_hide_hides_it(cx: &mut TestAppContext) {
    let (_dir, root) = mixed_case();
    let (workbench, cx) = opened(cx, &root, "fn a() {}\n");
    let dock = dock(&workbench, cx);
    let position =
        |cx: &mut VisualTestContext| dock.read_with(cx, |dock, _| dock.state().position());
    assert_eq!(position(cx), DockPosition::Left);

    // ⋯ opens the menu; its first item moves the sidebar to the other side.
    let more = button(MORE_BUTTON, cx);
    cx.simulate_click(more.center(), Modifiers::none());
    draw(cx);
    let titles = dock.read_with(cx, |dock, _| {
        let state = dock.state();
        assert!(state.is_menu_open());
        let menu = state.menu().iter();
        menu.map(|item| item.title.to_string()).collect::<Vec<_>>()
    });
    assert_eq!(titles, ["Move Sidebar to Right"]);
    let menu = bounds("dock-menu", cx);
    let first = menu.origin + gpui::point(px(24.), px(12.));
    cx.simulate_click(first, Modifiers::none());
    draw(cx);
    assert_eq!(position(cx), DockPosition::Right);
    assert!(!dock.read_with(cx, |dock, _| dock.state().is_menu_open()));
    let titles = dock.read_with(cx, |dock, _| {
        let menu = dock.state().menu().iter();
        menu.map(|item| item.title.to_string()).collect::<Vec<_>>()
    });
    assert_eq!(titles[0], "Move Sidebar to Left");

    // — hides the dock, as dock.toggle does, focus leaving what it hid.
    let tree = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    cx.update(|window, cx| window.focus(&tree.focus_handle(cx), cx));
    let hide = button(HIDE_BUTTON, cx);
    cx.simulate_click(hide.center(), Modifiers::none());
    draw(cx);
    assert!(!dock.read_with(cx, |dock, _| dock.is_visible()));
    assert!(!cx.update(|window, cx| tree.focus_handle(cx).contains_focused(window, cx)));
}

/// A view's body: nothing.
struct Body;

impl Render for Body {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui::test]
fn a_lone_view_has_no_header(cx: &mut TestAppContext) {
    const ONLY: ViewId = ViewId("only");
    const OTHER: ViewId = ViewId("other");
    let (stack, cx) = cx.add_window_view(|_, cx| {
        let mut stack = ViewStack::new();
        stack.add(StackedView::new(ONLY, "Only", cx.new(|_| Body)), cx);
        stack
    });
    draw(cx);
    let (headless, header, body, column) = stack.read_with(cx, |stack, _| {
        let body = stack.body_bounds(ONLY);
        (
            stack.is_headless(ONLY),
            stack.header_bounds(ONLY),
            body,
            stack.bounds(),
        )
    });
    assert_eq!((headless, header), (Some(true), None));
    // Its body fills the column, and it doesn't collapse.
    assert_eq!(body, Some(column));
    assert!(!stack.update(cx, |stack, cx| stack.toggle(ONLY, cx)));
    assert_eq!(
        stack.read_with(cx, |stack, _| stack.is_collapsed(ONLY)),
        Some(false)
    );

    // With a second view, both have headers.
    stack.update(cx, |stack, cx| {
        stack.add(StackedView::new(OTHER, "Other", cx.new(|_| Body)), cx)
    });
    draw(cx);
    let headers = stack.read_with(cx, |stack, _| {
        [ONLY, OTHER].map(|view| stack.header_bounds(view).is_some())
    });
    assert_eq!(headers, [true, true]);
}

#[gpui::test]
fn the_root_row_is_the_folder_in_its_own_case_and_rows_are_24_px(cx: &mut TestAppContext) {
    let (_dir, root) = mixed_case();
    let (workbench, cx) = opened(cx, &root, "fn a() {}\n");
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    let drawn = panel.read_with(cx, |panel, _| panel.drawn_root());
    assert_eq!(drawn.as_deref(), Some("MixedCase"));
    assert_eq!(ROW_HEIGHT, px(24.));
    assert_eq!(bounds("project-root", cx).size.height, ROW_HEIGHT);
    let row = panel.read_with(cx, |panel, _| panel.row_bounds(0).expect("drawn"));
    assert_eq!(row.size.height, px(24.));
    // The first tree row follows the root's.
    assert_eq!(row.top(), bounds("project-root", cx).bottom());
}

/// Lists `statuses`, by path from `root`, in the Changes panel, then draws.
fn set_statuses(
    workbench: &Entity<Workbench>,
    root: &Path,
    statuses: &[(&str, ChangeKind)],
    cx: &mut VisualTestContext,
) {
    let statuses = (statuses.iter())
        .map(|(path, kind)| FileStatus {
            path: root.join(path),
            kind: *kind,
        })
        .collect();
    let changes = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).changes().clone()
    });
    changes.update(cx, |changes, cx| changes.set_statuses(statuses, cx));
    draw(cx);
}

#[gpui::test]
fn a_modified_files_letter_is_vcs_modified_and_a_folders_dot_is_in_the_letter_column(
    cx: &mut TestAppContext,
) {
    let (_dir, root) = mixed_case();
    let (workbench, cx) = opened(cx, &root, "fn a() {}\n");
    use_yohen(cx);
    let modified = [
        ("a.rs", ChangeKind::Modified),
        ("src/b.rs", ChangeKind::Modified),
    ];
    set_statuses(&workbench, &root, &modified, cx);
    let (vcs, accent) = cx.update(|_, cx| {
        let colors = &cx.theme().colors;
        (colors.vcs.modified, colors.accent.base)
    });
    assert_ne!(vcs, accent, "Yohen's ruri and hare's fur");
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    let drawn = panel.read_with(cx, |panel, _| panel.drawn_status());
    let status = |name: &str| {
        let row = drawn.iter().rev().find(|(drawn, _)| drawn == name);
        row.unwrap_or_else(|| panic!("{name} drawn")).1.clone()
    };
    assert_eq!(status("a.rs"), (Some(vcs), Some(("M".to_owned(), vcs))));
    assert_eq!(status("src"), (Some(vcs), Some(("•".to_owned(), vcs))));

    // The folder's mark is a 6 px dot, centred where a file's letter is.
    let dot = bounds("tree-dot:src", cx);
    assert_eq!(dot.size, gpui::size(px(6.), px(6.)));
    assert_eq!(dot.size.width, fe_ui::DOT_SIZE);
    let letter = bounds("tree-badge:a.rs", cx);
    assert_eq!(letter.size.width, fe_ui::BADGE_COLUMN);
    assert!(
        (dot.center().x - letter.center().x).abs() < px(0.5),
        "{dot:?} {letter:?}"
    );
    assert!(cx.debug_bounds("tree-badge:src").is_none());
}

/// The symbol Structure's header shows, as last drawn.
fn caret_symbol(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let symbol = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).caret_symbol().clone()
    });
    symbol.read_with(cx, |symbol, _| symbol.drawn())
}

/// Draws until the outline has landed and Structure's header shows a
/// symbol, as the editor parses as it renders.
fn draw_until_symbol(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    for _ in 0..20 {
        draw(cx);
        if caret_symbol(workbench, cx).is_some() {
            return;
        }
    }
}

/// Puts the active editor's caret at `offset`.
fn put_caret(workbench: &Entity<Workbench>, offset: usize, cx: &mut VisualTestContext) {
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("a file is open");
    editor.update(cx, |editor, cx| {
        editor.select_and_reveal(offset..offset, cx)
    });
    draw(cx);
}

#[gpui::test]
fn structure_shows_the_caret_symbol_when_collapsed(cx: &mut TestAppContext) {
    let (_dir, root) = mixed_case();
    let text = "fn alpha() {}\n\nfn beta() {}\n";
    let (workbench, cx) = opened(cx, &root, text);
    let stack = workbench.read_with(cx, |workbench, cx| {
        workbench
            .dock()
            .read(cx)
            .explorer()
            .read(cx)
            .stack()
            .clone()
    });
    let (views, collapsed) = stack.read_with(cx, |stack, _| {
        (stack.view_ids(), stack.is_collapsed(STRUCTURE))
    });
    // Structure, the outline, at the foot of Project, collapsed.
    assert_eq!(views, [FOLDERS, STRUCTURE]);
    assert_eq!(collapsed, Some(true));
    let header = stack.read_with(cx, |stack, _| {
        stack.header_bounds(STRUCTURE).expect("drawn")
    });
    let column = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).column_bounds().expect("drawn")
    });
    assert_eq!(header.bottom(), column.bottom(), "at the foot");
    let title = workbench.read_with(cx, |workbench, cx| {
        let explorer = workbench.dock().read(cx).explorer().read(cx);
        explorer.stack().read(cx).is_headless(STRUCTURE)
    });
    assert_eq!(title, Some(false));
    assert_eq!(kiln_l10n::message("workbench-dock-structure"), "Structure");

    // The caret in alpha: its header shows alpha, after its title.
    draw_until_symbol(&workbench, cx);
    assert_eq!(caret_symbol(&workbench, cx).as_deref(), Some("alpha"));
    let symbol = bounds("structure-caret-symbol", cx);
    assert!(
        header.contains(&symbol.center()),
        "{symbol:?} in {header:?}"
    );

    // Moved into beta, it follows.
    put_caret(&workbench, text.find("beta").unwrap(), cx);
    assert_eq!(caret_symbol(&workbench, cx).as_deref(), Some("beta"));

    // Expanded, the outline shows it, and the header nothing more.
    cx.simulate_click(header.center(), Modifiers::none());
    draw(cx);
    assert_eq!(
        stack.read_with(cx, |stack, _| stack.is_collapsed(STRUCTURE)),
        Some(false)
    );
    assert!(cx.debug_bounds("structure-caret-symbol").is_none());
}

#[gpui::test]
fn structure_shows_nothing_outside_a_symbol(cx: &mut TestAppContext) {
    let (_dir, root) = mixed_case();
    let text = "fn alpha() {}\n\n\n";
    let (workbench, cx) = opened(cx, &root, text);
    draw_until_symbol(&workbench, cx);
    assert_eq!(caret_symbol(&workbench, cx).as_deref(), Some("alpha"));
    put_caret(&workbench, text.len(), cx);
    assert_eq!(caret_symbol(&workbench, cx), None);
}

/// Inserts `text` at `offset` as typing does, the caret after it, then
/// draws.
fn type_at(workbench: &Entity<Workbench>, offset: usize, text: &str, cx: &mut VisualTestContext) {
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    editor.expect("a file is open").update(cx, |editor, cx| {
        editor.model_mut().set_cursor(offset);
        editor.model_mut().insert(text);
        cx.notify();
    });
    draw(cx);
}

/// Deletes the text in `range`, the caret where it was, then draws.
fn delete(
    workbench: &Entity<Workbench>,
    range: std::ops::Range<usize>,
    cx: &mut VisualTestContext,
) {
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    editor.expect("a file is open").update(cx, |editor, cx| {
        editor.select_and_reveal(range, cx);
        editor.model_mut().insert("");
        cx.notify();
    });
    draw(cx);
}

#[gpui::test]
fn structure_shows_nothing_while_its_outline_trails_the_text(cx: &mut TestAppContext) {
    let (_dir, root) = mixed_case();
    let text = "fn alpha() {}\n\nfn beta() {}\n";
    let (workbench, cx) = opened(cx, &root, text);
    draw_until_symbol(&workbench, cx);
    let beta = text.find("beta").unwrap();
    put_caret(&workbench, beta, cx);
    assert_eq!(caret_symbol(&workbench, cx).as_deref(), Some("beta"));
    let outline = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).outline().clone()
    });

    // Text typed before beta, its outline held: the outline shown has the
    // old ranges, which would put the caret elsewhere, so nothing shows.
    outline.update(cx, |outline, _| outline.hold_jobs());
    let added = "fn gamma() {}\n";
    type_at(&workbench, 0, added, cx);
    put_caret(&workbench, beta + added.len(), cx);
    assert_eq!(caret_symbol(&workbench, cx), None, "the outline trails");
    // Once it lands, beta again.
    outline.update(cx, |outline, _| outline.release_jobs());
    draw_until_symbol(&workbench, cx);
    assert_eq!(caret_symbol(&workbench, cx).as_deref(), Some("beta"));

    // beta deleted, the outline held: nothing; landed, nothing still, the
    // caret in no symbol.
    outline.update(cx, |outline, _| outline.hold_jobs());
    let start = beta + added.len() - "fn ".len();
    delete(&workbench, start..start + "fn beta() {}".len(), cx);
    assert_eq!(caret_symbol(&workbench, cx), None, "the outline trails");
    outline.update(cx, |outline, _| outline.release_jobs());
    draw(cx);
    draw(cx);
    assert_eq!(caret_symbol(&workbench, cx), None);
}

/// Whether the sidebar's ⋯ menu is open, has focus, and which item is
/// highlighted.
fn menu_state(dock: &Entity<Dock>, cx: &mut VisualTestContext) -> (bool, bool, Option<usize>) {
    cx.update(|window, cx| {
        let state = dock.read(cx).state();
        (
            state.is_menu_open(),
            state.menu_focused(window),
            state.menu_highlighted(),
        )
    })
}

/// The active editor's text.
fn editor_text(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("a file is open");
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

#[gpui::test]
fn the_docks_menu_works_from_the_keyboard(cx: &mut TestAppContext) {
    let (_dir, root) = mixed_case();
    let (workbench, cx) = opened(cx, &root, "fn a() {}\n");
    let dock = dock(&workbench, cx);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("a file is open");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);

    // Opened, it has focus and its first item is highlighted; keys that
    // aren't its own don't reach the editor.
    let more = button(MORE_BUTTON, cx);
    cx.simulate_click(more.center(), Modifiers::none());
    draw(cx);
    assert_eq!(menu_state(&dock, cx), (true, true, Some(0)));
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(
        menu_state(&dock, cx),
        (true, true, Some(0)),
        "one item: wraps"
    );
    cx.simulate_keystrokes("x");
    draw(cx);
    assert_eq!(editor_text(&workbench, cx), "fn a() {}\n");

    // Escape closes it, focus going back to what had it.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(menu_state(&dock, cx), (false, false, None));
    assert!(cx.update(|window, cx| window.focused(cx).is_some()));

    // Enter runs the highlighted item.
    cx.simulate_click(more.center(), Modifiers::none());
    draw(cx);
    assert!(menu_state(&dock, cx).0);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(menu_state(&dock, cx), (false, false, None));
    let position = dock.read_with(cx, |dock, _| dock.state().position());
    assert_eq!(position, DockPosition::Right);
}

/// Whether the active editor has focus.
fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("a file is open");
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// The editor focused, then the dock's ⋯ menu opened from its button, so
/// the menu has focus and the editor is what it gives focus back to.
fn open_more_from_the_editor(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<Dock> {
    let dock = dock(workbench, cx);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("a file is open");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    let more = button(MORE_BUTTON, cx);
    cx.simulate_click(more.center(), Modifiers::none());
    draw(cx);
    assert_eq!(menu_state(&dock, cx), (true, true, Some(0)));
    dock
}

#[gpui::test]
fn the_sidebar_shortcut_hiding_the_docks_menu_gives_focus_back(cx: &mut TestAppContext) {
    let (_dir, root) = mixed_case();
    let (workbench, cx) = opened(cx, &root, "fn a() {}\n");
    let dock = open_more_from_the_editor(&workbench, cx);

    // dock.toggle's key, with the menu open: the sidebar hides, the menu
    // goes, and typing reaches the editor again.
    cx.simulate_keystrokes("cmd-b");
    draw(cx);
    assert!(!dock.read_with(cx, |dock, _| dock.is_visible()));
    assert_eq!(menu_state(&dock, cx), (false, false, None));
    assert!(editor_focused(&workbench, cx));
    cx.simulate_input("x");
    draw(cx);
    assert_eq!(editor_text(&workbench, cx), "xfn a() {}\n");
}

#[gpui::test]
fn hiding_the_dock_or_clearing_its_menu_gives_focus_back(cx: &mut TestAppContext) {
    let (_dir, root) = mixed_case();
    let (workbench, cx) = opened(cx, &root, "fn a() {}\n");

    // Hidden with no window to move focus in, as zen mode hides it: the
    // frame without the menu gives focus back.
    let dock = open_more_from_the_editor(&workbench, cx);
    dock.update(cx, |dock, cx| {
        dock.dock_mut().set_visible(false);
        cx.notify();
    });
    draw(cx);
    assert_eq!(menu_state(&dock, cx), (false, false, None));
    assert!(editor_focused(&workbench, cx));
    cx.simulate_input("x");
    draw(cx);
    assert_eq!(editor_text(&workbench, cx), "xfn a() {}\n");

    // Its items cleared: the same.
    dock.update(cx, |dock, cx| {
        dock.dock_mut().set_visible(true);
        cx.notify();
    });
    let items = dock.read_with(cx, |dock, _| dock.state().menu().to_vec());
    open_more_from_the_editor(&workbench, cx);
    dock.update(cx, |dock, cx| {
        dock.dock_mut().set_menu(Vec::new());
        cx.notify();
    });
    draw(cx);
    assert_eq!(menu_state(&dock, cx), (false, false, None));
    assert!(editor_focused(&workbench, cx));

    // Focus the host moved on first stays where it went.
    dock.update(cx, |dock, cx| {
        dock.dock_mut().set_menu(items);
        cx.notify();
    });
    open_more_from_the_editor(&workbench, cx);
    let tree = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    cx.update(|window, cx| {
        dock.update(cx, |dock, cx| {
            dock.dock_mut().set_menu(Vec::new());
            cx.notify();
        });
        window.focus(&tree.focus_handle(cx), cx);
    });
    draw(cx);
    assert!(cx.update(|window, cx| tree.focus_handle(cx).is_focused(window)));
    assert!(!editor_focused(&workbench, cx));
}

#[gpui::test]
fn the_docks_menu_stays_in_the_window_beside_a_narrow_sidebar(cx: &mut TestAppContext) {
    let (_dir, root) = mixed_case();
    let (workbench, cx) = opened(cx, &root, "fn a() {}\n");
    let dock = dock(&workbench, cx);
    dock.update(cx, |dock, cx| {
        dock.dock_mut().set_size(MIN_DOCK_WIDTH);
        cx.notify();
    });
    draw(cx);
    let column = dock.read_with(cx, |dock, _| dock.column_bounds().expect("drawn"));
    assert_eq!(column.size.width, MIN_DOCK_WIDTH);
    let more = button(MORE_BUTTON, cx);
    cx.simulate_click(more.center(), Modifiers::none());
    draw(cx);
    let menu = bounds("dock-menu", cx);
    let window = cx.update(|window, _| window.viewport_size());
    // Wider than the sidebar's right part, it's shifted in, not cut off.
    assert!(menu.left() >= px(0.), "{menu:?}");
    assert!(menu.right() <= window.width, "{menu:?} {window:?}");
    assert!(menu.top() >= more.bottom(), "{menu:?} under {more:?}");
}

#[gpui::test]
fn collapse_all_from_the_header_drops_a_reveal_waiting_for_a_listing(cx: &mut TestAppContext) {
    let (_dir, root) = mixed_case();
    fs::create_dir_all(root.join("src/deep")).unwrap();
    fs::write(root.join("src/deep/c.rs"), "").unwrap();
    let (workbench, cx) = opened(cx, &root, "fn a() {}\n");
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());

    // A reveal of a file under unlisted folders, then, before their
    // listings land, Collapse All from the dock's header.
    panel.update(cx, |panel, cx| panel.reveal(root.join("src/deep/c.rs"), cx));
    let collapse = crate::project_panel::pane_actions(&panel)
        .into_iter()
        .find(|action| action.id == "project-collapse")
        .expect("the header's Collapse All");
    cx.update(|window, cx| (collapse.run)(window, cx));
    draw(cx);
    draw(cx);
    let names = panel.read_with(cx, |panel, _| panel.list_names());
    let names: Vec<_> = names.into_iter().map(|(_, name)| name).collect();
    assert!(!names.contains(&"c.rs".to_owned()), "{names:?}");
    assert!(!names.contains(&"deep".to_owned()), "{names:?}");

    // Left alone, the same reveal lands.
    panel.update(cx, |panel, cx| panel.reveal(root.join("src/deep/c.rs"), cx));
    draw(cx);
    draw(cx);
    let names = panel.read_with(cx, |panel, _| panel.list_names());
    assert!(names.iter().any(|(_, name)| name == "c.rs"), "{names:?}");
}
