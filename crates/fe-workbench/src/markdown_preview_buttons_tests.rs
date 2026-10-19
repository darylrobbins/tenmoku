//! The tab bar's Markdown preview buttons, as VS Code's editor title bar
//! has them: shown only for a Markdown tab, before the split buttons. The
//! toggle swaps the tab's editor for its preview in the same tab and back;
//! the other opens the preview to the side, as
//! `markdown.show_preview_to_side` does. `markdown.toggle_preview` runs from
//! the editor and from the preview shown in its place, which carries the
//! `markdown_preview` key context.

use std::{fs, path::Path, process::Command, sync::Arc, time::Duration};

use fe_command::{CommandId, ContextFrame, ContextStack, Invocation, Source, Value};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_ui::Icon;
use gpui::{Bounds, Entity, Focusable as _, Modifiers, Pixels, TestAppContext, VisualTestContext};
use kiln_shell::{
    Picker,
    command_palette::{COMMAND_PALETTE, CommandPalette},
};

use crate::{
    Launch, Pane, Workbench,
    loader::test_support,
    pane::Reveal,
    tests::open,
    workbench_commands::markdown_preview::{DELAY, MarkdownPreview},
};

const STANDARD: [&str; 3] = ["split-right", "split-down", "pane-menu"];

/// Forty paragraphs under a heading, so the editor has rows to scroll.
fn readme() -> String {
    let mut text = "# Readme\n\n".to_owned();
    for n in 0..40 {
        text += &format!("Paragraph {n}.\n\n");
    }
    text
}

/// A workbench rooted at a folder of `files`, with each opened in turn in
/// its pane, the last active and focused.
fn opened<'a>(
    files: &[(&str, &str)],
    cx: &'a mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        fs::write(dir.path().join(name), text).unwrap();
    }
    let root = fs::canonicalize(dir.path()).unwrap();
    let (workbench, cx) = open_in(&root, cx);
    for (name, _) in files {
        open_file(&workbench, &root.join(name), cx);
    }
    focus(&workbench, cx);
    (dir, workbench, cx)
}

fn open_in<'a>(
    root: &Path,
    cx: &'a mut TestAppContext,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    (workbench, cx)
}

fn open_file(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = pane(workbench, cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    draw(cx);
}

/// Focuses the workbench as the app does, which focuses its active item.
fn focus(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().into_iter())
            .cloned()
            .collect()
    })
}

fn activate(pane: &Entity<Pane>, ix: usize, cx: &mut VisualTestContext) {
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(ix, window, cx)));
    draw(cx);
}

fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).expect("an editor").clone()
    })
}

/// The preview shown in the place of `pane`'s active tab's editor.
fn in_place(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Option<Entity<MarkdownPreview>> {
    pane.read_with(cx, |pane, _| {
        pane.tabs()[pane.active().unwrap()].preview().cloned()
    })
}

/// Each pane's tab titles and active tab.
fn layout(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(Vec<String>, usize)> {
    let panes = panes(workbench, cx);
    panes
        .iter()
        .map(|pane| {
            pane.read_with(cx, |pane, cx| {
                let titles = (pane.tabs().iter())
                    .map(|tab| tab.item().tab_title(cx).to_string())
                    .collect();
                (titles, pane.active().unwrap())
            })
        })
        .collect()
}

/// The previews open as tabs, in layout order.
fn preview_tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    let panes = panes(workbench, cx);
    panes
        .iter()
        .map(|pane| {
            pane.read_with(cx, |pane, _| {
                (pane.tabs().iter())
                    .filter(|tab| tab.item().downcast_ref::<MarkdownPreview>().is_some())
                    .count()
            })
        })
        .sum()
}

fn is_focused(handle: &impl gpui::Focusable, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| handle.focus_handle(cx).is_focused(window))
}

/// The IDs of the tab bar's buttons `pane` drew, in order.
fn strip(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Vec<&'static str> {
    pane.read_with(cx, |pane, _| {
        pane.drawn_actions().iter().map(|(id, _)| *id).collect()
    })
}

fn icon(pane: &Entity<Pane>, id: &str, cx: &mut VisualTestContext) -> Option<Icon> {
    pane.read_with(cx, |pane, _| {
        (pane.drawn_actions().iter())
            .find(|(drawn, _)| *drawn == id)
            .map(|(_, icon)| *icon)
    })
}

