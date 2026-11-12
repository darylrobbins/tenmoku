// SPDX-License-Identifier: Apache-2.0

//! Keyboard reach for the editor and the window's chrome (A11Y-16a, the
//! accessibility design's section 7), driven by keys:
//!
//! - a toast: `toast.focus` focuses the newest one's first button, Tab and
//!   Shift-Tab move between its buttons, drawing the focus ring, Enter or
//!   Space presses one and Escape dismisses it, answering none; focus then
//!   goes back where it was;
//! - tab reorder: `pane.reorder_tab_left` and `pane.reorder_tab_right` move
//!   the active tab within its strip;
//! - the breadcrumbs: `workbench.pick_breadcrumb_path` opens the file
//!   crumb's picker, its folder's entries, and
//!   `workbench.pick_breadcrumb_symbol` the symbol crumbs' quick outline;
//! - code lenses: `lsp.run_lens` with no lens, as from the palette, lists
//!   the lenses over the caret's row to pick one from;
//! - inlay hints: `editor.apply_inlay_hint` applies the edits of the hint
//!   at or nearest the caret on its line, as a double-click on it does.
//!
//! Commands with no key are run from the command palette, by keys.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Mutex, MutexGuard},
};

use fe_command::Invocation;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use fe_workbench::{GrammarOffer, GrammarOffers, Launch, Workbench, set_grammar_offers};
use gpui::{
    AppContext as _, Bounds, Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext,
};

static SERIAL: Mutex<()> = Mutex::new(());

/// Runs the rest of a test with no other of this binary running: with
/// nothing naming a config directory, the workbench neither reads the
/// user's settings nor watches them from a thread the test scheduler would
/// reject.
fn serially() -> MutexGuard<'static, ()> {
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for var in [
        "TENMOKU_CONFIG_DIR",
        "FASTEDIT_CONFIG_DIR",
        "XDG_CONFIG_HOME",
        "HOME",
        "APPDATA",
        "USERPROFILE",
    ] {
        // SAFETY: every test takes the lock before it starts gpui, so no
        // other thread is running to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    guard
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
}

/// Runs command `id` where focus is, as the palette or a key would.
fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(Invocation::of(id).unwrap());
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Presses and releases `keys`, a key at a time: GPUI clicks a focused
/// button when Enter or Space is released.
fn press(keys: &str, cx: &mut VisualTestContext) {
    for key in keys.split_whitespace() {
        cx.simulate_keystrokes(key);
        cx.simulate_event(gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse(key).unwrap(),
        });
        draw(cx);
    }
}

/// Runs the command titled `title` from the command palette, by keys.
fn palette(title: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    cx.simulate_input(title);
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

/// Whether the window draws what `selector` names.
fn bounds(selector: &str, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
}

fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).expect("an editor").clone()
    })
}

fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let editor = editor(workbench, cx);
    cx.update(|window, cx| editor.focus_handle(cx).contains_focused(window, cx))
}

mod toasts {
    use super::*;

    /// A grammar offer's answers and their toast's buttons: Install, More
    /// (whose menu holds Not now, then Never), then the close button.
    const INSTALL: &str = "toast-choice-0-0";
    const MORE: &str = "toast-more-0";
    const NEVER: &str = "toast-more-item-0-2";
    const CLOSE: &str = "icon-button:toast-close-0";

    fn kotlin() -> GrammarOffer {
        GrammarOffer {
            language: "kotlin".into(),
            name: "Kotlin".into(),
            extensions: vec!["kt".into()],
            file_names: Vec::new(),
            size: 2_800_000,
            sha256: "ab".repeat(32),
        }
    }

