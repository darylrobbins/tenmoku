//! The project tree's look, through the debug selectors `TreeRow` sets on
//! its chevrons, icons and indent guides, and every list's row height.

use std::fs;

use fe_buffer::Buffer;
use fe_ui::{CHEVRON_WIDTH, ICON_SIZE, ROW_HEIGHT, TREE_INDENT};
use gpui::{Bounds, Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext, px, size};

use super::{click_row, open, open_root, panel, rows};
use crate::{Launch, PickerDelegate as _, ProjectPanel, picker::VISIBLE_ROWS};

/// `src/{main.rs, util/mod.rs}`, `Cargo.toml`, `README.md`, and an ignored
/// `target/`.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src/util")).unwrap();
    fs::create_dir_all(root.join("target")).unwrap();
    for (rel, text) in [
        ("src/main.rs", "fn main() {}\n"),
        ("src/util/mod.rs", "pub fn util() {}\n"),
        ("Cargo.toml", "[package]\n"),
        ("README.md", "# Readme\n"),
        (".gitignore", "target/\n"),
        ("target/out", ""),
    ] {
        fs::write(root.join(rel), text).unwrap();
    }
    dir
}

/// The project's workbench, with `src/` expanded.
fn with_src_expanded<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
) -> (Entity<ProjectPanel>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = panel(&workbench, cx);
    click_row(&panel, "src", cx);
    cx.run_until_parked();
    (panel, cx)
}

fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("nothing drawn as {selector}"))
}

/// Row `name`'s laid-out bounds.
fn row_bounds(
    panel: &Entity<ProjectPanel>,
    name: &str,
    cx: &mut VisualTestContext,
) -> Bounds<Pixels> {
    let ix = rows(panel, cx)
        .iter()
        .position(|(_, row)| row == name)
        .unwrap_or_else(|| panic!("no row {name}"));
    panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"))
}

#[gpui::test]
fn rows_carry_icons_and_chevrons(cx: &mut TestAppContext) {
    let dir = project();
    let (panel, cx) = with_src_expanded(cx, &dir);
    for selector in [
        "tree-icon:main.rs:file-code",
        "tree-icon:Cargo.toml:package",
        "tree-icon:README.md:file-text",
        "tree-icon:src:folder-open",
        "tree-icon:util:folder",
        "tree-chevron:src:open",
        "tree-chevron:util:closed",
    ] {
        assert_eq!(
            bounds(selector, cx).size,
            size(px(16.), px(16.)),
            "{selector}"
        );
    }
    for selector in ["tree-chevron:main.rs:open", "tree-chevron:main.rs:closed"] {
        assert_eq!(cx.debug_bounds(selector), None, "{selector}");
    }

    click_row(&panel, "src", cx);
    cx.run_until_parked();
    assert_eq!(
        rows(&panel, cx)
            .iter()
            .filter(|(depth, _)| *depth > 0)
            .count(),
        0
    );
    assert_eq!(cx.debug_bounds("tree-chevron:src:open"), None);
    assert_eq!(cx.debug_bounds("tree-icon:src:folder-open"), None);
    bounds("tree-chevron:src:closed", cx);
    bounds("tree-icon:src:folder", cx);
}

#[gpui::test]
fn layout_metrics(cx: &mut TestAppContext) {
    let dir = project();
    let (panel, cx) = with_src_expanded(cx, &dir);
    let names = rows(&panel, cx);
    assert_eq!(names.len(), 6, "target/ is hidden: {names:?}");
    for (ix, (_, name)) in names.iter().enumerate() {
        let row = panel.read_with(cx, |panel, _| panel.row_bounds(ix).unwrap());
        assert_eq!(row.size.height, ROW_HEIGHT, "{name}");
    }

    let src_chevron = bounds("tree-chevron:src:open", cx);
    let util_chevron = bounds("tree-chevron:util:closed", cx);
    assert_eq!(util_chevron.left() - src_chevron.left(), TREE_INDENT);
    let util_icon = bounds("tree-icon:util:folder", cx);
    let main_icon = bounds("tree-icon:main.rs:file-code", cx);
    assert_eq!(
        main_icon.left(),
        util_icon.left(),
        "files keep the chevron column"
    );
    assert_eq!(util_icon.size, size(ICON_SIZE, ICON_SIZE));
    assert_eq!(util_chevron.size.width, CHEVRON_WIDTH);

    for (chevron, icon) in [
        (src_chevron, bounds("tree-icon:src:folder-open", cx)),
        (util_chevron, util_icon),
    ] {
        assert_eq!(icon.left() - chevron.right(), px(2.));
    }
    for (name, icon) in [
        ("src", "tree-icon:src:folder-open"),
        ("util", "tree-icon:util:folder"),
        ("main.rs", "tree-icon:main.rs:file-code"),
        ("Cargo.toml", "tree-icon:Cargo.toml:package"),
        ("README.md", "tree-icon:README.md:file-text"),
    ] {
        let row = row_bounds(&panel, name, cx);
        let icon = bounds(icon, cx);
        let off = (icon.center().y - row.center().y).abs();
        assert!(off <= px(0.5), "{name}'s icon is {off:?} off centre");
    }
}

