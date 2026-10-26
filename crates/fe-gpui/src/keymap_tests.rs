//! Key input through `fe-keymap`: the default keymap file, user layers, the
//! chord timer on the test executor's simulated clock, and the window's
//! key context reaching the matcher.

use std::{
    cell::RefCell,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_command::Invocation;
use fe_editor::EditorModel;
use fe_keymap::{
    DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, KeySequence, Keymap, KeymapIssue, KeymapLayer, KeymapStore,
};
use gpui::{
    AnyWindowHandle, AppContext as _, Context, Entity, Focusable as _, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window, div,
};

use crate::{
    EditorTheme, EditorView, init_with_keymap,
    keymap::{CHORD_TIMEOUT, chord_pending, context_stack},
    keymap_report, set_user_keymap,
};

/// The bindings of card 1b's `default_bindings.rs`, in order, as
/// `(keys, command, context)`.
const OLD_TABLE: &[(&str, &str, Option<&str>)] = &[
    ("left", "editor.move_left", Some("editor")),
    ("right", "editor.move_right", Some("editor")),
    ("up", "editor.move_up", Some("editor")),
    ("down", "editor.move_down", Some("editor")),
    ("alt-left", "editor.move_word_left", Some("editor")),
    ("alt-right", "editor.move_word_right", Some("editor")),
    ("cmd-left", "editor.move_line_start", Some("editor")),
    ("home", "editor.move_line_start", Some("editor")),
    ("cmd-right", "editor.move_line_end", Some("editor")),
    ("end", "editor.move_line_end", Some("editor")),
    ("cmd-up", "editor.move_document_start", Some("editor")),
    ("cmd-down", "editor.move_document_end", Some("editor")),
    ("pageup", "editor.page_up", Some("editor")),
    ("pagedown", "editor.page_down", Some("editor")),
    ("shift-left", "editor.select_left", Some("editor")),
    ("shift-right", "editor.select_right", Some("editor")),
    ("shift-up", "editor.select_up", Some("editor")),
    ("shift-down", "editor.select_down", Some("editor")),
    ("alt-shift-left", "editor.select_word_left", Some("editor")),
    (
        "alt-shift-right",
        "editor.select_word_right",
        Some("editor"),
    ),
    ("cmd-shift-left", "editor.select_line_start", Some("editor")),
    ("shift-home", "editor.select_line_start", Some("editor")),
    ("cmd-shift-right", "editor.select_line_end", Some("editor")),
    ("shift-end", "editor.select_line_end", Some("editor")),
    (
        "cmd-shift-up",
        "editor.select_document_start",
        Some("editor"),
    ),
    (
        "cmd-shift-down",
        "editor.select_document_end",
        Some("editor"),
    ),
    ("shift-pageup", "editor.select_page_up", Some("editor")),
    ("shift-pagedown", "editor.select_page_down", Some("editor")),
    ("cmd-a", "editor.select_all", Some("editor")),
    (
        "cmd-alt-up",
        "editor.add_cursor_above",
        Some("editor && !vim.enabled"),
    ),
    (
        "cmd-alt-down",
        "editor.add_cursor_below",
        Some("editor && !vim.enabled"),
    ),
    (
        "cmd-d",
        "editor.select_next_occurrence",
        Some("editor && !vim.enabled"),
    ),
    (
        "cmd-shift-l",
        "editor.select_all_occurrences",
        Some("editor && !vim.enabled"),
    ),
    (
        "alt-shift-i",
        "editor.split_selection_into_lines",
        Some("editor && !vim.enabled"),
    ),
    (
        "escape",
        "editor.single_selection",
        Some("editor && !vim.enabled && !chat_composer"),
    ),
    ("cmd-]", "editor.indent", Some("editor")),
    ("cmd-[", "editor.outdent", Some("editor")),
    (
        "shift-tab",
        "editor.outdent",
        Some("editor && (!vim.enabled || vim.mode == insert || vim.mode == replace)"),
    ),
    (
        "ctrl-j",
        "editor.join_lines",
        Some("editor && (!vim.enabled || vim.mode == insert || vim.mode == replace)"),
    ),
    (
        "alt-up",
        "editor.move_rows_up",
        Some("editor && (!vim.enabled || vim.mode == insert || vim.mode == replace)"),
    ),
    (
        "alt-down",
        "editor.move_rows_down",
        Some("editor && (!vim.enabled || vim.mode == insert || vim.mode == replace)"),
    ),
    (
        "shift-alt-down",
        "editor.duplicate_rows",
        Some("editor && (!vim.enabled || vim.mode == insert || vim.mode == replace)"),
    ),
    ("cmd-shift-k", "editor.delete_rows", Some("editor")),
    ("cmd-/", "editor.toggle_comment", Some("editor")),
    ("cmd-shift-\\", "editor.go_to_bracket", Some("editor")),
    (
        "cmd-u",
        "editor.cursor_undo",
        Some("editor && !vim.enabled"),
    ),
    ("backspace", "editor.backspace", Some("editor")),
    ("shift-backspace", "editor.backspace", Some("editor")),
    ("delete", "editor.delete", Some("editor")),
    ("alt-backspace", "editor.delete_word_left", Some("editor")),
    (
        "alt-delete",
        "editor.delete_word_right",
        Some("editor && (!vim.enabled || vim.mode == insert || vim.mode == replace)"),
    ),
    (
        "cmd-backspace",
        "editor.delete_all_left",
        Some("editor && (!vim.enabled || vim.mode == insert || vim.mode == replace)"),
    ),
    (
        "ctrl-k",
        "editor.delete_all_right",
        Some("editor && (!vim.enabled || vim.mode == insert || vim.mode == replace)"),
    ),
    (
        "ctrl-t",
        "editor.transpose_letters",
        Some("editor && !vim.enabled"),
    ),
    ("enter", "editor.newline", Some("editor && !chat_composer")),
    ("tab", "editor.tab", Some("editor")),
    ("cmd-alt-[", "editor.fold", Some("editor")),
    ("cmd-alt-]", "editor.unfold", Some("editor")),
    (
        "ctrl-shift-cmd-right",
        "editor.expand_selection",
        Some("editor && !vim.enabled"),
    ),
    (
        "ctrl-shift-cmd-left",
        "editor.shrink_selection",
        Some("editor && !vim.enabled"),
    ),
    ("cmd-k cmd-0", "editor.fold_all", Some("editor")),
    ("cmd-k cmd-j", "editor.unfold_all", Some("editor")),
    ("cmd-k cmd-1", "editor.fold_level_1", Some("editor")),
    ("cmd-k cmd-2", "editor.fold_level_2", Some("editor")),
    ("cmd-k cmd-3", "editor.fold_level_3", Some("editor")),
    ("cmd-k cmd-4", "editor.fold_level_4", Some("editor")),
    ("cmd-k cmd-5", "editor.fold_level_5", Some("editor")),
    ("cmd-k cmd-6", "editor.fold_level_6", Some("editor")),
    ("cmd-k cmd-7", "editor.fold_level_7", Some("editor")),
    ("cmd-k cmd-[", "editor.fold_recursively", Some("editor")),
    ("cmd-k cmd-]", "editor.unfold_recursively", Some("editor")),
    (
        "cmd-k cmd-x",
        "editor.trim_trailing_whitespace",
        Some("editor"),
    ),
    (
        "cmd-k cmd-q",
        "editor.go_to_last_edit_location",
        Some("editor"),
    ),
    ("cmd-k cmd-i", "editor.show_hover", Some("editor")),
    ("f2", "lsp.rename", Some("editor")),
    ("cmd-.", "lsp.code_actions", Some("editor")),
    ("shift-alt-o", "lsp.organize_imports", Some("editor")),
    ("ctrl-shift-r", "lsp.refactor", Some("editor")),
    (
        "shift-alt-f",
        "lsp.format_document",
        Some("editor && (!vim.enabled || vim.mode == insert || vim.mode == replace)"),
    ),
    ("cmd-k cmd-f", "lsp.format_selection", Some("editor")),
    (
        "cmd-shift-space",
        "editor.show_signature_help",
        Some("editor"),
    ),
    ("alt-z", "editor.toggle_soft_wrap", Some("editor")),
    ("cmd-c", "editor.copy", Some("editor")),
    ("cmd-x", "editor.cut", Some("editor")),
    ("cmd-v", "editor.paste", Some("editor")),
    ("cmd-z", "editor.undo", Some("editor")),
    ("cmd-shift-z", "editor.redo", Some("editor")),
    ("cmd-s", "editor.save", Some("editor")),
    ("cmd-n", "workbench.new_file", Some("workbench")),
    ("cmd-shift-s", "workbench.save_as", Some("workbench")),
    ("cmd-q", "app.quit", None),
];

/// The default keymap with `user` as the user's layer over it.
fn with_user_layer(user: &str) -> Keymap {
    let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    let (user, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), user, None);
    assert_eq!(diagnostics, [], "{user:?}");
    Keymap::from_layers([default.expect("the default keymap loads"), user.unwrap()])
}