    /// A workbench with `a.kt` open in an editor that has focus, and the
    /// grammar offer's toast, which asks a question, over it. The number
    /// of installs, and the file Never writes to.
    fn offered<'a>(
        dir: &Path,
        cx: &'a mut TestAppContext,
    ) -> (
        Entity<Workbench>,
        Rc<RefCell<u32>>,
        PathBuf,
        &'a mut VisualTestContext,
    ) {
        let installs = Rc::new(RefCell::new(0));
        let fetch = {
            let installs = installs.clone();
            Rc::new(move |_: &GrammarOffer, cx: &mut gpui::App| {
                *installs.borrow_mut() += 1;
                cx.background_spawn(std::future::pending())
            })
        };
        let never_file = dir.join(fe_workbench::PROMPT_FILE);
        let offers = GrammarOffers {
            catalog: vec![kotlin()],
            installed: Rc::new(|_| false),
            fetch,
            install: Rc::new(|_, _, _| panic!("the fetch never ends")),
            never_file: Some(never_file.clone()),
        };
        cx.update(|cx| {
            fe_gpui::init(cx);
            fe_workbench::init(cx);
            set_grammar_offers(cx, offers);
        });
        let launch = Launch {
            root: None,
            file: None,
        };
        let (workbench, cx) = cx.add_window_view(move |window, cx| {
            let theme = EditorTheme::default_dark(cx);
            Workbench::new(launch, theme, EditorSettings::default(), window, cx)
        });
        // Focus listeners hear only from an active window.
        cx.update(|window, _| window.activate_window());
        let path = dir.join("a.kt");
        fs::write(&path, "fun main() {}\n").unwrap();
        cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.open_document(path, window, cx)
            });
        });
        draw(cx);
        let editor = editor(&workbench, cx);
        cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
        draw(cx);
        assert!(bounds(INSTALL, cx).is_some(), "the offer shows");
        assert!(editor_focused(&workbench, cx));
        (workbench, installs, never_file, cx)
    }

    /// The button whose bounds hold the focus ring, of those named.
    fn ringed(cx: &mut VisualTestContext) -> Option<&'static str> {
        let ring = bounds("focus-ring", cx)?;
        [INSTALL, MORE, NEVER, CLOSE]
            .into_iter()
            .find(|button| bounds(button, cx).is_some_and(|b| ring.contains(&b.center())))
    }

    #[gpui::test]
    fn tab_and_shift_tab_move_the_ring_between_its_buttons(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let (workbench, installs, _, cx) = offered(dir.path(), cx);

        run("toast.focus", cx);
        assert!(!editor_focused(&workbench, cx), "the toast has focus");
        let mut seen = Vec::new();
        for _ in 0..4 {
            cx.simulate_keystrokes("tab");
            draw(cx);
            seen.push(ringed(cx));
        }
        assert_eq!(
            seen,
            [Some(MORE), Some(CLOSE), Some(INSTALL), Some(MORE)],
            "Tab goes through the answer and More, then the close button, and wraps"
        );
        cx.simulate_keystrokes("shift-tab");
        draw(cx);
        assert_eq!(ringed(cx), Some(INSTALL), "Shift-Tab goes back");
        assert_eq!(*installs.borrow(), 0, "moving presses nothing");
        assert!(bounds(INSTALL, cx).is_some());
    }

    /// Each button the focused toast's Tab order reaches is a labelled
    /// button to a screen reader.
    #[gpui::test]
    fn its_focusable_buttons_are_labelled_buttons(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let (_workbench, _, _, cx) = offered(dir.path(), cx);
        run("toast.focus", cx);
        let drawn = cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            fe_ui::nodes(window, cx)
        });
        let buttons: Vec<String> = (drawn.iter())
            .filter(|node| node.role == fe_ui::Role::Button)
            .filter(|node| node.id.to_string().contains("toast-"))
            .map(|node| node.label.to_string())
            .collect();
        // Install, More and the close button, in drawing order.
        assert_eq!(buttons.len(), 3, "{buttons:?}");
        assert!(buttons.iter().all(|label| !label.is_empty()), "{buttons:?}");
        for label in ["Install", "More", "Dismiss"] {
            assert!(
                buttons.iter().any(|button| button.starts_with(label)),
                "{label}: {buttons:?}"
            );
        }
    }

    #[gpui::test]
    fn enter_presses_the_focused_answer(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let (workbench, installs, _, cx) = offered(dir.path(), cx);

        run("toast.focus", cx);
        press("enter", cx);
        assert_eq!(*installs.borrow(), 1, "Install, the first button");
        assert!(bounds(INSTALL, cx).is_none(), "the offer's gone");
        assert!(editor_focused(&workbench, cx), "focus is back");
    }

    #[gpui::test]
    fn space_presses_a_later_answer(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let (workbench, installs, never_file, cx) = offered(dir.path(), cx);

        run("toast.focus", cx);
        // Tab to More, open it, then Tab to Not now and Never.
        press("tab space tab tab space", cx);
        assert_eq!(*installs.borrow(), 0);
        let never = fs::read_to_string(&never_file).unwrap_or_default();
        assert!(never.contains("kt"), "Never was answered: {never:?}");
        assert!(bounds(INSTALL, cx).is_none());
        assert!(editor_focused(&workbench, cx), "focus is back");
    }

    #[gpui::test]
    fn escape_dismisses_it_answering_none(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let (workbench, installs, never_file, cx) = offered(dir.path(), cx);

        run("toast.focus", cx);
        cx.simulate_keystrokes("tab escape");
        draw(cx);
        assert!(bounds(INSTALL, cx).is_none(), "dismissed");
        assert_eq!(*installs.borrow(), 0);
        assert!(!never_file.exists(), "no answer");
        assert!(editor_focused(&workbench, cx), "focus is back");
        // The editor has its keys again: Escape no longer reaches a toast.
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert!(editor_focused(&workbench, cx));
    }

    #[gpui::test]
    fn the_close_button_dismisses_it_by_keys(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let (workbench, installs, never_file, cx) = offered(dir.path(), cx);

        run("toast.focus", cx);
        press("shift-tab enter", cx);
        assert!(bounds(INSTALL, cx).is_none(), "closed");
        assert_eq!(*installs.borrow(), 0);
        assert!(!never_file.exists());
        assert!(editor_focused(&workbench, cx), "focus is back");
    }

    /// Focus can leave a focused toast for a picker and come back to the
    /// same button when the picker is cancelled; the toast's keys still
    /// work, and dismissing it still gives focus back to the editor.
    #[gpui::test]
    fn focus_comes_back_to_the_toast_after_go_to_file(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let (workbench, installs, never_file, cx) = offered(dir.path(), cx);

        run("toast.focus", cx);
        press("tab", cx);
        assert_eq!(ringed(cx), Some(MORE));
        cx.simulate_keystrokes("cmd-p");
        draw(cx);
        assert_eq!(ringed(cx), None, "Go to File has focus");
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert_eq!(ringed(cx), Some(MORE), "back on the same button");
        press("tab", cx);
        assert_eq!(ringed(cx), Some(CLOSE));
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(bounds(INSTALL, cx).is_none(), "dismissed");
        assert_eq!(*installs.borrow(), 0);
        assert!(!never_file.exists());
        assert!(editor_focused(&workbench, cx), "focus is back");
    }

    /// Show All Commands, which the shell beside the toasts handles, opens
    /// from a focused toast, from where focus was before it.
    #[gpui::test]
    fn show_all_commands_opens_from_a_focused_toast(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let (workbench, _, _, cx) = offered(dir.path(), cx);

        run("toast.focus", cx);
        cx.simulate_keystrokes("cmd-shift-p");
        draw(cx);
        assert!(!editor_focused(&workbench, cx), "the palette has focus");
        // Dismissed, it gives focus to where it was opened from.
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(editor_focused(&workbench, cx));
        assert!(bounds(INSTALL, cx).is_some(), "the toast is still there");

        // And it runs its commands: Toast: Focus again.
        run("toast.focus", cx);
        cx.simulate_keystrokes("cmd-shift-p");
        draw(cx);
        cx.simulate_input("Toast: Focus");
        draw(cx);
        cx.simulate_keystrokes("enter");
        draw(cx);
        press("tab", cx);
        assert_eq!(ringed(cx), Some(MORE));
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(bounds(INSTALL, cx).is_none());
        assert!(editor_focused(&workbench, cx));
    }

    #[gpui::test]
    fn the_palette_reaches_toast_focus(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let (workbench, installs, _, cx) = offered(dir.path(), cx);

        cx.simulate_keystrokes("cmd-shift-p");
        draw(cx);
        cx.simulate_input("Toast: Focus");
        draw(cx);
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(!editor_focused(&workbench, cx), "the toast has focus");
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert_eq!(ringed(cx), Some(MORE));
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert_eq!(*installs.borrow(), 0);
        assert!(editor_focused(&workbench, cx), "focus is back");
    }
}

