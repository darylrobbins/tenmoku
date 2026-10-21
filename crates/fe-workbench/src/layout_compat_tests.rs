//! The window's layout, saved and restored through kiln's workspace
//! ([`Workspace::set_layout`](kiln_shell::Workspace::set_layout)): an entry
//! saved before it did restores as it did then and saves again the same,
//! and nothing is saved as frames are drawn, only as the layout changes.
//!
//! And, carried from the review of T690 (Mac T690), the workspace keeps
//! fastedit's policy for a pane showing a file being read, or a read's
//! error, over its tab ([`Pane::active_item`], [`Pane::focus_target`]): a
//! split copies no tab hidden so, and focus goes to the pane, not the
//! hidden tab, as a click or a close makes it the active one.

use std::{fs, path::Path};

use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use gpui::{
    AppContext as _, Entity, Focusable as _, Modifiers, MouseButton, TestAppContext,
    VisualTestContext, WindowOptions, point, px,
};
use kiln_shell::{
    DockPosition, DockView as _, PaneView as _,
    layout::{self, LayoutStore, WindowLayout},
};
use serde_json::json;

use crate::{Launch, Workbench, pane::Pane};

/// A project of three files, `a.txt`, `b.txt` and `c.txt`, and its
/// layout store beside it.
struct Project {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    store: LayoutStore,
}

fn project() -> Project {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    fs::create_dir(&root).unwrap();
    for name in ["a", "b", "c"] {
        fs::write(
            root.join(format!("{name}.txt")),
            format!("{name}{name}{name}\n"),
        )
        .unwrap();
    }
    let root = fs::canonicalize(&root).unwrap();
    let store = LayoutStore::new(dir.path().join("layouts"), &root);
    Project {
        _dir: dir,
        root,
        store,
    }
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
}

/// An entry in the format saved before the workspace saved the layout:
/// `a.txt` and `b.txt` in the left pane, `b.txt` and `c.txt` in the right
/// one, active, the sidebar at the left, 300 wide, its project panel on
/// the rail, the terminal's dock at the right, 360 wide, the breadcrumbs
/// off, and the window's bounds.
fn todays_entry(root: &Path) -> Value {
    let editor = |name: &str, document: u64, caret: usize| {
        let path = root.join(name);
        json!({
            "kind": "fastedit.editor",
            "document": document,
            "state": {"path": path.to_str().unwrap(), "caret": caret},
        })
    };
    json!({
        "panes": {
            "tree": {
                "axis": "horizontal",
                "ratios": [0.5, 0.5],
                "members": [
                    {"pane": {"items": [editor("a.txt", 0, 1), editor("b.txt", 1, 2)], "active": 1}},
                    {"pane": {"items": [editor("b.txt", 1, 3), editor("c.txt", 2, 0)], "active": 1}},
                ],
            },
            "active": [1],
        },
        "docks": {"sidebar": "left", "terminal": "right"},
        "dock": {
            "visible": true,
            "active": "project",
            "joined": ["project"],
            "size": 300.0,
            "side_size": 300.0,
            "bottom_size": 240.0,
        },
        "terminal_dock": {"size": 360.0, "side_size": 360.0, "bottom_size": 240.0},
        "breadcrumbs": false,
        "bounds": {"state": "windowed", "x": 40.0, "y": 30.0, "width": 1400.0, "height": 900.0},
    })
}