/// An editor showing `text` with a caret at `caret`.
fn new_editor(text: &str, caret: usize, cx: &mut Context<EditorView>) -> EditorView {
    let mut model = EditorModel::new(Buffer::new(text), None);
    model.set_selections(vec![ResolvedSelection {
        range: caret..caret,
        reversed: false,
    }]);
    EditorView::new(model, EditorTheme::default_dark(cx), cx)
}

/// A focused editor showing `text` with a caret at `caret`, matching keys
/// against `keymap`, in a headless window.
fn editor<'a>(
    cx: &'a mut TestAppContext,
    keymap: Keymap,
    text: &str,
    caret: usize,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(|cx| init_with_keymap(cx, keymap));
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| new_editor(&text, caret, cx));
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    (view, cx)
}

/// The invocations run from now on.
fn record(cx: &mut TestAppContext) -> Rc<RefCell<Vec<Invocation>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|cx| {
        crate::commands::observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone());
        });
    });
    seen
}

fn commands(seen: &RefCell<Vec<Invocation>>) -> Vec<String> {
    let seen = std::mem::take(&mut *seen.borrow_mut());
    seen.iter()
        .map(|invocation| invocation.command.as_str().to_owned())
        .collect()
}

fn text_and_caret(view: &Entity<EditorView>, cx: &mut TestAppContext) -> (String, usize) {
    view.update(cx, |view, _| {
        let model = view.model();
        (model.text().to_string(), model.selections()[0].range.end)
    })
}