/// A Rust file with a nested outline, as the toolbar's tests use.
const MOD_RS: &str = "\
mod outer {
    pub struct S;
    impl S {
        fn a() {}
        fn b() {
            let x = 1;
        }
    }
}

fn main() {}
";

/// A workbench on a project of `src/one.txt`, `src/two.txt`,
/// `src/mod.rs`, `a.txt`, `b.txt` and `c.txt`, with `files` open in that
/// order, the last active and focused. Plain text and an untrusted root,
/// so no language server starts.
fn project<'a>(
    files: &[&str],
    cx: &'a mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("src")).unwrap();
    for (path, text) in [
        ("src/one.txt", "one\n"),
        ("src/two.txt", "two\n"),
        ("src/mod.rs", MOD_RS),
        ("a.txt", "alpha\n"),
        ("b.txt", "beta\n"),
        ("c.txt", "gamma\n"),
    ] {
        fs::write(root.join(path), text).unwrap();
    }
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_path_buf()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    for file in files {
        let path = root.join(file);
        cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.open_document(path, window, cx)
            });
        });
        draw(cx);
    }
    let editor = editor(&workbench, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    (dir, workbench, cx)
}

/// The active editor's file name.
fn active_file(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let editor = editor(workbench, cx);
    editor.read_with(cx, |editor, _| {
        let path = editor.model().path().expect("a file");
        path.file_name().unwrap().to_string_lossy().into_owned()
    })
}

