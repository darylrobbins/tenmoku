//! The Keyboard Shortcuts tab's recorder in a headless window: opening it
//! on a line, the keys it records (bound ones too, held from the keymap),
//! its chords, labels and conflict preview, its When field, and the writes
//! Enter makes to the user's keymap, with the tab following the reload.
//! On the parent module's workbench, config directory and keymap store.

use std::time::Duration;

use fe_keymap::Entry;
use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext};

use super::*;
use crate::shortcuts_editor::recorder::{CHORD_WINDOW, PROMPT};

/// The open recorder: its label, conflict preview and When error.
#[derive(Debug, PartialEq, Eq)]
struct Recording {
    label: String,
    conflict: Option<String>,
    when_error: Option<String>,
}

fn recording(editor: &Entity<ShortcutsEditor>, cx: &mut VisualTestContext) -> Option<Recording> {
    editor.read_with(cx, |editor, _| {
        editor.recorder().map(|recorder| Recording {
            label: recorder.label(),
            conflict: recorder.conflict().map(str::to_owned),
            when_error: recorder.when_error().map(str::to_owned),
        })
    })
}

/// `written`'s label, as menus print it on this platform.
fn label(written: &str) -> String {
    fe_keymap::shortcut_label(&KeySequence::parse_for(written, Os::current()).unwrap())
}

fn press(keystrokes: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keystrokes);
    draw(cx);
}

/// Clicks `selector`'s element, `count` clicks in a row.
fn click(selector: &str, count: usize, cx: &mut VisualTestContext) {
    let at = cx
        .debug_bounds(selector.to_owned().leak())
        .unwrap_or_else(|| panic!("{selector} isn't drawn"))
        .center();
    for click_count in 1..=count {
        cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
        });
    }
    draw(cx);
}

fn recorder_focused(editor: &Entity<ShortcutsEditor>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        (editor.read(cx).recorder()).is_some_and(|recorder| recorder.focus().is_focused(window))
    })
}

/// Every tab's title in the active pane.
fn tab_titles(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane().read(cx).tabs().iter())
            .map(|tab| tab.item().tab_title(cx).to_string())
            .collect()
    })
}

/// The user keymap's entries, as read on this platform.
fn entries(dirs: &Dirs) -> Vec<Entry> {
    let text = dirs.read_keymap();
    let (layer, diagnostics) = KeymapLayer::parse_for(&dirs.keymap(), &text, None, Os::current());
    assert_eq!(diagnostics, [], "{text}");
    layer.unwrap().entries
}

/// An entry's keys, context and command as written.
fn entry(entry: &Entry) -> (String, Option<String>, Option<String>) {
    (
        entry.keys.to_string(),
        entry.context.as_ref().map(ToString::to_string),
        entry.command.as_ref().map(ToString::to_string),
    )
}

/// The When field's text.
fn when_text(editor: &Entity<ShortcutsEditor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, cx| {
        let recorder = editor.recorder().expect("recording");
        let when = recorder.when_input().expect("a When field");
        when.read(cx).text().to_owned()
    })
}

/// Replaces the When field's text with `text`, typed.
fn type_when(editor: &Entity<ShortcutsEditor>, text: &str, cx: &mut VisualTestContext) {
    let when = editor.read_with(cx, |editor, _| {
        (editor.recorder().and_then(|recorder| recorder.when_input())).cloned()
    });
    let when = when.expect("a When field");
    cx.update(|window, cx| {
        when.update(cx, |when, cx| when.clear(cx));
        window.focus(&when.focus_handle(cx), cx);
    });
    cx.simulate_input(text);
    draw(cx);
}

/// The command's context as the tab's row has it.
fn row_context(
    editor: &Entity<ShortcutsEditor>,
    command: &str,
    cx: &mut VisualTestContext,
) -> Option<String> {
    editor.read_with(cx, |editor, _| {
        (0..editor.lines().len())
            .filter_map(|ix| editor.line(ix))
            .find(|(row, _)| row.command == command)
            .and_then(|(row, _)| row.context.clone())
    })
}