/// One of vim's `<C-w>` window keys: a `pane.` command on a `ctrl-w` chord
/// in vim's normal mode.
pub(crate) fn is_window_key(binding: &fe_keymap::Binding) -> bool {
    binding.command.as_str().starts_with("pane.")
        && binding.keys.to_string().starts_with("ctrl-w ")
        && binding.context.as_ref().map(ToString::to_string).as_deref()
            == Some("editor && vim.mode == normal")
}

/// No keys are bound twice in the same context.
pub(crate) fn assert_no_duplicates(bindings: &[fe_keymap::Binding]) {
    let mut seen = std::collections::HashSet::new();
    for binding in bindings {
        let context = binding.context.as_ref().map(ToString::to_string);
        assert!(
            seen.insert((&binding.keys, context.clone())),
            "{} is bound twice in {context:?}",
            binding.keys
        );
    }
}

#[gpui::test]
fn default_keymap_matches_the_old_table(cx: &mut TestAppContext) {
    // fe-workbench's commands (the `workbench`, `picker`, `text_input`,
    // `search_panel`, `project_panel` and `ReferencesPanel` contexts) are
    // checked by its own catalog, which fe-gpui can't depend on.
    let store = KeymapStore::load(DEFAULT_MACOS, None, None);
    assert_eq!(store.diagnostics(), []);
    let bindings = store.keymap().bindings();
    // The old table's rows come first, every one of them.
    assert!(bindings.len() >= OLD_TABLE.len());
    assert_no_duplicates(bindings);
    assert!(bindings.iter().any(is_window_key));
    for (binding, &(keys, command, context)) in bindings.iter().zip(OLD_TABLE) {
        assert_eq!(binding.keys, KeySequence::parse(keys).unwrap(), "{command}");
        assert_eq!(binding.command.as_str(), command, "{keys}");
        let found = binding.context.as_ref().map(ToString::to_string);
        assert_eq!(found.as_deref(), context, "{keys}");
    }
    // Each command but the workbench's checks out.
    let catalog = crate::commands::command_catalog();
    let workbench = |binding: &&fe_keymap::Binding| {
        let context = binding.context.as_ref().map(ToString::to_string);
        crate::keymap_tests::is_window_key(binding)
            || matches!(
                context.as_deref(),
                Some(
                    "workbench"
                        | "workbench && !terminal"
                        | "zen_mode"
                        | "terminal"
                        | "picker"
                        | "text_input"
                        | "search_panel"
                        | "search_panel && text_input"
                        | "find_bar"
                        | "find_bar && text_input"
                        | "editor || find_bar"
                        | "vim_line"
                        | "vim_line && text_input"
                        | "project_panel"
                        | "ReferencesPanel"
                        | "ReferencesPanel && vim"
                        | "search_results"
                        | "search_results && vim"
                        | "project_panel && vim"
                        | "problems_panel"
                        | "ai_changes"
                        | "problems_panel && vim"
                        | "outline_panel"
                        | "outline_panel && vim"
                        | "find_bar && replace_input && text_input"
                        | "editor && vim.mode == normal && !vim.pending"
                        | "editor && vim.mode == normal && !vim.pending && !vim.replace_with_register"
                        | "editor && vim.visual && !vim.replace_with_register"
                        | "workbench && !terminal && !vim_line && !(editor && vim.enabled)"
                        | "(project_panel || outline_panel || ReferencesPanel || problems_panel || search_results || git_panel) && vim && !editor && !text_input"
                        | "(project_panel || outline_panel || ReferencesPanel || problems_panel || search_results || git_panel || terminal_panel) && vim && !editor && !text_input"
                )
            )
    };
    // `find.show`, the `lsp.`, `git.` and `markdown.` commands are the
    // workbench's too, though bound in `editor` (or the Changes panel); so
    // are the chat's, bound in the composer, and Peek Definition, Peek
    // References and the peek's, bound in `editor` and its widget.
    let find = |binding: &&fe_keymap::Binding| {
        let command = binding.command.as_str();
        command.starts_with("find.")
            || command.starts_with("chat.")
            || command.starts_with("peek.")
            || command == "editor.peek_definition"
            || command == "editor.peek_references"
            || command.starts_with("lsp.")
            || command.starts_with("git.")
            || command.starts_with("ai.changes_")
            || command.starts_with("markdown.")
    };
    for binding in bindings
        .iter()
        .filter(|binding| !workbench(binding) && !find(binding))
    {
        assert_eq!(catalog.check(&binding.command, &binding.args), Ok(()));
    }
    // fe-workbench's, and vim's normal-mode caret keys, which share the
    // language-server keys' context.
    assert!(bindings.iter().any(|binding| workbench(&binding)));

    cx.update(|cx| init_with_keymap(cx, store.keymap().clone()));
    let issues = cx.update(|cx| keymap_report(cx));
    let shadowed: Vec<_> = issues
        .iter()
        .filter(|issue| matches!(issue, KeymapIssue::Shadowed { .. }))
        .collect();
    assert_eq!(shadowed, Vec::<&KeymapIssue>::new());
}