#[gpui::test]
fn indent_guides_follow_depth(cx: &mut TestAppContext) {
    let dir = project();
    let (panel, cx) = with_src_expanded(cx, &dir);
    click_row(&panel, "util", cx);
    cx.run_until_parked();
    assert!(rows(&panel, cx).contains(&(2, "mod.rs".into())));

    for selector in [
        "tree-guide:src:0",
        "tree-guide:Cargo.toml:0",
        "tree-guide:README.md:0",
        "tree-guide:.gitignore:0",
        "tree-guide:target:0",
    ] {
        assert_eq!(cx.debug_bounds(selector), None, "{selector}");
    }
    assert_eq!(cx.debug_bounds("tree-guide:mod.rs:2"), None);
    let mod_row = row_bounds(&panel, "mod.rs", cx);
    for (selector, chevron) in [
        ("tree-guide:mod.rs:0", "tree-chevron:src:open"),
        ("tree-guide:mod.rs:1", "tree-chevron:util:open"),
    ] {
        let guide = bounds(selector, cx);
        assert_eq!(guide.size, size(px(1.), ROW_HEIGHT), "{selector}");
        assert_eq!(guide.top(), mod_row.top(), "{selector}");
        assert_eq!(
            guide.center().x,
            bounds(chevron, cx).center().x,
            "{selector}"
        );
    }
}

#[gpui::test]
fn every_list_row_is_24px(cx: &mut TestAppContext) {
    assert_eq!(ROW_HEIGHT, px(24.));
    let dir = project();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((
            dir.path().join("src/main.rs"),
            Some(Buffer::new("fn main() {}\n").into()),
        )),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.run_until_parked();

    // The project panel.
    let panel = panel(&workbench, cx);
    let height = panel.read_with(cx, |panel, _| panel.row_bounds(0).unwrap().size.height);
    assert_eq!(height, ROW_HEIGHT, "a tree row");

    // Quick open, listing every file.
    let picker = workbench.read_with(cx, |workbench, _| workbench.quick_open().clone());
    cx.simulate_keystrokes("cmd-p");
    cx.run_until_parked();
    let (count, row, list) = picker.read_with(cx, |picker, _| {
        let row = picker.row_bounds(0).expect("drawn");
        (picker.delegate.match_count(), row, picker.list_bounds())
    });
    assert!(count > 0);
    assert_eq!(row.size.height, ROW_HEIGHT, "a picker row");
    assert_eq!(
        list.size.height,
        ROW_HEIGHT * count.min(VISIBLE_ROWS) as f32
    );
    cx.simulate_keystrokes("escape");

    // Search: a file header, then its match.
    let search = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).search().clone()
    });
    cx.simulate_keystrokes("cmd-shift-f");
    cx.simulate_input("fn main");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let heights = search.read_with(cx, |search, _| {
        assert_eq!(search.shown_rows().len(), 2, "{:?}", search.shown_rows());
        [0, 1].map(|ix| search.row_bounds(ix).expect("drawn").size.height)
    });
    assert_eq!(heights, [ROW_HEIGHT; 2], "search rows");

    // The outline of main.rs.
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-alt-o");
    cx.run_until_parked();
    let outline = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).outline().clone()
    });
    let height = outline.read_with(cx, |outline, _| {
        assert!(!outline.rows().is_empty());
        outline.row_bounds(0).expect("drawn").size.height
    });
    assert_eq!(height, ROW_HEIGHT, "an outline row");
}