#[gpui::test]
fn recording_captures_bound_keys_without_running_them(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    // The JSON on a platform-modifier key, which only the recorder's
    // holding every key keeps from the keymap.
    dirs.write_keymap(&format!(
        "[[bindings]]\nkeys = \"cmd-alt-9\"\ncommand = \"{OPEN_JSON}\"\ncontext = \"workbench\"\n"
    ));
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    let tabs = tab_titles(&workbench, cx);
    search("open keyboard shortcuts", cx);

    // Enter on the selected line opens the recorder in its Keybinding
    // cell, focused, prompting.
    click(&format!("shortcuts-row-{OPEN}"), 1, cx);
    assert_eq!(recording(&editor, cx), None);
    press("enter", cx);
    assert!(drawn("shortcuts-recorder", cx));
    assert!(recorder_focused(&editor, cx));
    assert_eq!(recording(&editor, cx).unwrap().label, "");
    assert!(!PROMPT.is_empty());

    // The keys of Close Tab are recorded; the tab stays.
    press(&keys("secondary-w"), cx);
    assert_eq!(recording(&editor, cx).unwrap().label, label("secondary-w"));
    assert_eq!(tab_titles(&workbench, cx), tabs);

    // So are the JSON's, on the platform modifier: nothing opens.
    cx.executor()
        .advance_clock(CHORD_WINDOW + Duration::from_millis(100));
    press("cmd-alt-9", cx);
    assert_eq!(recording(&editor, cx).unwrap().label, label("cmd-alt-9"));
    assert_eq!(tab_titles(&workbench, cx), tabs);
    assert!(recorder_focused(&editor, cx));

    // Once it's closed the keys run again.
    press("escape", cx);
    assert_eq!(recording(&editor, cx), None);
    press("cmd-alt-9", cx);
    assert_ne!(tab_titles(&workbench, cx), tabs, "the JSON opens");
}

#[gpui::test]
fn chord_of_two(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("open keyboard shortcuts", cx);

    // A double click opens it on the line.
    click(&format!("shortcuts-row-{OPEN}"), 2, cx);
    assert!(recorder_focused(&editor, cx));

    // A second keystroke soon after extends the first.
    press(&keys("secondary-k"), cx);
    cx.executor()
        .advance_clock(CHORD_WINDOW - Duration::from_millis(100));
    press(&keys("secondary-t"), cx);
    assert_eq!(
        recording(&editor, cx).unwrap().label,
        label("secondary-k secondary-t")
    );

    // One later than that starts again.
    press("escape", cx);
    click(&format!("shortcuts-row-{OPEN}"), 2, cx);
    press(&keys("secondary-k"), cx);
    cx.executor()
        .advance_clock(CHORD_WINDOW + Duration::from_millis(100));
    press(&keys("secondary-t"), cx);
    assert_eq!(recording(&editor, cx).unwrap().label, label("secondary-t"));
}

#[gpui::test]
fn third_key_starts_a_new_sequence(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("open keyboard shortcuts", cx);

    // The line's pencil opens it.
    click(&format!("shortcuts-edit-{OPEN}"), 1, cx);
    assert!(recorder_focused(&editor, cx));
    press("ctrl-a ctrl-b", cx);
    assert_eq!(
        recording(&editor, cx).unwrap().label,
        label("ctrl-a ctrl-b")
    );
    press("ctrl-c", cx);
    assert_eq!(recording(&editor, cx).unwrap().label, label("ctrl-c"));
    press("ctrl-d", cx);
    assert_eq!(
        recording(&editor, cx).unwrap().label,
        label("ctrl-c ctrl-d")
    );
}

#[gpui::test]
fn label_matches_shortcut_label(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("open keyboard shortcuts", cx);
    click(&format!("shortcuts-row-{OPEN}"), 2, cx);
    for written in [
        "secondary-shift-k",
        "alt-f5",
        "cmd-ctrl-left",
        "shift-tab",
        "ctrl-enter",
        "backspace",
    ] {
        cx.executor().advance_clock(CHORD_WINDOW * 2);
        press(&keys(written), cx);
        assert_eq!(
            recording(&editor, cx).unwrap().label,
            label(written),
            "{written}"
        );
    }
}