#[gpui::test]
fn user_layer_rebinds_and_unbinds(cx: &mut TestAppContext) {
    let keymap = with_user_layer(
        r#"
        [[bindings]]
        keys = "ctrl-a"
        command = "editor.move_line_start"
        context = "editor"

        [[bindings]]
        keys = "alt-z"
        context = "editor"
        command = ""
        "#,
    );
    let (view, cx) = editor(cx, keymap, "  abc\nd", 4);
    let soft_wrap =
        |cx: &mut VisualTestContext| view.update(cx, |view, _| view.model().soft_wrap());

    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(text_and_caret(&view, cx), ("  abc\nd".into(), 2));
    cx.simulate_keystrokes("alt-z");
    assert!(!soft_wrap(cx));
    assert_eq!(text_and_caret(&view, cx), ("  abc\nd".into(), 2));

    // A new layer applies from the next keystroke: ctrl-a now goes to the
    // line end, and alt-z is back.
    cx.update(|_, cx| {
        let keymap = with_user_layer(
            r#"
            [[bindings]]
            keys = "ctrl-a"
            command = "editor.move_line_end"
            context = "editor"
            "#,
        );
        set_user_keymap(cx, keymap);
    });
    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(text_and_caret(&view, cx), ("  abc\nd".into(), 5));
    cx.simulate_keystrokes("alt-z");
    assert!(soft_wrap(cx));
}