fn button(id: &str, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    // `debug_bounds` takes a `&'static str`.
    cx.debug_bounds(format!("icon-button:{id}").leak())
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let at = button(id, cx).unwrap_or_else(|| panic!("{id} is drawn"));
    cx.simulate_click(at.center(), Modifiers::none());
    draw(cx);
    draw(cx);
}

/// The tooltip of button `id` of `pane`, once hovered.
fn tooltip(pane: &Entity<Pane>, id: &str, cx: &mut VisualTestContext) -> Option<String> {
    let at = button(id, cx).expect("drawn");
    cx.simulate_mouse_move(at.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(500));
    draw(cx);
    let tooltip = pane.read_with(cx, |pane, _| pane.last_tooltip());
    // Off the bar, so the next hover starts afresh.
    let away = pane.read_with(cx, |pane, _| pane.content_bounds().center());
    cx.simulate_mouse_move(away, None, Modifiers::none());
    draw(cx);
    tooltip
}

/// Runs `command` as a key in `frames` within the workbench would.
fn run_in(command: &str, frames: &[&str], cx: &mut VisualTestContext) {
    let context: ContextStack = (["workbench"].iter().chain(frames))
        .map(|frame| ContextFrame::new(*frame))
        .collect();
    let run = RunCommand(Invocation::of(command).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn palette(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<Picker<CommandPalette>>> {
    workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        (layer.modal() == Some(COMMAND_PALETTE))
            .then(|| layer.modal_view().unwrap().clone().downcast().unwrap())
    })
}

/// Opens the palette where focus is, types `query` and returns the titles
/// it lists.
fn palette_titles(
    workbench: &Entity<Workbench>,
    query: &str,
    cx: &mut VisualTestContext,
) -> Vec<String> {
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    let picker = palette(workbench, cx).expect("the palette shows");
    cx.simulate_input(query);
    draw(cx);
    picker.read_with(cx, |picker, _| {
        (picker.delegate.matches().iter())
            .map(|found| found.title.to_string())
            .collect()
    })
}

fn last_run(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (String, Source) {
    workbench.read_with(cx, |workbench, _| {
        let run = workbench.commands_run.last().expect("a command ran");
        (run.command.to_string(), run.source)
    })
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

#[gpui::test]
fn buttons_show_only_for_markdown(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    // `big.md` opens in a viewer; README.md has a change to diff.
    test_support::lower_threshold(&root, 4096, false);
    fs::write(root.join("big.md"), "# Big\n".repeat(2000)).unwrap();
    fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("README.md"), "# Readme\n").unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("README.md"), "# Readme\n\nChanged.\n").unwrap();
    let (workbench, cx) = open_in(&root, cx);
    for name in ["main.rs", "README.md"] {
        open_file(&workbench, &root.join(name), cx);
    }
    focus(&workbench, cx);
    let pane = pane(&workbench, cx);

    // README.md: the toggle and the side button, before the split buttons,
    // in one row.
    let markdown = ["markdown-toggle", "markdown-side"];
    assert_eq!(strip(&pane, cx), [&markdown[..], &STANDARD[..]].concat());
    let bounds: Vec<_> = (markdown.iter().chain(&STANDARD))
        .map(|id| button(id, cx).unwrap_or_else(|| panic!("{id}")))
        .collect();
    for pair in bounds.windows(2) {
        assert!(pair[0].right() <= pair[1].left(), "{bounds:?}");
        assert_eq!(pair[0].top(), pair[1].top(), "{bounds:?}");
    }
    assert_eq!(icon(&pane, "markdown-toggle", cx), Some(Icon::Eye));
    assert_eq!(icon(&pane, "markdown-side", cx), Some(Icon::PanelRightOpen));
    let tips = markdown.map(|id| tooltip(&pane, id, cx));
    assert_eq!(
        tips,
        [
            Some("Open Preview".to_owned()),
            Some("Open Preview to the Side".to_owned())
        ]
    );

    let none = |pane: &Entity<Pane>, what: &str, cx: &mut VisualTestContext| {
        assert_eq!(strip(pane, cx), STANDARD, "{what}");
        for id in markdown {
            assert_eq!(button(id, cx), None, "{what}: {id}");
        }
    };
    // Another tab shown changes the strip on the next draw, both ways.
    activate(&pane, 0, cx);
    none(&pane, "main.rs", cx);
    activate(&pane, 1, cx);
    assert_eq!(strip(&pane, cx), [&markdown[..], &STANDARD[..]].concat());

    // A Markdown file too large to edit, in a viewer, has none.
    open_file(&workbench, &root.join("big.md"), cx);
    let viewer = pane.read_with(cx, |pane, _| pane.tabs()[2].viewer().is_some());
    assert!(viewer, "big.md opened in a viewer");
    none(&pane, "a viewer", cx);

    // Nor has a diff of README.md.
    activate(&pane, 1, cx);
    focus(&workbench, cx);
    run_in("git.diff_head", &[], cx);
    let title = workbench.read_with(cx, |workbench, cx| {
        let item = workbench.pane().read(cx).active_item().unwrap();
        item.tab_title(cx).to_string()
    });
    assert!(title.contains('↔'), "a diff tab is active: {title}");
    none(&pane, "a diff", cx);

    // An untitled buffer has none until the language picker makes it
    // Markdown.
    run_in("workbench.new_file", &[], cx);
    let untitled = active_editor(&workbench, cx);
    assert!(untitled.read_with(cx, |editor, _| editor.model().path().is_none()));
    none(&pane, "untitled", cx);
    run_in("language.select", &[], cx);
    cx.simulate_input("markdown");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    let language = untitled.read_with(cx, |editor, _| {
        editor.syntax().map(|syntax| syntax.language().name())
    });
    assert_eq!(language, Some("markdown"));
    assert_eq!(strip(&pane, cx), [&markdown[..], &STANDARD[..]].concat());
}

#[gpui::test]
fn toggle_swaps_the_body_in_the_same_tab(cx: &mut TestAppContext) {
    let files = [("notes.txt", "notes\n"), ("README.md", &readme()[..])];
    let (_dir, workbench, cx) = opened(&files, cx);
    let pane = pane(&workbench, cx);
    let editor = active_editor(&workbench, cx);
    // An edit to undo, a selection and a scroll to keep.
    cx.simulate_input("x");
    draw(cx);
    editor.update(cx, |editor, cx| {
        editor.model_mut().select_range(12..20, false);
        editor.set_scroll_top(5., cx);
    });
    draw(cx);
    let state = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            (editor.model().primary_selection(), editor.scroll_top())
        })
    };
    let before = state(cx);
    assert_ne!(before.1, 0., "the editor scrolled");
    let tab = |cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, cx| {
            let tab = &pane.tabs()[pane.active().unwrap()];
            (
                pane.tabs().len(),
                pane.active(),
                tab.item().tab_title(cx).to_string(),
                tab.path.clone(),
                tab.item().is_dirty(cx),
            )
        })
    };
    let source_tab = tab(cx);
    assert_eq!(&source_tab.2, "README.md");
    assert!(source_tab.4, "dirty");

    // The preview shows in the tab, focused; the tab is still the file's.
    click("markdown-toggle", cx);
    let preview = in_place(&pane, cx).expect("the preview shows in the tab");
    assert_eq!(tab(cx), source_tab);
    assert_eq!(layout(&workbench, cx).len(), 1);
    assert_eq!(preview_tabs(&workbench, cx), 0);
    assert_eq!(active_editor(&workbench, cx), editor);
    assert!(is_focused(&preview, cx));
    let source = preview.read_with(cx, |preview, _| preview.source().clone());
    assert_eq!(source, editor.downgrade());
    // Its document is drawn as the pane's body, scrolled to the source's
    // line.
    let body = pane.read_with(cx, |pane, _| pane.content_bounds());
    let block = cx.debug_bounds("md-block-0").expect("the preview is drawn");
    let across = body.left() <= block.left() && block.right() <= body.right();
    assert!(across, "{block:?} in {body:?}");
    assert_eq!(
        strip(&pane, cx),
        [&["markdown-toggle"][..], &STANDARD[..]].concat()
    );
    assert_eq!(icon(&pane, "markdown-toggle", cx), Some(Icon::Code));
    assert_eq!(button("markdown-side", cx), None);
    assert_eq!(
        tooltip(&pane, "markdown-toggle", cx).as_deref(),
        Some("Show Source")
    );
    assert_eq!(
        last_run(&workbench, cx),
        ("markdown.toggle_preview".to_owned(), Source::Mouse)
    );

    // Back: the same editor, as it was, focused, its history kept.
    click("markdown-toggle", cx);
    assert_eq!(in_place(&pane, cx), None);
    assert_eq!(cx.debug_bounds("md-block-0"), None);
    assert_eq!(active_editor(&workbench, cx), editor);
    assert_eq!(tab(cx), source_tab);
    assert_eq!(state(cx), before);
    assert!(is_focused(&editor, cx));
    assert_eq!(icon(&pane, "markdown-toggle", cx), Some(Icon::Eye));
    cx.simulate_keystrokes("cmd-z");
    draw(cx);
    let text = editor.read_with(cx, |editor, _| editor.model().text().to_string());
    assert_eq!(text, readme());

    // An edit to the source while the preview shows re-renders it once
    // edits settle.
    click("markdown-toggle", cx);
    let preview = in_place(&pane, cx).expect("shown again");
    let document = |cx: &mut VisualTestContext| {
        preview.read_with(cx, |preview, cx| preview.view().read(cx).document().clone())
    };
    let shown = document(cx);
    editor.update(cx, |editor, cx| {
        let end = editor.model().text().len();
        editor.model_mut().set_cursor(end);
        editor.model_mut().insert("# Added\n");
        cx.notify();
    });
    draw(cx);
    cx.executor()
        .advance_clock(DELAY - Duration::from_millis(1));
    draw(cx);
    assert!(Arc::ptr_eq(&document(cx), &shown), "re-rendered early");
    cx.executor().advance_clock(Duration::from_millis(1));
    draw(cx);
    let headings: Vec<String> = document(cx)
        .headings()
        .map(|(slug, _)| slug.to_owned())
        .collect();
    assert_eq!(headings, ["readme", "added"]);

    // A place revealed in the file, as a go to or a link opens it, shows
    // the text it's in.
    let path = editor.read_with(cx, |editor, _| editor.model().path().unwrap().to_owned());
    let reveal = Reveal {
        row: 2,
        columns: 0..0,
        within_row: true,
    };
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| pane.open_at(path, Some(reveal), window, cx))
    });
    draw(cx);
    assert_eq!(in_place(&pane, cx), None);
    assert!(is_focused(&editor, cx));
    assert_eq!(icon(&pane, "markdown-toggle", cx), Some(Icon::Eye));
}