#[gpui::test]
fn conflict_preview_lists_other_commands(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("keyboard shortcuts json", cx);

    // The JSON has no keys: Add opens it in its context.
    click(&format!("shortcuts-add-{OPEN_JSON}"), 1, cx);
    assert!(recorder_focused(&editor, cx));
    press(&keys("secondary-alt-8"), cx);
    assert_eq!(recording(&editor, cx).unwrap().conflict, None);
    assert!(!drawn("shortcuts-recorder-conflict", cx));

    // Close Tab's keys: the preview names it, and not the chords they
    // start (`secondary-w h`, a pane's), nor the command itself.
    cx.executor().advance_clock(CHORD_WINDOW * 2);
    press(&keys("secondary-w"), cx);
    let conflict = recording(&editor, cx).unwrap().conflict.unwrap();
    let also = conflict.strip_prefix("Also bound to ").expect(&conflict);
    let also: Vec<&str> = also.split(", ").collect();
    assert!(also.contains(&"workbench.close_tab"), "{conflict}");
    assert!(
        !also
            .iter()
            .any(|id| id.starts_with("pane.") || *id == OPEN_JSON),
        "{conflict}"
    );
    assert!(drawn("shortcuts-recorder-conflict", cx));

    // Only where the binding would apply: an editor's keys clash in an
    // editor, not in the project panel.
    cx.executor().advance_clock(CHORD_WINDOW * 2);
    press(&keys("secondary-d"), cx);
    type_when(&editor, "editor", cx);
    let conflict = recording(&editor, cx).unwrap().conflict.unwrap_or_default();
    assert!(
        conflict.contains("editor.select_next_occurrence"),
        "{conflict}"
    );
    type_when(&editor, "project_panel", cx);
    assert_eq!(recording(&editor, cx).unwrap().conflict, None);

    // Back in the command's context, a click on the box records again.
    let context = row_context(&editor, OPEN_JSON, cx);
    type_when(&editor, context.as_deref().unwrap_or_default(), cx);
    click("shortcuts-recorder", 1, cx);
    assert!(recorder_focused(&editor, cx));
    cx.executor().advance_clock(CHORD_WINDOW * 2);
    press(&keys("secondary-w"), cx);
    let conflict = recording(&editor, cx).unwrap().conflict.unwrap_or_default();
    assert!(conflict.contains("workbench.close_tab"), "{conflict}");

    // Saving isn't blocked.
    press("enter", cx);
    assert_eq!(recording(&editor, cx), None);
    let written: Vec<_> = entries(&dirs).iter().map(entry).collect();
    assert_eq!(
        written,
        [(keys("secondary-w"), context, Some(OPEN_JSON.to_owned()))]
    );
}

#[gpui::test]
fn enter_adds_binding(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("keyboard shortcuts json", cx);
    let context = row_context(&editor, OPEN_JSON, cx);
    click(&format!("shortcuts-add-{OPEN_JSON}"), 1, cx);
    press(&keys("secondary-alt-8"), cx);
    press("enter", cx);
    assert_eq!(recording(&editor, cx), None);
    let list_focused = cx.update(|window, cx| editor.read(cx).list_focus().is_focused(window));
    assert!(list_focused, "the list has focus again");

    // An entry after the header, in the command's context.
    let text = dirs.read_keymap();
    assert!(text.starts_with(KEYMAP_HEADER), "{text}");
    let written: Vec<_> = entries(&dirs).iter().map(entry).collect();
    assert_eq!(
        written,
        [(keys("secondary-alt-8"), context, Some(OPEN_JSON.to_owned()))]
    );
}

#[gpui::test]
fn enter_changes_binding(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    dirs.write_keymap(&format!(
        "# Mine.\n\n# The JSON on a key.\n[[bindings]]\nkeys = \"secondary-alt-9\"\n\
         command = \"{OPEN_JSON}\"\ncontext = \"workbench\"\n"
    ));
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("keyboard shortcuts json", cx);

    // A user entry: its keys are rewritten, its comment kept. A changed
    // binding has no When field.
    click(&format!("shortcuts-edit-{OPEN_JSON}"), 1, cx);
    assert!(editor.read_with(cx, |editor, _| {
        editor.recorder().unwrap().when_input().is_none()
    }));
    assert!(!drawn("shortcuts-recorder-when", cx));
    press(&keys("secondary-alt-7"), cx);
    press("enter", cx);
    let text = dirs.read_keymap();
    assert!(
        text.starts_with("# Mine.\n\n# The JSON on a key.\n"),
        "{text}"
    );
    let written: Vec<_> = entries(&dirs).iter().map(entry).collect();
    assert_eq!(
        written,
        [(
            keys("secondary-alt-7"),
            Some("workbench".into()),
            Some(OPEN_JSON.to_owned())
        )]
    );

    // A default: unbound and bound again at the new keys.
    reload(&dirs, cx);
    set_search(&editor, "open keyboard shortcuts", cx);
    click(&format!("shortcuts-edit-{OPEN}"), 1, cx);
    press(&keys("secondary-alt-6"), cx);
    press("enter", cx);
    let written: Vec<_> = entries(&dirs).iter().map(entry).collect();
    assert_eq!(
        written[1..],
        [
            (
                keys("secondary-k secondary-s"),
                Some("workbench".into()),
                None
            ),
            (
                keys("secondary-alt-6"),
                Some("workbench".into()),
                Some(OPEN.to_owned())
            ),
        ]
    );
}