#[gpui::test]
fn chord_timeout_uses_the_simulated_clock(cx: &mut TestAppContext) {
    let keymap = with_user_layer(
        r#"
        [[bindings]]
        keys = "cmd-k"
        command = "editor.select_all"
        context = "editor"
        "#,
    );
    let (view, cx) = editor(cx, keymap, "a:\n  b\nc:\n  d", 5);
    let seen = record(cx);
    let started = Instant::now();

    // Alone, cmd-k waits for the timeout, then runs.
    cx.simulate_keystrokes("cmd-k");
    cx.executor()
        .advance_clock(CHORD_TIMEOUT - Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(commands(&seen), Vec::<String>::new());
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(commands(&seen), ["editor.select_all"]);

    // cmd-k then x types x, and the timer is cancelled.
    cx.simulate_keystrokes("right cmd-k x");
    cx.executor().advance_clock(CHORD_TIMEOUT * 2);
    cx.run_until_parked();
    assert_eq!(commands(&seen), ["editor.move_right", "editor.insert"]);
    assert_eq!(text_and_caret(&view, cx).0, "a:\n  b\nc:\n  dx");

    // The chord completed in time runs, not cmd-k.
    cx.simulate_keystrokes("cmd-k cmd-0");
    cx.executor().advance_clock(CHORD_TIMEOUT * 2);
    cx.run_until_parked();
    assert_eq!(commands(&seen), ["editor.fold_all"]);
    let folds: Vec<_> = view.update(cx, |view, _| {
        let folds = view.model().display().folds();
        folds.iter().map(|fold| (fold.start, fold.end)).collect()
    });
    assert_eq!(folds, [(0, 1), (2, 3)]);
    assert!(
        started.elapsed() < CHORD_TIMEOUT,
        "waited {:?}",
        started.elapsed()
    );
}

#[gpui::test]
fn swallowed_keys_are_typed_when_a_chord_fails(cx: &mut TestAppContext) {
    let keymap = with_user_layer(
        r#"
        [[bindings]]
        keys = "g g"
        command = "editor.move_document_start"
        context = "editor"
        "#,
    );
    let (view, cx) = editor(cx, keymap, "ab\ncd", 5);
    let seen = record(cx);

    // Timed out, the prefix is typed.
    cx.simulate_keystrokes("g");
    assert_eq!(text_and_caret(&view, cx), ("ab\ncd".into(), 5));
    cx.executor().advance_clock(CHORD_TIMEOUT);
    cx.run_until_parked();
    assert_eq!(text_and_caret(&view, cx), ("ab\ncdg".into(), 6));
    // Ended by a key it doesn't bind, both are typed, in order.
    cx.simulate_keystrokes("g x");
    assert_eq!(text_and_caret(&view, cx), ("ab\ncdggx".into(), 8));
    // And a key that ends one is matched afresh.
    cx.simulate_keystrokes("g left");
    assert_eq!(text_and_caret(&view, cx), ("ab\ncdggxg".into(), 8));
    commands(&seen);
    cx.simulate_keystrokes("g g");
    assert_eq!(commands(&seen), ["editor.move_document_start"]);
    assert_eq!(text_and_caret(&view, cx).1, 0);
}

#[gpui::test]
fn window_context_reaches_the_matcher(cx: &mut TestAppContext) {
    let keymap = with_user_layer(
        r#"
        [[bindings]]
        keys = "ctrl-b"
        command = "editor.select_all"
        context = "workbench"
        "#,
    );
    let (view, cx) = editor(cx, keymap, "ab", 0);
    let seen = record(cx);

    let stack = cx.update(|window, _| context_stack(&window.context_stack()));
    assert_eq!(stack, fe_editor::commands::context());
    cx.simulate_keystrokes("right");
    let invocations = std::mem::take(&mut *seen.borrow_mut());
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].context, fe_editor::commands::context());

    cx.simulate_keystrokes("ctrl-b");
    assert_eq!(commands(&seen), Vec::<String>::new());
    let selection = view.update(cx, |view, _| view.model().selections()[0].range.clone());
    assert_eq!(selection, 1..1);
}

