// SPDX-License-Identifier: Apache-2.0

//! What a screen reader sees of a Workbench window's pickers, tab switcher
//! and find bar (design `docs/decisions/qenw9xxdzk-accessibility-design.md`,
//! section 6):
//!
//! - the command palette and quick open are a `Dialog` named by their
//!   title or placeholder, holding a `SearchInput` and a `ListBox` of
//!   `ListBoxOption`s named by what their rows say. Focus stays in the
//!   query, the rows' sibling, so the selected row can't be the active
//!   descendant; it is the one option selected, and it's announced as the
//!   query refilters and the selection moves;
//! - the tab switcher holds focus on its `Dialog`, so its selected row is
//!   the active descendant, following each press;
//! - the find bar is a `Search` landmark with a `SearchInput`, a replace
//!   `TextInput` and named toggles and buttons.

use std::fs;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_ui::{Node, Role, RowState, nodes};
use fe_workbench::{Launch, Workbench};
use gpui::{Bounds, Entity, Focusable as _, Modifiers, Pixels, TestAppContext, VisualTestContext};
use kiln_shell::{Picker, PickerDelegate as _, command_palette::CommandPalette};

/// A workbench rooted at a temporary folder holding `names`, each opened
/// in its pane in order, the last shown and focused.
fn opened<'a>(
    names: &[&str],
    cx: &'a mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &'a mut VisualTestContext,
) {
    // See palette_ghost_text.rs: no config directory, no watcher threads.
    for var in ["FASTEDIT_CONFIG_DIR", "XDG_CONFIG_HOME", "HOME"] {
        // SAFETY: no other thread is running yet to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    let dir = tempfile::tempdir().unwrap();
    for name in names {
        fs::write(dir.path().join(name), format!("{name}\n")).unwrap();
    }
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_path_buf()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    // Announcements go to the active window.
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    for name in names {
        let path = dir.path().join(name);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    cx.update(|window, cx| {
        let editor = workbench.read(cx).active_editor(cx).cloned();
        let editor = editor.expect("the last file opens in an editor");
        window.focus(&editor.focus_handle(cx), cx);
    });
    draw(cx);
    (dir, workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// What the recorder saw in a frame that drew every view afresh.
fn drawn(cx: &mut VisualTestContext) -> Vec<Node> {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
        nodes(window, cx)
    })
}

/// Runs `command` as a key would: on the focused element, in the context
/// its key contexts make.
fn run(command: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let context: ContextStack = window
            .context_stack()
            .iter()
            .map(|context| ContextFrame::new(context.primary().map_or("", |entry| &entry.key)))
            .collect();
        let run = RunCommand(Invocation::of(command).unwrap().context(context));
        window.dispatch_action(Box::new(run), cx);
    });
    draw(cx);
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

/// The labels of the recorded nodes of `role`, in draw order.
fn labelled(nodes: &[Node], role: Role) -> Vec<String> {
    (nodes.iter())
        .filter(|n| n.role == role)
        .map(|n| n.label.to_string())
        .collect()
}

/// The recorded options' labels, and which is selected.
fn options(nodes: &[Node]) -> (Vec<String>, Vec<String>) {
    let options: Vec<&Node> = (nodes.iter())
        .filter(|n| n.role == Role::ListBoxOption)
        .collect();
    let labels = options.iter().map(|n| n.label.to_string()).collect();
    let selected = (options.iter())
        .filter(|n| n.states.selected == Some(true))
        .map(|n| n.label.to_string())
        .collect();
    (labels, selected)
}

/// What the window's live region says now.
fn announced(nodes: &[Node]) -> Option<String> {
    (nodes.iter().rev())
        .find(|n| n.role == Role::Status)
        .and_then(|n| n.states.value.as_ref().map(ToString::to_string))
}

/// Whether the palette matches `title` to `query`: its letters in order.
fn matches(query: &str, title: &str) -> bool {
    let mut title = title.chars().flat_map(char::to_lowercase);
    query.chars().all(|want| title.any(|ch| ch == want))
}

/// The command palette showing, if it is.
fn palette(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<Picker<CommandPalette>>> {
    workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        layer.modal_view()?.clone().downcast().ok()
    })
}

/// The selected match's label, as the palette's delegate gives it.
fn selected_label(picker: &Entity<Picker<CommandPalette>>, cx: &mut VisualTestContext) -> String {
    picker.read_with(cx, |picker, cx| {
        picker
            .delegate
            .match_label(picker.selected(), cx)
            .to_string()
    })
}