#[gpui::test]
fn escape_writes_nothing(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("open keyboard shortcuts", cx);
    click(&format!("shortcuts-row-{OPEN}"), 2, cx);

    // A modifier alone records nothing, and Enter with nothing recorded
    // saves nothing.
    cx.simulate_modifiers_change(Modifiers::shift());
    cx.simulate_modifiers_change(Modifiers::none());
    press("shift", cx);
    press("control", cx);
    assert_eq!(recording(&editor, cx).unwrap().label, "");
    press("enter", cx);
    assert!(recording(&editor, cx).is_some(), "still recording");

    // Escape after keys: closed, nothing written.
    press(&keys("secondary-alt-8"), cx);
    press("escape", cx);
    assert_eq!(recording(&editor, cx), None);
    assert!(!drawn("shortcuts-recorder", cx));
    assert!(!dirs.keymap().exists());

    // From a new binding's When field too.
    set_search(&editor, "keyboard shortcuts json", cx);
    click(&format!("shortcuts-add-{OPEN_JSON}"), 1, cx);
    press(&keys("secondary-alt-8"), cx);
    type_when(&editor, "workbench", cx);
    press("escape", cx);
    assert_eq!(recording(&editor, cx), None);
    assert!(!dirs.keymap().exists());

    // A click on another line closes it too.
    set_search(&editor, "keyboard shortcuts", cx);
    click(&format!("shortcuts-add-{OPEN_JSON}"), 1, cx);
    press(&keys("secondary-alt-8"), cx);
    click(&format!("shortcuts-row-{OPEN}"), 1, cx);
    assert_eq!(recording(&editor, cx), None);
    assert!(!dirs.keymap().exists());

    // So does focus going elsewhere, as to the search; but not from the
    // box to its When field and back.
    click(&format!("shortcuts-add-{OPEN_JSON}"), 1, cx);
    press(&keys("secondary-alt-8"), cx);
    type_when(&editor, "workbench", cx);
    click("shortcuts-recorder", 1, cx);
    assert!(recorder_focused(&editor, cx));
    assert_eq!(
        recording(&editor, cx).unwrap().label,
        label("secondary-alt-8")
    );
    let search = editor.read_with(cx, |editor, _| editor.search_input().clone());
    cx.update(|window, cx| window.focus(&search.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(recording(&editor, cx), None);
    assert!(!drawn("shortcuts-recorder", cx));
    assert!(search_focused(&editor, cx), "focus stays where it went");
    assert!(!dirs.keymap().exists());
}

#[gpui::test]
fn invalid_when_blocks_save(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("keyboard shortcuts json", cx);
    click(&format!("shortcuts-add-{OPEN_JSON}"), 1, cx);
    press(&keys("secondary-alt-8"), cx);
    assert!(drawn("shortcuts-recorder-when", cx));

    // A predicate that doesn't parse: why, under the field, and Enter
    // does nothing.
    type_when(&editor, "editor &&", cx);
    let error = recording(&editor, cx).unwrap().when_error;
    let expected = fe_command::ContextPredicate::parse("editor &&").unwrap_err();
    assert_eq!(error, Some(expected.to_string()));
    assert!(drawn("shortcuts-recorder-when-error", cx));
    press("enter", cx);
    assert!(recording(&editor, cx).is_some());
    assert!(!dirs.keymap().exists());

    // Fixed, Enter in the field saves with it.
    type_when(&editor, "editor && !vim.enabled", cx);
    assert_eq!(recording(&editor, cx).unwrap().when_error, None);
    assert!(!drawn("shortcuts-recorder-when-error", cx));
    press("enter", cx);
    assert_eq!(recording(&editor, cx), None);
    let written: Vec<_> = entries(&dirs).iter().map(entry).collect();
    assert_eq!(
        written,
        [(
            keys("secondary-alt-8"),
            Some("editor && !vim.enabled".into()),
            Some(OPEN_JSON.to_owned())
        )]
    );
}

#[gpui::test]
fn new_binding_defaults_to_command_context(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);

    // A command with a context of its own, and a binding: Add Keybinding
    // in its menu adds another.
    let (command, context) = editor.read_with(cx, |editor, _| {
        (0..editor.lines().len())
            .filter_map(|ix| editor.line(ix))
            .find_map(|(row, binding)| {
                let context = row.context.clone()?;
                binding.is_some().then(|| (row.command.clone(), context))
            })
            .expect("a bound command with a context")
    });
    set_search(&editor, &format!("@command:{command}"), cx);
    right_click(&format!("shortcuts-row-{command}"), cx);
    click_menu_item(MenuItem::AddKeybinding, cx);
    assert!(recorder_focused(&editor, cx));
    assert_eq!(when_text(&editor, cx), context);
    press(&keys("secondary-alt-8"), cx);
    press("enter", cx);
    let written: Vec<_> = entries(&dirs).iter().map(entry).collect();
    let printed = fe_command::ContextPredicate::parse(&context)
        .unwrap()
        .to_string();
    assert_eq!(
        written,
        [(
            keys("secondary-alt-8"),
            Some(printed),
            Some(command.clone())
        )]
    );
}

#[gpui::test]
fn saved_binding_shows_after_reload(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let text = "# Mine.\n\n# Nothing yet.\n";
    dirs.write_keymap(text);
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("keyboard shortcuts json", cx);
    click(&format!("shortcuts-add-{OPEN_JSON}"), 1, cx);
    press(&keys("secondary-alt-8"), cx);
    press("enter", cx);

    // The file keeps its comments.
    let written = dirs.read_keymap();
    assert!(written.starts_with(text), "{written}");

    // Reloaded, the line has the keys, from the user's file.
    reload(&dirs, cx);
    let json = lines_of(&editor, OPEN_JSON, cx);
    assert_eq!(
        json,
        [Shown {
            command: OPEN_JSON.into(),
            keys: Some(keys("secondary-alt-8")),
            when: row_context(&editor, OPEN_JSON, cx),
            source: Some(ShortcutSource::User),
        }]
    );
    assert!(drawn(&format!("shortcuts-edit-{OPEN_JSON}"), cx));
}

/// Scrolls the list by `dy` pixels, with the wheel over it.
fn scroll_list(dy: f32, cx: &mut VisualTestContext) {
    let at = cx.debug_bounds("shortcuts-list").unwrap().center();
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: at,
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(dy))),
        ..Default::default()
    });
    draw(cx);
}

