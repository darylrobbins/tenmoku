//! The Explorer: one rail button for a view container of two views, the
//! project tree (Folders), its lead, without a header and never collapsed,
//! over Structure, the outline, collapsed at first; `dock.show_project`
//! shows it with the tree, `outline.show` with the outline, expanded; the
//! views' collapse is saved with the layout, and a layout saved before the
//! Explorer, which has none, loads with the views as built.

use std::fs;

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::commands::RunCommand;
use fe_ui::Icon;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln_shell::{PanelId, ViewId};
use serde_json::json;

use super::DockPanel;
use crate::{
    Launch, Workbench, outline_panel::VIEW as OUTLINE, project_panel::FOLDERS, tests::open,
};

/// A workbench on a folder of `a.rs`, showing it, focused as the app
/// focuses it.
fn opened(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(root.to_owned()),
        file: Some((root.join("a.rs"), Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (dir, workbench, cx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs workbench command `id` as an unbound key would.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Whether the dock shows, its active panel and its title row.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (bool, DockPanel, String) {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        let title = dock.state().title(cx).unwrap_or_default().to_string();
        (dock.is_visible(), dock.active(), title)
    })
}

/// Whether each of the Explorer's views, Folders then Outline, is
/// collapsed.
fn collapsed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> [Option<bool>; 2] {
    workbench.read_with(cx, |workbench, cx| {
        let explorer = workbench.dock().read(cx).explorer().read(cx);
        let stack = explorer.stack().read(cx);
        [stack.is_collapsed(FOLDERS), stack.is_collapsed(OUTLINE)]
    })
}

fn set_collapsed(
    workbench: &Entity<Workbench>,
    view: ViewId,
    collapsed: bool,
    cx: &mut VisualTestContext,
) {
    let stack = workbench.read_with(cx, |workbench, cx| {
        (workbench.dock().read(cx).explorer().read(cx).stack()).clone()
    });
    stack.update(cx, |stack, cx| stack.set_collapsed(view, collapsed, cx));
    draw(cx);
}

/// Whether the active editor has focus.
fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("a file is open");
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// Whether the project tree has focus.
fn tree_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    cx.update(|window, cx| panel.focus_handle(cx).is_focused(window))
}

/// The Explorer's views' saved state in the window's layout.
fn saved_views(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Value {
    let entry = workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    entry["dock"]["panels"]["project"]["views"].clone()
}

#[gpui::test]
fn the_rail_has_one_explorer_of_folders_over_the_outline(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    let (rail, panels, icons) = workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        let state = dock.state();
        let rail: Vec<_> = (state.panel_ids().into_iter())
            .filter(|id| state.on_rail(*id, cx))
            .collect();
        (rail, state.panel_ids(), dock.rail_icons())
    });
    assert_eq!(
        rail,
        [
            DockPanel::Explorer.id(),
            DockPanel::Search.id(),
            DockPanel::SourceControl.id(),
            DockPanel::Chat.id(),
        ]
    );
    assert!(!panels.contains(&PanelId("outline")), "{panels:?}");
    assert_eq!(DockPanel::of(PanelId("outline")), None);
    assert_eq!(icons[..2], [Icon::Files, Icon::Search]);
    assert!(!icons.contains(&Icon::ListTree), "{icons:?}");
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::Explorer, "Project".to_owned())
    );

    // Its views: the tree, drawn in its body without a header, over the
    // outline, collapsed.
    let (views, titles) = workbench.read_with(cx, |workbench, cx| {
        let explorer = workbench.dock().read(cx).explorer().read(cx);
        let stack = explorer.stack().read(cx);
        let headers = [FOLDERS, OUTLINE].map(|view| stack.header_bounds(view));
        let bodies = [FOLDERS, OUTLINE].map(|view| stack.body_bounds(view));
        (explorer.view_ids(cx), (headers, bodies))
    });
    assert_eq!(views, [FOLDERS, OUTLINE]);
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(true)]);
    let ([folders, outline], [tree, no_outline]) = titles;
    assert_eq!(folders, None);
    let (outline, tree) = (outline.unwrap(), tree.unwrap());
    assert!(tree.bottom() <= outline.top());
    assert_eq!(no_outline, None);
    let drawn = cx.debug_bounds("sidebar-panel").expect("the tree drawn");
    assert_eq!(drawn, tree);

    // A click on the Outline's header expands it under the tree, and the
    // layout keeps it.
    cx.simulate_click(outline.center(), Modifiers::none());
    draw(cx);
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(false)]);
    assert_eq!(saved_views(&workbench, cx)["outline"]["collapsed"], false);
    let outline_body = workbench.read_with(cx, |workbench, cx| {
        let explorer = workbench.dock().read(cx).explorer().read(cx);
        explorer.stack().read(cx).body_bounds(OUTLINE)
    });
    assert!(outline_body.expect("drawn").top() > tree.top());

    // A new root's tree takes the old one's place, the views as they were.
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_owned();
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_root(path, window, cx)
    });
    draw(cx);
    let (panel, folders_view) = workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        let views = dock.explorer().read(cx).view_ids(cx);
        (dock.project().clone() == *workbench.panel(), views)
    });
    assert!(panel);
    assert_eq!(folders_view, [FOLDERS, OUTLINE]);
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(false)]);
}