/// `cmd-k` alone selects all, after the chord timeout: a chord prefix that
/// runs if the chord is ended rather than abandoned. `g g` goes to the
/// start: a prefix that is typed if the chord fails.
const PREFIXES: &str = r#"
[[bindings]]
keys = "cmd-k"
command = "editor.select_all"
context = "editor"

[[bindings]]
keys = "g g"
command = "editor.move_document_start"
context = "editor"
"#;

/// The folded line ranges.
fn folds(view: &Entity<EditorView>, cx: &mut TestAppContext) -> Vec<(u32, u32)> {
    view.update(cx, |view, _| {
        let folds = view.model().display().folds();
        folds.iter().map(|fold| (fold.start, fold.end)).collect()
    })
}

/// Waits out the chord timeout, twice over.
fn time_out(cx: &mut TestAppContext) {
    cx.executor().advance_clock(CHORD_TIMEOUT * 2);
    cx.run_until_parked();
}

/// A focused editor showing `text`, with the caret at its end, in a window
/// of its own.
fn editor_window(cx: &mut TestAppContext, text: &str) -> (AnyWindowHandle, Entity<EditorView>) {
    let window = cx.add_window(|_, cx| new_editor(text, text.len(), cx));
    let view = window
        .update(cx, |view, window, cx| {
            window.focus(&view.focus_handle(cx), cx);
            cx.entity()
        })
        .unwrap();
    cx.run_until_parked();
    (window.into(), view)
}

#[gpui::test]
fn a_chord_belongs_to_its_window(cx: &mut TestAppContext) {
    cx.update(|cx| init_with_keymap(cx, with_user_layer(PREFIXES)));
    let (a, a_view) = editor_window(cx, "a:\n  b");
    let (b, b_view) = editor_window(cx, "c:\n  d");
    let seen = record(cx);

    // cmd-0 in b doesn't complete a's cmd-k, and a's cmd-k never runs.
    cx.simulate_keystrokes(a, "cmd-k");
    cx.simulate_keystrokes(b, "cmd-0");
    time_out(cx);
    assert_eq!(commands(&seen), Vec::<String>::new());
    assert_eq!(folds(&a_view, cx), []);
    assert_eq!(folds(&b_view, cx), []);

    // A prefix in b isn't typed into a.
    cx.simulate_keystrokes(b, "g");
    cx.simulate_keystrokes(a, "g");
    time_out(cx);
    assert_eq!(text_and_caret(&a_view, cx), ("a:\n  bg".into(), 7));
    assert_eq!(text_and_caret(&b_view, cx), ("c:\n  d".into(), 6));
    commands(&seen);

    // Closing a abandons its chord: cmd-0 in b doesn't complete it.
    cx.simulate_keystrokes(a, "cmd-k");
    assert!(cx.update(|cx| chord_pending(cx)));
    cx.update_window(a, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert!(!cx.update(|cx| chord_pending(cx)));
    cx.simulate_keystrokes(b, "cmd-0");
    time_out(cx);
    assert_eq!(commands(&seen), Vec::<String>::new());
    assert_eq!(folds(&b_view, cx), []);

    // b's own chord still runs.
    cx.simulate_keystrokes(b, "cmd-k cmd-0");
    assert_eq!(commands(&seen), ["editor.fold_all"]);
    assert_eq!(folds(&b_view, cx), [(0, 1)]);
}

/// Two editors side by side, in one window.
struct Pair {
    left: Entity<EditorView>,
    right: Entity<EditorView>,
}

impl Render for Pair {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .child(self.left.clone())
            .child(self.right.clone())
    }
}