/// A launch's window, as the host opens it: at the entry's bounds, if it
/// has one, which it restores, saving in `layout`. Drawn and focused.
fn open(
    project: &Project,
    layout: WindowLayout,
    entry: Option<Value>,
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, VisualTestContext) {
    let launch = Launch {
        root: Some(project.root.clone()),
        file: None,
    };
    let options = WindowOptions {
        window_bounds: entry
            .as_ref()
            .and_then(|entry| layout::restore_bounds(&entry["bounds"])),
        ..Default::default()
    };
    let window = cx.update(|cx| {
        cx.open_window(options, move |window, cx| {
            cx.new(|cx| {
                let theme = EditorTheme::default_dark(cx);
                let mut workbench =
                    Workbench::new(launch, theme, EditorSettings::default(), window, cx);
                if let Some(entry) = &entry {
                    workbench.restore_layout(entry, window, cx);
                }
                workbench.set_layout(layout, window, cx);
                workbench
            })
        })
        .unwrap()
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(&mut vcx);
    (workbench, vcx)
}

/// Draws a frame, then those it asks for.
fn draw(cx: &mut VisualTestContext) {
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
            break;
        }
    }
    cx.run_until_parked();
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// The store's entries, without what the store stamps on them.
fn saved(store: &LayoutStore) -> Vec<Value> {
    let mut entries = store.entries();
    for entry in &mut entries {
        let object = entry.as_object_mut().unwrap();
        object.remove("id");
        object.remove("saved");
    }
    entries
}

/// Whether the store's file was written since the last call: it's taken
/// away each time, so a write puts it back.
fn written(store: &LayoutStore) -> bool {
    let file = store.file();
    let written = file.exists();
    if written {
        fs::remove_file(file).unwrap();
    }
    written
}

/// Each pane's editors.
fn editors(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<Vec<Entity<EditorView>>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().iter())
            .map(|pane| {
                (pane.read(cx).tabs().iter())
                    .map(|tab| tab.editor().unwrap().clone())
                    .collect()
            })
            .collect()
    })
}

/// An editor's file name and caret.
fn shown(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (String, usize) {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let path = model.path().unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        (name, model.primary_selection().head())
    })
}

#[gpui::test]
fn todays_entry_restores_and_saves_the_same(cx: &mut TestAppContext) {
    let project = project();
    init(cx);
    let entry = todays_entry(&project.root);
    // Saved by the last session, as it quit.
    let claim = project.store.claim_new().unwrap();
    project.store.write(&claim, entry.clone()).unwrap();
    drop(claim);
    let mut windows = layout::launch(&project.store, true);
    assert_eq!(windows.len(), 1);
    let (layout, restored) = windows.remove(0);
    let (workbench, mut cx) = open(&project, layout, restored, cx);
    let cx = &mut cx;

    // The panes and their tabs, carets and active ones.
    let panes = editors(&workbench, cx);
    let names: Vec<Vec<(String, usize)>> = (panes.iter())
        .map(|pane| pane.iter().map(|editor| shown(editor, cx)).collect())
        .collect();
    let tab = |name: &str, caret| (name.to_owned(), caret);
    assert_eq!(
        names,
        [
            vec![tab("a.txt", 1), tab("b.txt", 2)],
            vec![tab("b.txt", 3), tab("c.txt", 0)],
        ]
    );
    let shares = cx.update(|_, cx| {
        let (left, right) = (panes[0][1].read(cx).model(), panes[1][0].read(cx).model());
        left.shares_buffer_with(right)
    });
    assert!(shares, "the two views of b.txt share its buffer");
    let active = workbench.read_with(cx, |workbench, cx| {
        let group = workbench.pane_group(cx).panes();
        let actives: Vec<_> = (group.iter())
            .map(|pane| pane.read(cx).pane().active())
            .collect();
        let active = group.iter().position(|pane| **pane == *workbench.pane());
        (actives, active)
    });
    assert_eq!(active, (vec![Some(1), Some(1)], Some(1)));
    let focused = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    assert_eq!(focused.as_ref(), Some(&panes[1][1]));

    // The docks' places and sizes, the terminal's hidden.
    let (sidebar, terminal) = workbench.read_with(cx, |workbench, cx| {
        let sidebar = workbench.dock().read(cx).state();
        let terminal = workbench.terminal_dock().read(cx).dock();
        (
            (
                sidebar.position(),
                sidebar.is_visible(),
                sidebar.current_size(),
            ),
            (
                terminal.position(),
                terminal.is_visible(),
                terminal.current_size(),
            ),
        )
    });
    assert_eq!(sidebar, (DockPosition::Left, true, px(300.)));
    assert_eq!(terminal, (DockPosition::Right, false, px(360.)));

    // The breadcrumbs off, in every pane.
    let breadcrumbs = workbench.read_with(cx, |workbench, cx| {
        let panes = workbench.pane_group(cx).panes();
        let toolbars: Vec<_> = (panes.iter())
            .map(|pane| pane.read(cx).toolbar().visible)
            .collect();
        (workbench.breadcrumbs, toolbars)
    });
    assert_eq!(breadcrumbs, (false, vec![false, false]));

    // Saved again, once its files were in, the same, but for the
    // Explorer's and Source Control's views, saved before them as built.
    assert_eq!(
        cx.update(|window, _| window.window_bounds()),
        layout::restore_bounds(&entry["bounds"]).unwrap()
    );
    let mut entry = entry;
    entry["dock"]["panels"] = json!({
        "project": {"views": {
            "folders": {"collapsed": false, "weight": 1.0},
            "outline": {"collapsed": true, "weight": 1.0},
        }},
        "scm": {"views": {
            "changes": {"collapsed": false, "weight": 1.0},
            "graph": {"collapsed": false, "weight": 1.0},
        }},
    });
    assert_eq!(saved(&project.store), std::slice::from_ref(&entry));
    let again = workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert_eq!(again, entry);
}