#[gpui::test]
fn side_button_opens_the_split_preview(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&[("README.md", "# Readme\n")], cx);
    let left = pane(&workbench, cx);
    let source = active_editor(&workbench, cx);

    click("markdown-side", cx);
    assert_eq!(
        last_run(&workbench, cx),
        ("markdown.show_preview_to_side".to_owned(), Source::Mouse)
    );
    let titles = |titles: &[&str]| titles.iter().map(|title| title.to_string()).collect();
    let expected = vec![
        (titles(&["README.md"]), 0),
        (titles(&["Preview README.md"]), 0),
    ];
    assert_eq!(layout(&workbench, cx), expected);
    assert_eq!(pane(&workbench, cx), left);
    assert!(is_focused(&source, cx));
    assert_eq!(in_place(&left, cx), None);

    // Again: the same preview, not a second.
    click("markdown-side", cx);
    assert_eq!(layout(&workbench, cx), expected);
    assert_eq!(preview_tabs(&workbench, cx), 1);
    assert_eq!(active_editor(&workbench, cx), source);
    assert_eq!(in_place(&left, cx), None);
    assert!(is_focused(&source, cx));
}

#[gpui::test]
fn the_command_toggles_and_in_place_state_is_not_persisted(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let files = [("notes.txt", "notes\n"), ("README.md", "# Readme\n")];
    let (dir, workbench, cx) = opened(&files, cx);
    let pane = pane(&workbench, cx);
    let editor = active_editor(&workbench, cx);

    // Registered, with no key in the keymap the app runs.
    let id = CommandId::parse("markdown.toggle_preview").unwrap();
    assert_eq!(crate::command_catalog().check(&id, &Value::Null), Ok(()));
    let bound = cx.update(|_, cx| {
        let keymap = fe_gpui::current_keymap(cx);
        (keymap.bindings().iter()).any(|binding| binding.command == id)
    });
    assert!(!bound, "markdown.toggle_preview has no default key");

    // From the palette, in the editor, then in the preview shown, whose
    // `markdown_preview` context it applies in.
    let titles = palette_titles(&workbench, "markdown: toggle preview", cx);
    assert_eq!(
        titles.first().map(String::as_str),
        Some("Markdown: Toggle Preview")
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    let preview = in_place(&pane, cx).expect("toggled from the palette");
    assert!(is_focused(&preview, cx));
    assert_eq!(
        last_run(&workbench, cx),
        ("markdown.toggle_preview".to_owned(), Source::Palette)
    );
    let titles = palette_titles(&workbench, "markdown: toggle preview", cx);
    assert_eq!(
        titles.first().map(String::as_str),
        Some("Markdown: Toggle Preview")
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(in_place(&pane, cx), None);
    assert!(is_focused(&editor, cx));

    // As a key would run it in the preview, and only where it applies.
    run_in("markdown.toggle_preview", &["editor"], cx);
    let preview = in_place(&pane, cx).expect("toggled from the editor");
    run_in("markdown.toggle_preview", &[], cx);
    assert_eq!(
        in_place(&pane, cx),
        Some(preview.clone()),
        "not in a context"
    );
    run_in("markdown.toggle_preview", &["markdown_preview"], cx);
    assert_eq!(in_place(&pane, cx), None);

    // A tab that isn't Markdown is left alone.
    activate(&pane, 0, cx);
    run_in("markdown.toggle_preview", &["editor"], cx);
    assert_eq!(in_place(&pane, cx), None);
    assert_eq!(
        layout(&workbench, cx),
        [(vec!["notes.txt".to_owned(), "README.md".to_owned()], 0)]
    );
    activate(&pane, 1, cx);

    // A dirty tab shown as its preview asks before it closes, as any does.
    focus(&workbench, cx);
    cx.simulate_input("x");
    draw(cx);
    click("markdown-toggle", cx);
    assert!(in_place(&pane, cx).is_some());
    run_in("workbench.close_tab", &[], cx);
    let question = cx.pending_prompt().map(|(question, _)| question);
    assert_eq!(
        question.as_deref(),
        Some("Discard unsaved changes to README.md?")
    );
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert_eq!(pane.read_with(cx, |pane, _| pane.tabs().len()), 2);
    assert!(in_place(&pane, cx).is_some());

    // Saved while the preview shows, the layout restores the tab as the
    // editor.
    let entry = workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert!(!entry.to_string().contains("markdown_preview"), "{entry}");
    let root = fs::canonicalize(dir.path()).unwrap();
    let (restored, cx) = open_in(&root, &mut app);
    restored.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx)
    });
    draw(cx);
    let restored_pane = restored.read_with(cx, |workbench, _| workbench.pane().clone());
    let tabs = restored_pane.read_with(cx, |pane, cx| {
        (pane.tabs().iter())
            .map(|tab| {
                (
                    tab.item().tab_title(cx).to_string(),
                    tab.editor().is_some(),
                    tab.preview().is_some(),
                )
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        tabs,
        [
            ("notes.txt".to_owned(), true, false),
            ("README.md".to_owned(), true, false)
        ]
    );
    assert_eq!(strip(&restored_pane, cx)[0], "markdown-toggle");
    assert_eq!(icon(&restored_pane, "markdown-toggle", cx), Some(Icon::Eye));
}

#[gpui::test]
fn save_works_from_the_preview_shown_in_place(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(&[("README.md", "# Readme\n")], cx);
    let pane = pane(&workbench, cx);
    let editor = active_editor(&workbench, cx);
    cx.simulate_input("x");
    draw(cx);
    click("markdown-toggle", cx);
    let preview = in_place(&pane, cx).expect("shown");
    assert!(is_focused(&preview, cx));

    // Cmd-S saves the file, with the preview still shown and focused.
    cx.simulate_keystrokes("cmd-s");
    draw(cx);
    let saved = fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert_eq!(saved, "x# Readme\n");
    assert!(!editor.read_with(cx, |editor, _| editor.model().is_dirty()));
    assert_eq!(in_place(&pane, cx), Some(preview.clone()));
    assert!(is_focused(&preview, cx));

    // An untitled Markdown buffer asks where, as from its editor.
    run_in("workbench.new_file", &[], cx);
    let untitled = active_editor(&workbench, cx);
    untitled.update(cx, |editor, cx| editor.set_language(Some("markdown"), cx));
    draw(cx);
    cx.simulate_input("# New\n");
    draw(cx);
    click("markdown-toggle", cx);
    assert!(in_place(&pane, cx).is_some());
    cx.simulate_keystrokes("cmd-s");
    draw(cx);
    assert!(cx.did_prompt_for_new_path(), "the save panel shows");
    let path = fs::canonicalize(dir.path()).unwrap().join("new.md");
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    draw(cx);
    assert_eq!(fs::read_to_string(&path).unwrap(), "# New\n");
    let saved_as = untitled.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned));
    assert_eq!(saved_as, Some(path));
}