#[gpui::test]
fn a_chord_belongs_to_its_focused_element(cx: &mut TestAppContext) {
    cx.update(|cx| init_with_keymap(cx, with_user_layer(PREFIXES)));
    let (pair, cx) = cx.add_window_view(|_, cx| Pair {
        left: cx.new(|cx| new_editor("a:\n  b", 6, cx)),
        right: cx.new(|cx| new_editor("c:\n  d", 6, cx)),
    });
    let (left, right) = pair.update(cx, |pair, _| (pair.left.clone(), pair.right.clone()));
    // Then draws, as the next frame would, which reports the focus change.
    let focus = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            window.focus(&view.focus_handle(cx), cx);
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    };
    // Focus events are only reported in the active window.
    cx.update(|window, _| window.activate_window());
    focus(&left, cx);
    let seen = record(cx);

    // Started in left, cmd-k is neither completed in right nor run there
    // on timeout.
    cx.simulate_keystrokes("cmd-k");
    focus(&right, cx);
    cx.simulate_keystrokes("cmd-0");
    time_out(cx);
    focus(&left, cx);
    cx.simulate_keystrokes("cmd-k");
    focus(&right, cx);
    time_out(cx);
    assert_eq!(commands(&seen), Vec::<String>::new());
    assert_eq!(folds(&right, cx), []);

    // A prefix isn't typed into right.
    focus(&left, cx);
    cx.simulate_keystrokes("g");
    focus(&right, cx);
    time_out(cx);
    assert_eq!(text_and_caret(&left, cx).0, "a:\n  b");
    assert_eq!(text_and_caret(&right, cx).0, "c:\n  d");

    // Focus that leaves and comes back abandons the chord too.
    focus(&left, cx);
    cx.simulate_keystrokes("cmd-k");
    assert!(cx.update(|_, cx| chord_pending(cx)));
    focus(&right, cx);
    assert!(!cx.update(|_, cx| chord_pending(cx)));
    focus(&left, cx);
    cx.simulate_keystrokes("cmd-0");
    time_out(cx);
    assert_eq!(commands(&seen), Vec::<String>::new());
    assert_eq!(folds(&left, cx), []);

    // Kept in one element, it runs.
    cx.simulate_keystrokes("cmd-k cmd-0");
    assert_eq!(commands(&seen), ["editor.fold_all"]);
    assert_eq!(folds(&left, cx), [(0, 1)]);
}

#[gpui::test]
fn unsupported_keys_fail_a_chord(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, with_user_layer(PREFIXES), "a:\n  b", 6);
    let seen = record(cx);

    // f21, which fe-keymap has no name for, ends cmd-k: cmd-0 after it is
    // a fresh keystroke, and cmd-k doesn't run on timeout.
    cx.simulate_keystrokes("cmd-k f21 cmd-0");
    assert_eq!(folds(&view, cx), []);
    cx.simulate_keystrokes("cmd-k f21");
    time_out(cx);
    assert_eq!(commands(&seen), Vec::<String>::new());

    // A prefix it ends is typed at once.
    cx.simulate_keystrokes("g f21");
    assert_eq!(text_and_caret(&view, cx), ("a:\n  bg".into(), 7));
    time_out(cx);
    assert_eq!(text_and_caret(&view, cx), ("a:\n  bg".into(), 7));
    commands(&seen);

    // A modifier pressed alone leaves the chord be.
    cx.simulate_keystrokes("cmd-k");
    cx.simulate_modifiers_change(gpui::Modifiers::shift());
    cx.simulate_modifiers_change(gpui::Modifiers::none());
    cx.simulate_keystrokes("cmd-0");
    assert_eq!(commands(&seen), ["editor.fold_all"]);
    assert_eq!(folds(&view, cx), [(0, 1)]);
}