#[gpui::test]
fn frames_save_nothing_and_a_sidebar_drag_saves_once(cx: &mut TestAppContext) {
    let project = project();
    init(cx);
    let claim = project.store.claim_new().unwrap();
    let layout = WindowLayout::new(project.store.clone(), claim);
    let (workbench, mut cx) = open(&project, layout, None, cx);
    let cx = &mut cx;
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = project.root.join("a.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    assert_eq!(saved(&project.store).len(), 1);
    assert!(written(&project.store));

    // Frames alone, with nothing changed.
    for _ in 0..3 {
        cx.update(|window, _| window.refresh());
        draw(cx);
    }
    assert!(!written(&project.store), "drawing saved the layout");

    // The sidebar's inner edge dragged: once, as it's let go.
    let handle = workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx).state();
        (dock.drawn().handle_bounds())
            .expect("the sidebar shows")
            .center()
    });
    let to = handle + point(px(-80.), px(0.));
    let none = Modifiers::none();
    let mut writes = 0;
    cx.simulate_mouse_down(handle, MouseButton::Left, none);
    writes += usize::from(written(&project.store));
    // Past the drag's threshold, then to its end.
    cx.simulate_mouse_move(handle + point(px(3.), px(3.)), MouseButton::Left, none);
    writes += usize::from(written(&project.store));
    cx.simulate_mouse_move(to, MouseButton::Left, none);
    draw(cx);
    writes += usize::from(written(&project.store));
    cx.simulate_mouse_up(to, MouseButton::Left, none);
    draw(cx);
    writes += usize::from(written(&project.store));
    let size = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).state().current_size()
    });
    assert_ne!(size, px(240.), "the drag resized the sidebar");
    assert_eq!(writes, 1);
    let entry = workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert_eq!(
        entry["dock"]["size"].as_f64(),
        Some(f64::from(f32::from(size)))
    );
}

/// A window on the project showing `a.txt`, its editor focused.
fn open_on_a(project: &Project, cx: &mut TestAppContext) -> (Entity<Workbench>, VisualTestContext) {
    let claim = project.store.claim_new().unwrap();
    let layout = WindowLayout::new(project.store.clone(), claim);
    let (workbench, mut cx) = open(project, layout, None, cx);
    let pane = active(&workbench, &mut cx);
    let path = project.root.join("a.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(&mut cx);
    (workbench, cx)
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

/// Runs a command without drawing: a read started stays unread.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
}

/// Clicks the middle of `pane`, where the last frame laid it out.
fn click(pane: &Entity<Pane>, cx: &mut VisualTestContext) {
    let at = pane.read_with(cx, |pane, _| pane.bounds().center());
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    draw(cx);
}

/// Whether `pane` itself has focus, not an item in it.
fn pane_focused(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| pane.read(cx).focus_handle(cx).is_focused(window))
}

fn editor_focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.read(cx).focus_handle(cx).is_focused(window))
}