mod reorder {
    use super::*;
    use fe_ui::{Role, nodes};

    /// The editor tabs' labels, in strip order, as a screen reader is
    /// given them.
    fn tabs(cx: &mut VisualTestContext) -> Vec<String> {
        draw(cx);
        let drawn = cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            nodes(window, cx)
        });
        let numbered = |node: &fe_ui::Node| matches!(&node.id, gpui::ElementId::NamedInteger(n, _) if n.as_ref() == "tab");
        (drawn.iter())
            .filter(|node| node.role == Role::Tab && numbered(node))
            .map(|node| node.label.to_string())
            .collect()
    }

    #[gpui::test]
    fn the_active_tab_moves_within_its_strip(cx: &mut TestAppContext) {
        let _serial = serially();
        let (_dir, workbench, cx) = project(&["a.txt", "b.txt", "c.txt"], cx);
        assert_eq!(tabs(cx), ["a.txt", "b.txt", "c.txt"]);

        palette("Reorder Tab Left", cx);
        assert_eq!(tabs(cx), ["a.txt", "c.txt", "b.txt"]);
        palette("Reorder Tab Left", cx);
        assert_eq!(tabs(cx), ["c.txt", "a.txt", "b.txt"]);
        // At the start it stays.
        palette("Reorder Tab Left", cx);
        assert_eq!(tabs(cx), ["c.txt", "a.txt", "b.txt"]);

        palette("Reorder Tab Right", cx);
        assert_eq!(tabs(cx), ["a.txt", "c.txt", "b.txt"]);
        // The moved tab is still the active one, with focus.
        assert_eq!(active_file(&workbench, cx), "c.txt");
        assert!(editor_focused(&workbench, cx));

        // A pinned tab stays among the pinned: alone there, it can't move.
        palette("Pin Tab", cx);
        assert_eq!(tabs(cx), ["c.txt", "a.txt", "b.txt"]);
        palette("Reorder Tab Right", cx);
        assert_eq!(tabs(cx), ["c.txt", "a.txt", "b.txt"]);
        assert_eq!(active_file(&workbench, cx), "c.txt");
    }
}

mod breadcrumbs {
    use super::*;

    fn caret(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
        let editor = editor(workbench, cx);
        editor.read_with(cx, |editor, _| {
            let primary = editor.model().primary_selection();
            if primary.reversed {
                primary.range.start
            } else {
                primary.range.end
            }
        })
    }