#[gpui::test]
fn the_command_palette_is_a_dialog_with_a_search_input_and_options(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&["a.txt"], cx);
    run("command_palette.toggle", cx);
    let picker = palette(&workbench, cx).expect("the palette shows");
    let nodes = drawn(cx);

    assert_eq!(labelled(&nodes, Role::Dialog), ["Show All Commands"]);
    assert!(
        labelled(&nodes, Role::SearchInput).contains(&"Run a command…".to_string()),
        "the query is a search field named by its placeholder: {:?}",
        labelled(&nodes, Role::SearchInput)
    );
    assert_eq!(labelled(&nodes, Role::ListBox), ["Show All Commands"]);
    let (labels, selected) = options(&nodes);
    assert!(labels.len() > 1, "the visible rows are options: {labels:?}");
    let first = selected_label(&picker, cx);
    assert_eq!(
        selected,
        std::slice::from_ref(&first),
        "the first match is selected"
    );
    assert_eq!(labels[0], first);
    let id = picker.read_with(cx, |picker, _| picker.delegate.match_id(0).to_string());
    assert_ne!(first, id, "an option reads the command's title, not its id");
    assert_eq!(
        announced(&nodes).as_deref(),
        Some(first.as_str()),
        "opening says the selected match"
    );

    // The query refilters: the selection, and what's said, go with it.
    assert!(!matches("quit", &first), "{first}");
    cx.simulate_input("quit");
    let nodes = drawn(cx);
    let top = selected_label(&picker, cx);
    assert!(matches("quit", &top), "{top}");
    let (labels, selected) = options(&nodes);
    assert_eq!(selected, std::slice::from_ref(&top));
    assert_eq!(labels[0], top);
    assert!(labels.len() > 1, "{labels:?}");
    assert!(labels.iter().all(|l| matches("quit", l)), "{labels:?}");
    assert_eq!(announced(&nodes).as_deref(), Some(top.as_str()));

    // A key moves the selection: the next option is selected and said.
    press("down", cx);
    let nodes = drawn(cx);
    let next = selected_label(&picker, cx);
    assert_ne!(next, top);
    assert_eq!(options(&nodes).1, std::slice::from_ref(&next));
    assert_eq!(announced(&nodes).as_deref(), Some(next.as_str()));
    press("up", cx);
    let nodes = drawn(cx);
    assert_eq!(options(&nodes).1, std::slice::from_ref(&top));
    assert_eq!(announced(&nodes).as_deref(), Some(top.as_str()));

    // Focus stays in the query, the rows' sibling: no row claims to be
    // the active descendant (section 6, Q2).
    assert_eq!(cx.debug_bounds(RowState::ACTIVE_DESCENDANT), None);
}

#[gpui::test]
fn quick_open_is_a_dialog_named_by_its_placeholder(cx: &mut TestAppContext) {
    let (_dir, _workbench, cx) = opened(&["alpha.txt", "beta.txt"], cx);
    run("quick_open.toggle", cx);
    cx.simulate_input("beta");
    let nodes = drawn(cx);
    assert_eq!(labelled(&nodes, Role::Dialog), ["Go to file…"]);
    assert!(labelled(&nodes, Role::SearchInput).contains(&"Go to file…".to_string()));
    assert_eq!(labelled(&nodes, Role::ListBox), ["Go to file…"]);
    let (labels, selected) = options(&nodes);
    assert_eq!(labels, ["beta.txt"]);
    assert_eq!(selected, ["beta.txt"]);
    assert_eq!(announced(&nodes).as_deref(), Some("beta.txt"));
}