fn tab_count(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> usize {
    pane.read_with(cx, |pane, _| pane.tabs().len())
}

#[gpui::test]
fn a_split_over_a_read_or_its_error_copies_no_tab(cx: &mut TestAppContext) {
    let project = project();
    init(cx);
    let (workbench, mut cx) = open_on_a(&project, cx);
    let cx = &mut cx;
    let left = active(&workbench, cx);

    // `b.txt` still being read over `a.txt`'s tab.
    let path = project.root.join("b.txt");
    cx.update(|window, cx| left.update(cx, |pane, cx| pane.open(path, window, cx)));
    dispatch("pane.split_right", cx);
    let right = active(&workbench, cx);
    assert_ne!(right, left);
    assert_eq!(tab_count(&right, cx), 0, "the split copied the hidden tab");
    draw(cx);
    assert_eq!((tab_count(&left, cx), tab_count(&right, cx)), (2, 0));

    // A read that failed, over `b.txt`'s tab: the pane shows the error.
    let folder = project.root.join("folder");
    fs::create_dir(&folder).unwrap();
    cx.update(|window, cx| left.update(cx, |pane, cx| pane.open(folder, window, cx)));
    draw(cx);
    assert!(left.read_with(cx, |pane, _| pane.active_item().is_none()));
    assert_eq!(active(&workbench, cx), left);
    run("pane.split_right", cx);
    let split = active(&workbench, cx);
    assert!(split != left && split != right);
    assert_eq!(
        tab_count(&split, cx),
        0,
        "the split copied the tab under the error"
    );
    assert!(pane_focused(&split, cx));
}

#[gpui::test]
fn a_click_on_an_empty_or_reading_pane_focuses_it(cx: &mut TestAppContext) {
    let project = project();
    init(cx);
    let (workbench, mut cx) = open_on_a(&project, cx);
    let cx = &mut cx;
    let left = active(&workbench, cx);
    let a = editors(&workbench, cx)[0][0].clone();

    // An empty pane at the right; `a.txt`'s editor focused in the left.
    let path = project.root.join("b.txt");
    cx.update(|window, cx| left.update(cx, |pane, cx| pane.open(path, window, cx)));
    dispatch("pane.split_right", cx);
    draw(cx);
    let right = (workbench.read_with(cx, |workbench, cx| workbench.panes(cx)))[1].clone();
    assert_eq!(tab_count(&right, cx), 0);
    click(&left, cx);
    let b = editors(&workbench, cx)[0][1].clone();
    assert!(editor_focused(&b, cx));
    click(&right, cx);
    assert_eq!(active(&workbench, cx), right);
    assert!(
        pane_focused(&right, cx),
        "focus stayed in the left pane's editor"
    );

    // The left pane reading `c.txt` over its tabs.
    let path = project.root.join("c.txt");
    left.update(cx, |pane, cx| pane.opening(path, cx));
    draw(cx);
    click(&left, cx);
    assert_eq!(active(&workbench, cx), left);
    assert!(pane_focused(&left, cx));
    assert!(!editor_focused(&a, cx) && !editor_focused(&b, cx));
}

#[gpui::test]
fn closing_onto_a_reading_or_failed_pane_focuses_the_pane(cx: &mut TestAppContext) {
    let project = project();
    init(cx);
    let (workbench, mut cx) = open_on_a(&project, cx);
    let cx = &mut cx;
    let left = active(&workbench, cx);
    let a = editors(&workbench, cx)[0][0].clone();

    // The left pane reading `b.txt` over `a.txt`'s tab, the right active.
    run("pane.split_right", cx);
    let path = project.root.join("b.txt");
    left.update(cx, |pane, cx| pane.opening(path, cx));
    draw(cx);
    assert_ne!(active(&workbench, cx), left);
    run("pane.close", cx);
    assert_eq!(active(&workbench, cx), left);
    assert!(
        pane_focused(&left, cx),
        "focus went to the tab under the read"
    );
    assert!(!editor_focused(&a, cx));

    // The left pane showing a read's error over `a.txt`'s tab, an empty
    // pane split off it active.
    let folder = project.root.join("folder");
    fs::create_dir(&folder).unwrap();
    cx.update(|window, cx| left.update(cx, |pane, cx| pane.open(folder, window, cx)));
    draw(cx);
    assert!(left.read_with(cx, |pane, _| pane.active_item().is_none()));
    run("pane.split_right", cx);
    assert_ne!(active(&workbench, cx), left);
    run("pane.close", cx);
    assert_eq!(active(&workbench, cx), left);
    assert!(
        pane_focused(&left, cx),
        "focus went to the tab under the error"
    );
    assert!(!editor_focused(&a, cx));
}