#[gpui::test]
fn the_preview_stays_with_its_tab_as_tabs_close_and_move(cx: &mut TestAppContext) {
    let files = [("a.txt", "a\n"), ("README.md", "# Readme\n")];
    let (_dir, workbench, cx) = opened(&files, cx);
    let left = pane(&workbench, cx);
    click("markdown-toggle", cx);
    let preview = in_place(&left, cx).expect("shown");
    let names = |pane: &Entity<Pane>, cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, cx| {
            (pane.tabs().iter())
                .map(|tab| tab.item().tab_title(cx).to_string())
                .collect::<Vec<_>>()
        })
    };
    // Shown, focused and drawn with its toggle wherever its tab goes.
    let shows = |pane: &Entity<Pane>, what: &str, cx: &mut VisualTestContext| {
        assert_eq!(in_place(pane, cx).as_ref(), Some(&preview), "{what}");
        assert!(is_focused(&preview, cx), "{what}: focused");
        assert!(
            workbench.read_with(cx, |workbench, cx| {
                workbench.focus_handle(cx) == preview.focus_handle(cx)
            }),
            "{what}: the workbench's focus"
        );
        assert_eq!(
            icon(pane, "markdown-toggle", cx),
            Some(Icon::Code),
            "{what}"
        );
    };

    // Reordered, both ways.
    cx.update(|window, cx| left.update(cx, |pane, cx| pane.reorder(1, Some(0), window, cx)));
    draw(cx);
    assert_eq!(names(&left, cx), ["README.md", "a.txt"]);
    shows(&left, "moved first", cx);
    cx.update(|window, cx| left.update(cx, |pane, cx| pane.reorder(0, None, window, cx)));
    draw(cx);
    assert_eq!(names(&left, cx), ["a.txt", "README.md"]);
    shows(&left, "moved last", cx);

    // The tab before it closed.
    activate(&left, 0, cx);
    run_in("workbench.close_tab", &[], cx);
    assert_eq!(names(&left, cx), ["README.md"]);
    shows(&left, "after a close", cx);

    // A panel shown and hidden gives focus back to it.
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    draw(cx);
    run_in("dock.toggle", &[], cx);
    shows(&left, "after the dock", cx);

    // Moved to a split, and back, and in before another pane's tab.
    open_file(&workbench, &files_root(&workbench, cx).join("a.txt"), cx);
    activate(&left, 0, cx);
    shows(&left, "shown again", cx);
    run_in("pane.move_tab_right", &[], cx);
    let [one, two] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("split");
    assert_eq!(
        (names(&one, cx), names(&two, cx)),
        (vec!["a.txt".to_owned()], vec!["README.md".to_owned()])
    );
    shows(&two, "moved to a split", cx);
    run_in("pane.move_tab_left", &[], cx);
    let [left] = <[_; 1]>::try_from(panes(&workbench, cx)).expect("one pane");
    assert_eq!(names(&left, cx), ["a.txt", "README.md"]);
    shows(&left, "moved back", cx);
    activate(&left, 0, cx);
    run_in("pane.split_right", &[], cx);
    let right = panes(&workbench, cx).remove(1);
    assert_eq!(names(&right, cx), ["a.txt"]);
    let id = left.read_with(cx, |pane, _| pane.tabs()[1].id());
    let workspace = workbench.read_with(cx, |workbench, _| workbench.workspace().clone());
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let to = kiln_shell::pane_group::Destination::Pane {
                pane: right.clone(),
                index: Some(0),
            };
            workspace.move_tab(&left, id, to, window, cx)
        })
    });
    draw(cx);
    assert_eq!(names(&right, cx), ["README.md", "a.txt"]);
    shows(&right, "in before a tab", cx);
}

fn files_root(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> std::path::PathBuf {
    workbench.read_with(cx, |workbench, _| workbench.root().unwrap().to_owned())
}