#[gpui::test]
fn show_project_and_outline_show_show_the_explorer(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    dispatch("search.show", cx);
    assert_eq!(shown(&workbench, cx).1, DockPanel::Search);

    // The outline's command shows the Explorer with the outline expanded,
    // focus going to the outline from the search panel it hid.
    let outline = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).outline().focus_handle(cx)
    });
    dispatch("outline.show", cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::Explorer, "Project".to_owned())
    );
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(false)]);
    assert!(cx.update(|window, _| outline.is_focused(window)));
    assert!(!editor_focused(&workbench, cx));

    // The tree, the lead, doesn't collapse. The outline collapsed, and
    // the dock hidden, it shows the outline again.
    set_collapsed(&workbench, FOLDERS, true, cx);
    set_collapsed(&workbench, OUTLINE, true, cx);
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(true)]);
    dispatch("dock.toggle", cx);
    assert!(!shown(&workbench, cx).0);
    dispatch("outline.show", cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::Explorer, "Project".to_owned())
    );
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(false)]);
    assert!(cx.update(|window, _| outline.is_focused(window)));
    assert!(!editor_focused(&workbench, cx));

    // dock.show_project shows it with the tree expanded and focused, the
    // outline as it was.
    dispatch("search.show", cx);
    dispatch("dock.show_project", cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::Explorer, "Project".to_owned())
    );
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(false)]);
    assert!(tree_focused(&workbench, cx));
    set_collapsed(&workbench, OUTLINE, true, cx);
    dispatch("dock.show_project", cx);
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(true)]);

    // The Explorer's button does as dock.show_project does.
    dispatch("search.show", cx);
    let button = workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        dock.rail_button_bounds(DockPanel::Explorer).expect("drawn")
    });
    cx.simulate_click(button.center(), Modifiers::none());
    draw(cx);
    assert_eq!(shown(&workbench, cx).1, DockPanel::Explorer);
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(true)]);
    assert!(tree_focused(&workbench, cx));
}

/// A layout entry of fastedit's keys alone, its sidebar as `dock`.
fn entry(dock: Value) -> Value {
    json!({
        "docks": {"sidebar": "right", "terminal": "bottom"},
        "dock": dock,
        "breadcrumbs": true,
    })
}

/// Restores `entry`, drawing after.
fn restore(workbench: &Entity<Workbench>, entry: &Value, cx: &mut VisualTestContext) {
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(entry, window, cx)
    });
    draw(cx);
}

#[gpui::test]
fn a_layout_saved_with_the_project_panel_loads_the_explorer_as_built(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    dispatch("search.show", cx);
    let project = json!({
        "visible": true,
        "active": "project",
        "joined": ["project"],
        "size": 300.0,
        "side_size": 300.0,
        "bottom_size": 240.0,
    });
    restore(&workbench, &entry(project), cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::Explorer, "Project".to_owned())
    );
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(true)]);
    let size = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).state().current_size()
    });
    assert_eq!(size, gpui::px(300.));
    // Saved again, it has the views.
    assert_eq!(
        saved_views(&workbench, cx),
        json!({
            "folders": {"collapsed": false, "weight": 1.0},
            "outline": {"collapsed": true, "weight": 1.0},
        })
    );
}

#[gpui::test]
fn a_layout_saved_with_the_outline_panel_loads_the_explorer_then_views_as_saved(
    cx: &mut TestAppContext,
) {
    let (_dir, workbench, cx) = opened(cx);
    dispatch("search.show", cx);
    // The Outline panel showing: the Explorer shows, its outline as built.
    let outline = json!({"visible": true, "active": "outline", "joined": ["outline", "search"]});
    restore(&workbench, &entry(outline), cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::Explorer, "Project".to_owned())
    );
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(true)]);

    // Saved since, the views come back as they were saved, a view the
    // Explorer doesn't have ignored, the tree expanded whatever was saved.
    let views = json!({
        "visible": true,
        "active": "project",
        "panels": {"project": {"views": {
            "folders": {"collapsed": true, "weight": 1.0},
            "outline": {"collapsed": false, "weight": 2.0},
            "timeline": {"collapsed": false, "weight": 1.0},
        }}},
    });
    restore(&workbench, &entry(views), cx);
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(false)]);
    let saved = saved_views(&workbench, cx);
    assert_eq!(saved["outline"]["weight"], 2.0);
    assert_eq!(saved["folders"]["collapsed"], false);
    assert_eq!(saved.get("timeline"), None);
}