#[gpui::test]
fn scrolling_the_line_away_closes_the_recorder(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    dirs.write_keymap(&format!(
        "[[bindings]]\nkeys = \"cmd-alt-9\"\ncommand = \"{OPEN_JSON}\"\ncontext = \"workbench\"\n"
    ));
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    let tabs = tab_titles(&workbench, cx);

    // Recording on the first line, which then scrolls out of the list:
    // the box that held the keys goes, and the recorder closes with it.
    let first = editor.read_with(cx, |editor, _| editor.line(0).unwrap().0.command.clone());
    click(&format!("shortcuts-row-{first}"), 2, cx);
    assert!(recorder_focused(&editor, cx));
    press(&keys("secondary-alt-8"), cx);
    scroll_list(-100_000., cx);
    assert!(!drawn(&format!("shortcuts-row-{first}"), cx));
    assert_eq!(recording(&editor, cx), None);
    assert_eq!(entries(&dirs).len(), 1, "nothing written");

    // Keys run again, rather than going nowhere.
    press("cmd-alt-9", cx);
    assert_ne!(tab_titles(&workbench, cx), tabs, "the JSON opens");

    // Back on the line, no recorder is shown.
    run(OPEN, cx);
    scroll_list(100_000., cx);
    assert!(drawn(&format!("shortcuts-row-{first}"), cx));
    assert!(!drawn("shortcuts-recorder", cx));
    assert_eq!(recording(&editor, cx), None);
}

#[gpui::test]
fn conflict_preview_keeps_a_changed_bindings_args(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    // vim's Left motion on another key too, in normal mode.
    let context = "editor && vim.mode == normal";
    dirs.write_keymap(&format!(
        "[[bindings]]\nkeys = \"ctrl-alt-1\"\ncommand = \"vim.move\"\n\
         args = {{ motion = \"left\" }}\ncontext = \"{context}\"\n"
    ));
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    set_search(&editor, "@command:vim.move", cx);
    let ix = line_ix(&editor, "vim.move", Some(&keys("ctrl-alt-1")), cx);
    let nth = editor.read_with(cx, |editor, _| editor.lines()[ix].binding.unwrap());
    let pencil = match nth {
        0 => "shortcuts-edit-vim.move".to_owned(),
        nth => format!("shortcuts-edit-vim.move-{nth}"),
    };
    click(&pencil, 1, cx);
    assert!(recorder_focused(&editor, cx));

    // Moved to Left, where the default runs the same motion: no clash
    // with vim.move, which runs the same either way.
    press("left", cx);
    let conflict = recording(&editor, cx).unwrap().conflict.unwrap_or_default();
    assert!(!conflict.contains("vim.move"), "{conflict}");

    // Moved to Right, the other motion's: it clashes.
    cx.executor().advance_clock(CHORD_WINDOW * 2);
    press("right", cx);
    let conflict = recording(&editor, cx).unwrap().conflict.unwrap_or_default();
    assert!(conflict.contains("vim.move"), "{conflict}");
}