    #[gpui::test]
    fn the_path_picker_lists_the_files_folder(cx: &mut TestAppContext) {
        let _serial = serially();
        let (_dir, workbench, cx) = project(&["src/one.txt"], cx);

        palette("Breadcrumbs: Pick from Folder", cx);
        // The folder's entries, the file selected: the next one down is
        // `two.txt` (`mod.rs` sorts before `one.txt`).
        cx.simulate_keystrokes("down enter");
        draw(cx);
        assert_eq!(active_file(&workbench, cx), "two.txt");
        assert!(editor_focused(&workbench, cx));
    }

    #[gpui::test]
    fn the_symbol_picker_is_the_quick_outline(cx: &mut TestAppContext) {
        let _serial = serially();
        let (_dir, workbench, cx) = project(&["src/mod.rs"], cx);
        assert_eq!(caret(&workbench, cx), 0);

        palette("Breadcrumbs: Pick Symbol", cx);
        cx.simulate_input("main");
        draw(cx);
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert_eq!(
            caret(&workbench, cx),
            MOD_RS.find("main").unwrap() + "main".len()
        );
        assert!(editor_focused(&workbench, cx));
    }
}

/// The active editor's text.
fn text(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let editor = editor(workbench, cx);
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

/// Puts the active editor's caret at `offset`.
fn caret_to(workbench: &Entity<Workbench>, offset: usize, cx: &mut VisualTestContext) {
    let editor = editor(workbench, cx);
    editor.update(cx, |editor, cx| {
        editor.model_mut().set_cursor(offset);
        cx.notify();
    });
    draw(cx);
}

/// The active editor's caret.
fn caret(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    let editor = editor(workbench, cx);
    editor.read_with(cx, |editor, _| {
        let primary = editor.model().primary_selection();
        if primary.reversed {
            primary.range.start
        } else {
            primary.range.end
        }
    })
}

mod lenses {
    use super::*;
    use fe_buffer::BufferSnapshot;
    use fe_gpui::{Lens, LensItem, LensProvider};

    /// Two lenses over row 0, Select All and Line End, and one over row 2.
    struct Fixed;

    fn item(title: &str, command: &str) -> LensItem {
        LensItem {
            title: title.to_owned().into(),
            invocation: Invocation::of(command).unwrap(),
        }
    }

    impl LensProvider for Fixed {
        fn lenses(&mut self, _: &BufferSnapshot) -> Vec<Lens> {
            vec![
                Lens {
                    row: 0,
                    items: vec![
                        item("Select All", "editor.select_all"),
                        item("Line End", "editor.move_line_end"),
                    ],
                },
                Lens {
                    row: 2,
                    items: vec![item("Elsewhere", "editor.select_all")],
                },
            ]
        }
    }

    fn with_lenses(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
        let editor = editor(workbench, cx);
        editor.update(cx, |editor, cx| {
            let provider: Rc<RefCell<dyn LensProvider>> = Rc::new(RefCell::new(Fixed));
            editor.add_lens_provider(provider);
            cx.notify();
        });
        draw(cx);
    }

    #[gpui::test]
    fn the_palette_picks_a_lens_on_the_caret_row(cx: &mut TestAppContext) {
        let _serial = serially();
        let (_dir, workbench, cx) = project(&["a.txt"], cx);
        with_lenses(&workbench, cx);
        assert_eq!(caret(&workbench, cx), 0);

        // The row's lenses, in order: the second is Line End.
        palette("Code Lens: Run…", cx);
        cx.simulate_keystrokes("down enter");
        draw(cx);
        assert_eq!(caret(&workbench, cx), "alpha".len(), "Line End ran");
        assert!(editor_focused(&workbench, cx));

        // Typed, the picker filters them.
        caret_to(&workbench, 0, cx);
        palette("Code Lens: Run…", cx);
        cx.simulate_input("line");
        draw(cx);
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert_eq!(caret(&workbench, cx), "alpha".len());

        // Escape runs none, and gives the editor focus back.
        caret_to(&workbench, 0, cx);
        palette("Code Lens: Run…", cx);
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert_eq!(caret(&workbench, cx), 0);
        assert!(editor_focused(&workbench, cx));
    }

    #[gpui::test]
    fn a_row_without_lenses_offers_none(cx: &mut TestAppContext) {
        let _serial = serially();
        let (_dir, workbench, cx) = project(&["a.txt"], cx);
        let editor = editor(&workbench, cx);
        editor.update(cx, |editor, cx| {
            editor.model_mut().set_cursor(0);
            editor.model_mut().insert("one\ntwo\n");
            cx.notify();
        });
        draw(cx);
        with_lenses(&workbench, cx);
        // Row 1 has no lens: nothing opens, and the editor keeps focus.
        caret_to(&workbench, "one\n".len(), cx);
        palette("Code Lens: Run…", cx);
        assert!(editor_focused(&workbench, cx));
        assert_eq!(caret(&workbench, cx), "one\n".len());
    }
}

mod inlay_hints {
    use super::*;
    use fe_gpui::{InlayHint, InlayHintFuture, InlayHintHost};

    /// `let x = 1;`, with a hint `: i32` after `x` whose edit inserts it.
    const LINE: &str = "let x = 1;\nlet y = 2;\n";

    fn hint(offset: usize, label: &str, edits: bool) -> InlayHint {
        InlayHint {
            offset,
            label: label.to_owned(),
            kind: None,
            padding_left: false,
            padding_right: false,
            data: None,
            tooltip: None,
            edits: match edits {
                true => vec![(offset..offset, label.to_owned())],
                false => Vec::new(),
            },
        }
    }

    /// The active editor holding [`LINE`], with `hints`.
    fn hinted(
        hints: Vec<InlayHint>,
        host: Option<Rc<dyn InlayHintHost>>,
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
        let (dir, workbench, cx) = project(&["a.txt"], cx);
        let editor = editor(&workbench, cx);
        editor.update(cx, |editor, cx| {
            let len = editor.model().text().len();
            editor.model_mut().select_range(0..len, false);
            editor.model_mut().insert(LINE);
            let version = editor.model().snapshot().version();
            editor.set_inlay_hint_host(host);
            editor.set_inlay_hints(version, hints);
            cx.notify();
        });
        draw(cx);
        (dir, workbench, cx)
    }

    #[gpui::test]
    fn the_hint_nearest_the_caret_on_its_line_is_applied(cx: &mut TestAppContext) {
        let _serial = serially();
        let x = "let x".len();
        let y = LINE.find("let y").unwrap() + "let y".len();
        let hints = vec![hint(x, ": i32", true), hint(y, ": u8", true)];
        let (_dir, workbench, cx) = hinted(hints, None, cx);

        // From the start of the second line, its own hint.
        caret_to(&workbench, LINE.find("let y").unwrap(), cx);
        palette("Apply Inlay Hint", cx);
        assert_eq!(text(&workbench, cx), "let x = 1;\nlet y: u8 = 2;\n");
        assert!(editor_focused(&workbench, cx));

        // And then the first line's.
        caret_to(&workbench, "let x = 1".len(), cx);
        palette("Apply Inlay Hint", cx);
        assert_eq!(text(&workbench, cx), "let x: i32 = 1;\nlet y: u8 = 2;\n");

        // No hint is left on the line: nothing changes.
        palette("Apply Inlay Hint", cx);
        assert_eq!(text(&workbench, cx), "let x: i32 = 1;\nlet y: u8 = 2;\n");
    }

    #[gpui::test]
    fn a_hint_on_another_line_is_left(cx: &mut TestAppContext) {
        let _serial = serially();
        let y = LINE.find("let y").unwrap() + "let y".len();
        let (_dir, workbench, cx) = hinted(vec![hint(y, ": u8", true)], None, cx);
        caret_to(&workbench, 0, cx);
        palette("Apply Inlay Hint", cx);
        assert_eq!(text(&workbench, cx), LINE);
    }

    #[gpui::test]
    fn a_read_only_editor_refuses_it(cx: &mut TestAppContext) {
        let _serial = serially();
        let x = "let x".len();
        let (_dir, workbench, cx) = hinted(vec![hint(x, ": i32", true)], None, cx);
        let editor = editor(&workbench, cx);
        editor.update(cx, |editor, cx| editor.set_read_only(true, cx));
        caret_to(&workbench, 0, cx);
        palette("Apply Inlay Hint", cx);
        assert_eq!(text(&workbench, cx), LINE);
    }

    /// Resolves each hint with its edit once told to, as a slow server.
    struct Gated(RefCell<Vec<futures::channel::oneshot::Sender<()>>>);

    /// The host, sharing the gates with the test.
    struct GatedHost(Rc<Gated>);

    impl InlayHintHost for GatedHost {
        fn resolve(&self, mut hint: InlayHint) -> InlayHintFuture {
            let (go, gate) = futures::channel::oneshot::channel();
            self.0.0.borrow_mut().push(go);
            Box::pin(async move {
                gate.await.ok();
                hint.edits = vec![(hint.offset..hint.offset, hint.label.clone())];
                hint
            })
        }
    }

    /// An editor made read-only while its hint resolves doesn't take the
    /// hint's edits when they come.
    #[gpui::test]
    fn a_resolution_landing_read_only_is_refused(cx: &mut TestAppContext) {
        let _serial = serially();
        let x = "let x".len();
        let gated = Rc::new(Gated(RefCell::new(Vec::new())));
        let host: Rc<dyn InlayHintHost> = Rc::new(GatedHost(gated.clone()));
        let (_dir, workbench, cx) = hinted(vec![hint(x, ": i32", false)], Some(host), cx);
        caret_to(&workbench, 0, cx);
        palette("Apply Inlay Hint", cx);
        assert_eq!(gated.0.borrow().len(), 1, "asked to resolve");
        let editor = editor(&workbench, cx);
        editor.update(cx, |editor, cx| editor.set_read_only(true, cx));
        for go in gated.0.borrow_mut().drain(..) {
            go.send(()).ok();
        }
        draw(cx);
        assert_eq!(text(&workbench, cx), LINE);
    }

    /// Fills in each hint's edit when asked, as a language server resolves
    /// one.
    struct Resolver;

    impl InlayHintHost for Resolver {
        fn resolve(&self, mut hint: InlayHint) -> InlayHintFuture {
            hint.edits = vec![(hint.offset..hint.offset, hint.label.clone())];
            Box::pin(async move { hint })
        }
    }

    #[gpui::test]
    fn an_unresolved_hint_is_applied_once_resolved(cx: &mut TestAppContext) {
        let _serial = serially();
        let x = "let x".len();
        let (_dir, workbench, cx) =
            hinted(vec![hint(x, ": i32", false)], Some(Rc::new(Resolver)), cx);
        caret_to(&workbench, 0, cx);
        palette("Apply Inlay Hint", cx);
        assert_eq!(text(&workbench, cx), "let x: i32 = 1;\nlet y = 2;\n");
    }
}

/// A timed toast that goes while a picker opened from it is open: the
/// picker, cancelled, gives focus back to the toast's button, no longer
/// drawn, and focus goes on to where it was before `toast.focus`.
mod toast_gone_under_a_picker {
    use super::*;

    #[gpui::test]
    fn focus_goes_back_to_the_editor(cx: &mut TestAppContext) {
        let _serial = serially();
        let (_dir, workbench, cx) = project(&["a.txt"], cx);
        cx.update(|_, cx| Workbench::notify(cx, log::Level::Info, "Saved".into(), None));
        draw(cx);
        assert!(bounds("icon-button:toast-close-0", cx).is_some());

        run("toast.focus", cx);
        assert!(!editor_focused(&workbench, cx), "the toast has focus");
        cx.simulate_keystrokes("cmd-p");
        draw(cx);
        // The toast's time runs while Go to File has focus, and it goes.
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(30));
        draw(cx);
        assert!(bounds("icon-button:toast-close-0", cx).is_none(), "expired");

        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(
            editor_focused(&workbench, cx),
            "focus is back in the editor"
        );
        cx.simulate_input("x");
        draw(cx);
        assert_eq!(text(&workbench, cx), "xalpha\n", "and it types there");
    }
}