/// Where the active descendant's row is drawn, by the title its row shows.
fn active_row(titles: &[&'static str], cx: &mut VisualTestContext) -> Option<&'static str> {
    let active: Bounds<Pixels> = cx.debug_bounds(RowState::ACTIVE_DESCENDANT)?;
    titles.iter().copied().find(|title| {
        let selector: &'static str =
            Box::leak(format!("tab-switcher-row:{title}").into_boxed_str());
        cx.debug_bounds(selector)
            .is_some_and(|row| row.top() == active.top() && row.bottom() == active.bottom())
    })
}

#[gpui::test]
fn the_tab_switcher_makes_the_selected_tab_the_active_descendant(cx: &mut TestAppContext) {
    let titles = ["a.txt", "b.txt", "c.txt"];
    let (_dir, _workbench, cx) = opened(&titles, cx);
    cx.simulate_modifiers_change(Modifiers::control());
    draw(cx);
    press("ctrl-tab", cx);
    let nodes = drawn(cx);
    assert_eq!(labelled(&nodes, Role::Dialog), ["Switch Tab"]);
    assert_eq!(labelled(&nodes, Role::ListBox), ["Switch Tab"]);
    // In order of use, the one before the shown tab selected.
    let (labels, selected) = options(&nodes);
    assert_eq!(labels, ["c.txt", "b.txt", "a.txt"]);
    assert_eq!(selected, ["b.txt"]);
    assert_eq!(active_row(&titles, cx), Some("b.txt"));

    press("ctrl-tab", cx);
    let nodes = drawn(cx);
    assert_eq!(options(&nodes).1, ["a.txt"]);
    assert_eq!(
        active_row(&titles, cx),
        Some("a.txt"),
        "it follows the press"
    );
    press("ctrl-tab", cx);
    drawn(cx);
    assert_eq!(active_row(&titles, cx), Some("c.txt"), "and wraps");

    cx.simulate_modifiers_change(Modifiers::none());
    draw(cx);
    assert_eq!(cx.debug_bounds(RowState::ACTIVE_DESCENDANT), None, "gone");
}

#[gpui::test]
fn the_find_bar_is_a_search_landmark_with_named_fields_and_buttons(cx: &mut TestAppContext) {
    let (_dir, _workbench, cx) = opened(&["a.txt"], cx);
    run("find.show", cx);
    run("find.show_replace", cx);
    let nodes = drawn(cx);
    assert_eq!(labelled(&nodes, Role::Search), ["Find"]);
    assert_eq!(labelled(&nodes, Role::SearchInput), ["Find"]);
    assert!(labelled(&nodes, Role::TextInput).contains(&"Replace".to_string()));
    let buttons = labelled(&nodes, Role::Button);
    for name in [
        "Toggle Replace",
        "Match Case",
        "Match Whole Word",
        "Use Regular Expression",
        "Previous Match",
        "Next Match",
        "Find in Selection",
        "Preserve Case",
        "Replace",
        "Replace All",
    ] {
        assert!(buttons.contains(&name.to_string()), "{name} in {buttons:?}");
    }
    // Every control the recorder saw has a name.
    for node in &nodes {
        if matches!(
            node.role,
            Role::Button | Role::SearchInput | Role::TextInput | Role::Search
        ) {
            assert!(!node.label.trim().is_empty(), "{node:?} is named");
        }
    }

    // A toggle says whether it's on.
    run("find.toggle_case", cx);
    let nodes = drawn(cx);
    let case = (nodes.iter())
        .find(|n| n.role == Role::Button && n.label == "Match Case")
        .unwrap();
    assert_eq!(case.states.toggled, Some(true));
}

#[gpui::test]
fn options_are_named_by_their_text_not_their_ids(cx: &mut TestAppContext) {
    let (_dir, _workbench, cx) = opened(&["a.txt", "b.txt"], cx);

    // The buffer picker's ids are entity and tab ids.
    run("buffers.show", cx);
    let nodes = drawn(cx);
    let (labels, selected) = options(&nodes);
    assert_eq!(labels.len(), 2, "{labels:?}");
    for name in ["a.txt", "b.txt"] {
        assert!(
            labels.iter().any(|l| l.starts_with(name)),
            "{name} in {labels:?}"
        );
    }
    assert_eq!(selected.len(), 1);
    assert_eq!(announced(&nodes), selected.first().cloned());
    press("escape", cx);

    // Plain Text's id is empty.
    run("language.select", cx);
    let (labels, _) = options(&drawn(cx));
    assert!(labels[0].starts_with("Plain Text"), "{labels:?}");
    assert!(labels.iter().all(|l| !l.trim().is_empty()), "{labels:?}");
}

#[gpui::test]
fn a_hidden_picker_says_nothing(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&["a.txt"], cx);
    run("command_palette.toggle", cx);
    let picker = palette(&workbench, cx).expect("the palette shows");
    cx.simulate_input("quit");
    let said = announced(&drawn(cx)).expect("the selection is said");

    // Dismissed, its matches still change and its selection moves, as a
    // picker kept for its next showing hears late results: nothing is said.
    press("escape", cx);
    assert!(palette(&workbench, cx).is_none());
    picker.update(cx, |picker, cx| {
        picker
            .input()
            .update(cx, |input, cx| input.set_text("save", cx));
    });
    draw(cx);
    picker.update(cx, |picker, cx| picker.select(1, cx));
    let nodes = drawn(cx);
    assert_ne!(selected_label(&picker, cx), said);
    assert_eq!(announced(&nodes), Some(said.clone()), "nothing new is said");
}

#[gpui::test]
fn a_picker_in_an_inactive_window_says_nothing(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&["a.txt"], cx);
    run("command_palette.toggle", cx);
    let picker = palette(&workbench, cx).expect("the palette shows");
    let said = announced(&drawn(cx)).expect("the selection is said");

    cx.deactivate_window();
    draw(cx);
    picker.update(cx, |picker, cx| picker.select(2, cx));
    let nodes = drawn(cx);
    assert_ne!(selected_label(&picker, cx), said);
    assert_eq!(announced(&nodes), Some(said), "nothing new is said");
}
