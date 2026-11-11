//! The editor's announcements ([`super::a11y_announce`]) as the window's
//! live region says them: each one's text and politeness, read off the
//! announcer's `Status` node in a window that mounts one beside the
//! editor, and the completion menu leaving the text area the focused node.

use std::{cell::RefCell, ops::Range, rc::Rc, time::Duration};

use fe_buffer::{Buffer, BufferId, Origin, Version};
use fe_command::Invocation;
use fe_editor::{
    EditorModel,
    completion::{CompletionItem, CompletionList, CompletionTrigger},
    diagnostics::{Diagnostic, Severity},
    ghost_text::GhostText,
};
use fe_ui::{Announcer, Politeness, Role, announce, nodes};
use gpui::{
    AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    Styled as _, TestAppContext, VisualTestContext, Window, div,
};

use super::a11y_announce::{clips, hidden_lines};
use crate::{
    element::a11y_text::{A11yText, Run, RunKey},
    hover::HOVER_DELAY,
    *,
};

/// A window holding the editor, another to move focus to, and the
/// announcer, as the workbench's does.
struct Root {
    editor: Entity<EditorView>,
    other: Entity<EditorView>,
    /// Whether the editor is drawn: a tab switch leaves it out.
    shown: bool,
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                div()
                    .h_1_2()
                    .children(self.shown.then(|| self.editor.clone())),
            )
            .child(div().h_1_2().child(self.other.clone()))
            .child(Announcer::mount(window, cx))
    }
}

const TEXT: &str = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n";

/// A focused editor on `text`, vim on or off, in an active window with an
/// announcer, drawn.
fn opened<'a>(
    text: &str,
    vim: bool,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let (root, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        let editor = cx.new(|cx| EditorView::new(model, EditorTheme::default_dark(cx), cx));
        let other = EditorModel::new(Buffer::new("other\n"), None);
        let other = cx.new(|cx| EditorView::new(other, EditorTheme::default_dark(cx), cx));
        Root {
            editor,
            other,
            shown: true,
        }
    });
    let other = root.read_with(cx, |root, _| root.other.clone());
    OTHER.with(|o| *o.borrow_mut() = Some(other));
    ROOT.with(|r| *r.borrow_mut() = Some(root.clone()));
    let editor = root.read_with(cx, |root, _| root.editor.clone());
    cx.update(|window, cx| {
        set_vim_enabled(cx, vim);
        window.activate_window();
        window.focus(&editor.focus_handle(cx), cx);
    });
    draw(cx);
    (editor, cx)
}

thread_local! {
    /// The window's other editor, to move focus to.
    static OTHER: RefCell<Option<Entity<EditorView>>> = const { RefCell::new(None) };
    static ROOT: RefCell<Option<Entity<Root>>> = const { RefCell::new(None) };
}

/// Draws the window with the editor or without it, as a tab switch does.
fn show_editor(shown: bool, cx: &mut VisualTestContext) {
    let root = ROOT.with(|r| r.borrow().clone()).expect("an open window");
    root.update(cx, |root, cx| {
        root.shown = shown;
        cx.notify();
    });
    draw(cx);
}

fn focus(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
}

fn focus_other(cx: &mut VisualTestContext) {
    let other = OTHER.with(|o| o.borrow().clone()).expect("an open window");
    focus(&other, cx);
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }
    cx.run_until_parked();
}

/// What the announcer says now, and how urgently.
fn said(cx: &mut VisualTestContext) -> (String, Politeness) {
    draw(cx);
    cx.update(|window, cx| {
        let status = (nodes(window, cx).into_iter())
            .find(|node| node.role == Role::Status)
            .expect("the announcer's Status node");
        let text = status.states.value.map(|value| value.to_string());
        (
            text.unwrap_or_default(),
            status.states.live.unwrap_or_default(),
        )
    })
}

/// Says a marker, so a test can tell that nothing was said after it.
const MARK: &str = "(marker)";

fn mark(cx: &mut VisualTestContext) {
    cx.update(|_, cx| announce(cx, MARK, Politeness::Polite));
    assert_eq!(said(cx).0, MARK);
}

fn polite(text: &str) -> (String, Politeness) {
    (text.to_owned(), Politeness::Polite)
}

fn assertive(text: &str) -> (String, Politeness) {
    (text.to_owned(), Politeness::Assertive)
}

fn run(editor: &Entity<EditorView>, id: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        editor.run_command(Invocation::of(id).unwrap(), None, cx);
    });
}

fn caret(editor: &Entity<EditorView>, at: usize, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        editor.model_mut().set_cursor(at);
        editor.changed(cx);
    });
}

#[gpui::test]
fn vim_mode_changes_are_said_politely_and_only_on_change(cx: &mut TestAppContext) {
    let (_editor, cx) = opened(TEXT, true, cx);
    mark(cx);
    // Opening in normal mode says nothing.
    assert_eq!(said(cx).0, MARK);
    cx.simulate_keystrokes("i");
    assert_eq!(said(cx), polite("Insert"));
    cx.simulate_keystrokes("escape");
    assert_eq!(said(cx), polite("Normal"));
    cx.simulate_keystrokes("v");
    assert_eq!(said(cx), polite("Visual"));
    cx.simulate_keystrokes("escape");
    assert_eq!(said(cx), polite("Normal"));
    // An operator waiting is still normal mode.
    mark(cx);
    cx.simulate_keystrokes("d");
    assert_eq!(said(cx).0, MARK);
    cx.simulate_keystrokes("escape");
    assert_eq!(said(cx).0, MARK);
}

#[gpui::test]
fn vim_errors_are_assertive_and_said_again_after_a_key(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, true, cx);
    mark(cx);
    // No search to repeat.
    cx.simulate_keystrokes("n");
    let message = editor.read_with(cx, |editor, _| {
        editor.vim_state().unwrap().message().unwrap().to_owned()
    });
    assert!(message.starts_with("E35:"), "{message}");
    assert_eq!(said(cx), assertive(&message));
    mark(cx);
    cx.simulate_keystrokes("n");
    assert_eq!(said(cx), assertive(&message), "the same error again");

    // The host's report is polite, its error assertive, even without a
    // code, and each is said again when set again.
    editor.update(cx, |editor, cx| {
        editor.set_vim_message("3 substitutions", cx)
    });
    assert_eq!(said(cx), polite("3 substitutions"));
    editor.update(cx, |editor, cx| {
        editor.set_vim_error("not saved: a.txt", cx)
    });
    assert_eq!(said(cx), assertive("not saved: a.txt"));
    mark(cx);
    cx.simulate_keystrokes("l");
    assert_eq!(said(cx).0, MARK, "a key ends the message");
    editor.update(cx, |editor, cx| {
        editor.set_vim_error("not saved: a.txt", cx)
    });
    assert_eq!(said(cx), assertive("not saved: a.txt"));
}

#[gpui::test]
fn each_host_setting_is_said_and_says_whether_its_an_error(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, true, cx);
    let error = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.update(cx, |editor, cx| {
            editor.set_vim_error("not saved: a.txt", cx)
        })
    };
    let report = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.update(cx, |editor, cx| {
            editor.set_vim_message("not saved: a.txt", cx)
        })
    };
    error(&editor, cx);
    assert_eq!(said(cx), assertive("not saved: a.txt"));
    mark(cx);
    error(&editor, cx);
    assert_eq!(
        said(cx),
        assertive("not saved: a.txt"),
        "the same error again"
    );
    report(&editor, cx);
    assert_eq!(said(cx), polite("not saved: a.txt"), "the text as a report");
    mark(cx);
    report(&editor, cx);
    assert_eq!(
        said(cx),
        polite("not saved: a.txt"),
        "the same report again"
    );
    error(&editor, cx);
    assert_eq!(
        said(cx),
        assertive("not saved: a.txt"),
        "and an error once more"
    );
}

#[gpui::test]
fn a_vim_error_without_a_code_is_assertive(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, true, cx);
    mark(cx);
    // Yanking into the read-only search register fails the key.
    cx.simulate_keystrokes("\" / y y");
    let message = editor.read_with(cx, |editor, _| {
        editor.vim_state().unwrap().message().map(str::to_owned)
    });
    let message = message.expect("an error");
    assert!(!message.starts_with('E'), "{message}");
    assert_eq!(said(cx), assertive(&message));
}

/// Applies `items` to `editor` as a server's diagnostics.
fn diagnose(
    editor: &Entity<EditorView>,
    items: Vec<(Range<usize>, Diagnostic)>,
    cx: &mut VisualTestContext,
) {
    editor.update(cx, |editor, cx| {
        let version = editor.model().snapshot().version();
        let id = (1, 1);
        assert!((editor.model_mut()).apply_diagnostics(1u32, id, version, items, Some(id)));
        cx.notify();
    });
    draw(cx);
}

fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

#[gpui::test]
fn the_diagnostic_at_the_caret_is_said_once_it_rests_once_a_visit(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, false, cx);
    let x = TEXT.find("x =").unwrap();
    let y = TEXT.find("y =").unwrap();
    diagnose(
        &editor,
        vec![
            (
                x..x + 1,
                Diagnostic::new(Severity::Error, "unused variable x"),
            ),
            (y..y + 1, Diagnostic::new(Severity::Warning, "y shadows")),
        ],
        cx,
    );
    mark(cx);
    caret(&editor, x, cx);
    // Not before it has rested.
    assert_eq!(said(cx).0, MARK);
    cx.executor().advance_clock(HOVER_DELAY);
    assert_eq!(said(cx), polite("error: unused variable x"));

    // A frame doesn't say it again, nor resting there again.
    mark(cx);
    editor.update(cx, |_, cx| cx.notify());
    cx.executor().advance_clock(HOVER_DELAY);
    assert_eq!(said(cx).0, MARK);

    // Passing through another without resting says nothing; coming back
    // is another visit.
    caret(&editor, y, cx);
    caret(&editor, 0, cx);
    cx.executor().advance_clock(HOVER_DELAY);
    assert_eq!(said(cx).0, MARK);
    caret(&editor, y, cx);
    cx.executor().advance_clock(HOVER_DELAY);
    assert_eq!(said(cx), polite("warning: y shadows"));

    // Next or previous problem says it at once, even this visit.
    mark(cx);
    editor.update(cx, |editor, cx| editor.announce_caret_diagnostics(cx));
    assert_eq!(said(cx), polite("warning: y shadows"));
}

#[gpui::test]
fn moving_within_a_diagnostic_puts_its_speech_off_until_the_caret_rests(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, false, cx);
    let x = TEXT.find("let x = 1").unwrap();
    diagnose(
        &editor,
        vec![(
            x..x + 9,
            Diagnostic::new(Severity::Error, "unused variable x"),
        )],
        cx,
    );
    mark(cx);
    caret(&editor, x, cx);
    for step in 1..4 {
        cx.executor().advance_clock(ms(200));
        caret(&editor, x + step, cx);
        assert_eq!(said(cx).0, MARK, "still moving at {step}");
    }
    cx.executor().advance_clock(ms(200));
    assert_eq!(said(cx).0, MARK, "200 ms since the last move");
    cx.executor().advance_clock(ms(100));
    assert_eq!(said(cx), polite("error: unused variable x"));
    // Moving on within it, once told, says nothing more.
    mark(cx);
    caret(&editor, x + 5, cx);
    cx.executor().advance_clock(HOVER_DELAY);
    assert_eq!(said(cx).0, MARK);
}

#[gpui::test]
fn two_diagnostics_with_the_same_text_are_each_said(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, false, cx);
    let x = TEXT.find("x =").unwrap();
    diagnose(
        &editor,
        vec![
            (
                x..x + 1,
                Diagnostic::new(Severity::Error, "unused variable"),
            ),
            (
                x + 1..x + 2,
                Diagnostic::new(Severity::Error, "unused variable"),
            ),
        ],
        cx,
    );
    caret(&editor, x, cx);
    cx.executor().advance_clock(HOVER_DELAY);
    assert_eq!(said(cx), polite("error: unused variable"));
    // Straight onto the next one, with the same text.
    mark(cx);
    caret(&editor, x + 1, cx);
    cx.executor().advance_clock(HOVER_DELAY);
    assert_eq!(said(cx), polite("error: unused variable"));
}

#[gpui::test]
fn a_diagnostic_waiting_when_focus_leaves_is_not_said(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, false, cx);
    let x = TEXT.find("x =").unwrap();
    diagnose(
        &editor,
        vec![(
            x..x + 1,
            Diagnostic::new(Severity::Error, "unused variable x"),
        )],
        cx,
    );
    mark(cx);
    caret(&editor, x, cx);
    cx.executor().advance_clock(ms(100));
    // As a tab switch does: another editor takes focus before the wait is up.
    focus_other(cx);
    cx.executor().advance_clock(HOVER_DELAY);
    assert_eq!(said(cx).0, MARK);
    // Back in it, the caret rests there again: another visit.
    focus(&editor, cx);
    cx.executor().advance_clock(HOVER_DELAY);
    assert_eq!(said(cx), polite("error: unused variable x"));
}

#[gpui::test]
fn folding_says_how_many_lines_and_unfolding_says_so(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, false, cx);
    mark(cx);
    caret(&editor, 0, cx);
    run(&editor, "editor.fold", cx);
    let hidden = editor.read_with(cx, |editor, _| {
        hidden_lines(
            editor
                .model()
                .display()
                .folds()
                .iter()
                .map(|f| (f.start, f.end)),
        )
    });
    assert_eq!(said(cx), polite(&format!("Folded {hidden} lines")));
    run(&editor, "editor.unfold", cx);
    assert_eq!(said(cx), polite("Unfolded"));
}

#[gpui::test]
fn a_second_cursor_and_back_to_one_are_said(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, false, cx);
    mark(cx);
    caret(&editor, 0, cx);
    run(&editor, "editor.add_cursor_below", cx);
    assert_eq!(said(cx), polite("2 cursors"));
    mark(cx);
    run(&editor, "editor.add_cursor_below", cx);
    assert_eq!(
        said(cx).0,
        MARK,
        "only one to more is said, not more to more"
    );
    caret(&editor, 0, cx);
    assert_eq!(said(cx), polite("One cursor"));
}

#[gpui::test]
fn ghost_text_says_a_suggestion_is_available_once(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, false, cx);
    let suggest = |text: &str, editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.update(cx, |editor, cx| {
            let model = editor.model();
            let at = model.primary_selection().head();
            let origin = Origin::Ai {
                agent: "agent".into(),
                session: "session".into(),
            };
            let ghost = GhostText::new(model.snapshot(), at, text, origin);
            editor.set_ghost_text(Some(ghost), cx);
        })
    };
    caret(&editor, 2, cx);
    mark(cx);
    suggest("_start", &editor, cx);
    assert_eq!(said(cx), polite("Suggestion available"));
    mark(cx);
    suggest("_other", &editor, cx);
    assert_eq!(said(cx).0, MARK, "a changed suggestion isn't said");
}

#[gpui::test]
fn announce_position_says_the_line_and_column_of_the_lines(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, false, cx);
    caret(&editor, TEXT.find("x =").unwrap(), cx);
    run(&editor, commands::ANNOUNCE_POSITION, cx);
    assert_eq!(said(cx), polite("line 2, column 9 of 5"));
}

/// Offers `labels` over the word before the caret, at once.
struct Offers {
    labels: RefCell<Vec<&'static str>>,
}

impl CompletionProvider for Offers {
    fn trigger_characters(&self, _: BufferId) -> Vec<String> {
        Vec::new()
    }

    fn completions(
        &self,
        _: BufferId,
        _: Version,
        offset: usize,
        _: CompletionTrigger,
    ) -> CompletionFuture {
        let items = (self.labels.borrow().iter())
            .map(|label| CompletionItem {
                label: label.to_string(),
                edit: (offset - 2..offset, label.to_string()),
                ..CompletionItem::default()
            })
            .collect();
        Box::pin(async move {
            Some(CompletionList {
                items,
                is_incomplete: false,
            })
        })
    }

    fn resolve(&self, item: CompletionItem) -> ResolveFuture {
        Box::pin(async move { item })
    }
}

/// The editor holds focus, and the frame made the text area the focused
/// node (its active descendant), and the menu draws no node of its own.
fn text_area_focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    draw(cx);
    cx.update(|window, cx| {
        assert!(
            editor.focus_handle(cx).is_focused(window),
            "the editor has focus"
        );
        // The frame made the text area its own active descendant, as it
        // does whenever the editor has focus.
        assert!(
            editor.read(cx).text_area_active,
            "the text area is the focused node"
        );
        let nodes = nodes(window, cx);
        let roles: Vec<Role> = nodes.iter().map(|node| node.role).collect();
        assert!(roles.contains(&Role::MultilineTextInput), "{roles:?}");
        for role in [Role::ListBox, Role::ListBoxOption, Role::Dialog, Role::Menu] {
            assert!(
                !roles.contains(&role),
                "the menu drew a {role:?}: {roles:?}"
            );
        }
    });
}

#[gpui::test]
fn completion_says_its_count_and_selection_and_keeps_the_text_area_focused(
    cx: &mut TestAppContext,
) {
    const TEXT: &str = "fn main() {\n    va\n}\n";
    let (editor, cx) = opened(TEXT, false, cx);
    let offers = Rc::new(Offers {
        labels: RefCell::new(vec!["value", "values", "vary"]),
    });
    let provider = offers.clone();
    editor.update(cx, |editor, cx| {
        editor.set_completion_provider(Some(provider));
        editor
            .model_mut()
            .set_cursor(TEXT.find("va\n").unwrap() + 2);
        editor.changed(cx);
    });
    draw(cx);
    // Opening: how many, and the first.
    cx.simulate_keystrokes("ctrl-space");
    assert_eq!(said(cx), polite("3 suggestions, value"));
    text_area_focused(&editor, cx);
    // Each move of the selection says the selected one.
    cx.simulate_keystrokes("down");
    assert_eq!(said(cx), polite("values"));
    text_area_focused(&editor, cx);
    // Refiltering says the selected one after it.
    cx.simulate_keystrokes("l");
    let count = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.completion_spoken().map(|c| c.count))
    };
    assert_eq!(count(&editor, cx), Some(2), "`vary` is filtered out");
    assert_eq!(said(cx), polite("values"));
    text_area_focused(&editor, cx);
    // Even when the matches and the selection stay as they were.
    mark(cx);
    cx.simulate_keystrokes("u");
    assert_eq!(count(&editor, cx), Some(2));
    assert_eq!(said(cx), polite("values"));
    text_area_focused(&editor, cx);
    // A new answer with other labels says the selected one of them.
    *offers.labels.borrow_mut() = vec!["valuable", "valued"];
    mark(cx);
    cx.simulate_keystrokes("ctrl-space");
    assert_eq!(said(cx), polite("valuable"));
    text_area_focused(&editor, cx);
    // Dismissing says nothing.
    mark(cx);
    cx.simulate_keystrokes("escape");
    assert_eq!(said(cx).0, MARK);
    assert!(!editor.read_with(cx, |editor, _| editor.completion_shown()));
    text_area_focused(&editor, cx);
    // Opening again says it all again.
    cx.simulate_keystrokes("ctrl-space");
    assert_eq!(said(cx), polite("2 suggestions, valuable"));
    // Accepting says nothing either: the text typed is echoed.
    mark(cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(said(cx).0, MARK);
    assert!(!editor.read_with(cx, |editor, _| editor.completion_shown()));
    assert!(editor.read_with(cx, |editor, _| {
        editor.model().text().to_string().contains("valuable\n")
    }));
    text_area_focused(&editor, cx);
}

/// A run for `text` at buffer row `row` from `start`, not drawn.
fn run_of(row: u32, start: usize, text: &str, truncated: bool) -> Run {
    let text_len = text.trim_end_matches('\n').len();
    let character_lengths: Vec<u8> = text.chars().map(|c| c.len_utf8() as u8).collect();
    let n = character_lengths.len();
    Run {
        key: RunKey {
            buffer_row: row,
            segment: 0,
        },
        display_row: row,
        start,
        text: text.to_owned(),
        text_len,
        truncated,
        character_lengths,
        positions: vec![0.; n],
        widths: vec![0.; n],
        bounds: gpui::accesskit::Rect::default(),
    }
}

fn node_text(runs: Vec<Run>) -> A11yText {
    let key = runs[0].key;
    A11yText {
        runs,
        anchor: (key, 0),
        focus: (key, 0),
        read_only: false,
    }
}

/// `r0\nr1\nr2\nr3\n`: each row's end, before its break.
fn row_end(row: u32) -> usize {
    row as usize * 3 + 2
}

#[test]
fn a_selection_past_the_windows_edges_or_a_cut_row_is_clipped() {
    // Rows 1 and 2 of four.
    let window = node_text(vec![
        run_of(1, 3, "r1\n", false),
        run_of(2, 6, "r2\n", false),
    ]);
    let cases: [(Range<usize>, bool); 6] = [
        (0..12, true),   // Select All
        (0..4, true),    // from above
        (7..10, true),   // to below
        (3..8, false),   // inside
        (3..9, false),   // to the last run's end, its break
        (10..10, false), // a caret off screen isn't a selection
    ];
    for (selection, clipped) in cases {
        assert_eq!(
            clips(&window, selection.clone(), row_end, unfolded),
            clipped,
            "{selection:?}"
        );
    }
    // Row 1 cut after its first character.
    let cut = node_text(vec![run_of(1, 3, "r", true), run_of(2, 6, "r2\n", false)]);
    assert!(clips(&cut, 3..5, row_end, unfolded), "past the cut");
    assert!(!clips(&cut, 3..4, row_end, unfolded), "up to the cut");
    // Rows 1 and 2 folded behind row 0: an end in the fold isn't clipped.
    let folded = node_text(vec![
        run_of(0, 0, "r0\n", false),
        run_of(3, 9, "r3\n", false),
    ]);
    let behind_row_0 = |offset: usize| (3..9).contains(&offset).then_some(0);
    assert!(!clips(&folded, 1..7, row_end, behind_row_0));
    // Rows 1 to 3 folded through the end, the header the last run: an end
    // past it, in the fold, isn't clipped either.
    let to_the_end = node_text(vec![run_of(0, 0, "r0\n", false)]);
    let behind_row_0 = |offset: usize| (3..=11).contains(&offset).then_some(0);
    assert!(!clips(&to_the_end, 0..10, row_end, behind_row_0));
    assert!(clips(&to_the_end, 0..10, row_end, unfolded));
}

/// No folds.
fn unfolded(_: usize) -> Option<u32> {
    None
}

/// The text node a frame of `editor` gives a screen reader, as its prepaint
/// builds it while one is on: a test window never turns that on.
fn built(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> A11yText {
    draw(cx);
    editor.update(cx, |editor, _| {
        let tab_size = editor.model().tab_size();
        let primary = editor.model().primary_selection();
        let read_only = editor.is_read_only() || !editor.accepts_text();
        let display = editor.model.display();
        let layout = editor.layout.as_mut().expect("a frame");
        crate::element::a11y_text::build(crate::element::a11y_text::Input {
            display,
            frame: &layout.frame,
            long_rows: &mut layout.long_rows,
            widths: &mut layout.glyph_widths,
            tab_size,
            anchor: primary.tail(),
            head: primary.head(),
            origin: (layout.origin.x.into(), layout.origin.y.into()),
            scale: 1.,
            cap: crate::element::a11y_text::Cap::EDITOR,
            read_only,
        })
    })
}

#[gpui::test]
fn a_selection_ending_in_a_fold_through_the_end_is_not_clipped(cx: &mut TestAppContext) {
    // No line break at the end, and the fold runs to it.
    let source = "fn a() {\n    one\n    two\n}";
    let (editor, cx) = opened(source, false, cx);
    editor.update(cx, |editor, cx| {
        editor.model_mut().fold_rows(0..=3);
        let inside = source.find("two").unwrap() + 1;
        editor.model_mut().select_range(0..inside, false);
        editor.changed(cx);
    });
    let text = built(&editor, cx);
    assert_eq!(text.runs.len(), 1, "the header alone: {:?}", text.runs);
    mark(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.a11y_text.borrow_mut().text = Some(text);
            let cell = editor.a11y_text.clone();
            editor.say_clipped_selection(&cell, window, cx);
        })
    });
    assert_eq!(said(cx).0, MARK);
}

#[gpui::test]
fn a_clipped_selection_says_its_lines_once(cx: &mut TestAppContext) {
    let (editor, cx) = opened("r0\nr1\nr2\nr3\n", false, cx);
    let window = node_text(vec![
        run_of(1, 3, "r1\n", false),
        run_of(2, 6, "r2\n", false),
    ]);
    let say = |range: Range<usize>, editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        cx.update(|win, cx| {
            editor.update(cx, |editor, cx| {
                editor.model_mut().select_range(range, false);
                editor.a11y_text.borrow_mut().text = Some(window.clone());
                let cell = editor.a11y_text.clone();
                editor.say_clipped_selection(&cell, win, cx);
            })
        });
    };
    mark(cx);
    say(0..12, &editor, cx);
    assert_eq!(said(cx), polite("4 lines selected"));
    mark(cx);
    say(0..12, &editor, cx);
    assert_eq!(said(cx).0, MARK, "once a selection");
    say(3..8, &editor, cx);
    assert_eq!(said(cx).0, MARK, "a selection the node holds isn't");
    say(0..4, &editor, cx);
    assert_eq!(said(cx), polite("2 lines selected"));
}

#[test]
fn nested_folds_hide_their_lines_once() {
    assert_eq!(hidden_lines([]), 0);
    assert_eq!(hidden_lines([(0, 3)]), 3);
    assert_eq!(hidden_lines([(0, 9), (2, 4)]), 9);
    assert_eq!(hidden_lines([(0, 2), (5, 6)]), 3);
}

#[gpui::test]
fn a_tab_switch_ends_the_visit_and_coming_back_starts_another(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, false, cx);
    let x = TEXT.find("x =").unwrap();
    diagnose(
        &editor,
        vec![(
            x..x + 1,
            Diagnostic::new(Severity::Error, "unused variable x"),
        )],
        cx,
    );
    mark(cx);
    caret(&editor, x, cx);
    cx.executor().advance_clock(ms(100));
    // Another tab takes focus and the editor isn't drawn at all.
    focus_other(cx);
    show_editor(false, cx);
    cx.executor().advance_clock(HOVER_DELAY);
    assert_eq!(said(cx).0, MARK);
    // Back, the caret where it was: it rests there in a new visit.
    show_editor(true, cx);
    focus(&editor, cx);
    cx.executor().advance_clock(HOVER_DELAY);
    assert_eq!(said(cx), polite("error: unused variable x"));
}

#[gpui::test]
fn vims_own_error_with_a_host_reports_text_is_still_assertive(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, true, cx);
    // The host reported this text once; a key has ended that message.
    let failure = "register / is read-only";
    editor.update(cx, |editor, cx| editor.set_vim_message(failure, cx));
    assert_eq!(said(cx), polite(failure));
    cx.simulate_keystrokes("l");
    mark(cx);
    // Now vim fails a key with the same text, its own error.
    cx.simulate_keystrokes("\" / y y");
    let message = editor.read_with(cx, |editor, _| {
        editor.vim_state().unwrap().message().map(str::to_owned)
    });
    assert_eq!(message.as_deref(), Some(failure));
    assert_eq!(said(cx), assertive(failure));
}

#[gpui::test]
fn a_host_report_with_nvims_error_code_is_assertive(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, true, cx);
    let exists = "E13: File exists (add ! to override)";
    editor.update(cx, |editor, cx| editor.set_vim_message(exists, cx));
    assert_eq!(said(cx), assertive(exists));
}

#[gpui::test]
fn a_maps_command_that_cant_run_is_an_assertive_error(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, true, cx);
    let remap = |from: &str, to: &str| fe_vim::Remap {
        modes: vec![fe_vim::RemapMode::Normal],
        from: from.to_string(),
        to: to.to_string(),
        recursive: false,
    };
    let remaps = vec![
        // Bound in the completion menu's context only.
        remap("<leader>a", "<Cmd>fe:completion.accept<CR>"),
        // Wants the text to insert.
        remap("<leader>i", "<Cmd>fe:editor.insert<CR>"),
    ];
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &remaps, ","));
    assert_eq!(issues, []);
    draw(cx);
    let message = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            editor.vim_state().unwrap().message().map(str::to_owned)
        })
    };
    for keys in [", a", ", i"] {
        mark(cx);
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        let failure = message(&editor, cx).expect("the map's command fails");
        assert!(!failure.starts_with('E'), "{failure}");
        assert_eq!(said(cx), assertive(&failure), "{keys}");
        cx.simulate_keystrokes("escape");
    }
}

#[gpui::test]
fn an_unfocused_splits_clipped_selection_is_not_said(cx: &mut TestAppContext) {
    let (_editor, cx) = opened("r0\nr1\nr2\nr3\n", false, cx);
    let other = OTHER.with(|o| o.borrow().clone()).unwrap();
    let window_text = node_text(vec![
        run_of(1, 3, "r1\n", false),
        run_of(2, 6, "r2\n", false),
    ]);
    other.update(cx, |other, _| {
        other.model_mut().insert("r0\nr1\nr2\nr3\n");
    });
    mark(cx);
    let say = |other: &Entity<EditorView>, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            other.update(cx, |other, cx| {
                other.model_mut().select_range(0..12, false);
                other.a11y_text.borrow_mut().text = Some(window_text.clone());
                let cell = other.a11y_text.clone();
                other.say_clipped_selection(&cell, window, cx);
            })
        });
    };
    say(&other, cx);
    assert_eq!(said(cx).0, MARK, "the other editor isn't focused");
    // Focus coming to it doesn't say the selection it had.
    focus_other(cx);
    say(&other, cx);
    assert_eq!(said(cx).0, MARK);
}

/// A plain window with an announcer, to make active instead.
struct Elsewhere;

impl Render for Elsewhere {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(Announcer::mount(window, cx))
    }
}

/// What `window`'s live region says, drawn afresh.
fn said_in(window: gpui::AnyWindowHandle, cx: &mut VisualTestContext) -> Option<String> {
    cx.run_until_parked();
    let nodes = window
        .update(cx, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            nodes(window, cx)
        })
        .unwrap();
    (nodes.into_iter())
        .find(|node| node.role == Role::Status)
        .and_then(|node| node.states.value.map(|value| value.to_string()))
}

#[gpui::test]
fn an_editor_in_an_inactive_window_keeps_quiet(cx: &mut TestAppContext) {
    let (editor, cx) = opened(TEXT, true, cx);
    mark(cx);
    let elsewhere = cx.update(|_, cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| Elsewhere))
            .unwrap()
    });
    let elsewhere: gpui::AnyWindowHandle = elsewhere.into();
    (elsewhere.update(cx, |_, window, _| window.activate_window())).unwrap();
    cx.run_until_parked();
    // The editor keeps focus in its own window, which isn't active.
    editor.update(cx, |editor, cx| {
        editor.set_vim_error("not saved: a.txt", cx)
    });
    run(&editor, "editor.add_cursor_below", cx);
    assert_eq!(said(cx).0, MARK);
    assert_eq!(said_in(elsewhere, cx), None);
}

#[gpui::test]
fn moving_the_caret_back_within_the_typed_word_refilters_and_says_so(cx: &mut TestAppContext) {
    const TEXT: &str = "fn main() {\n    va\n}\n";
    let (editor, cx) = opened(TEXT, false, cx);
    let offers = Rc::new(Offers {
        labels: RefCell::new(vec!["value", "values", "vary"]),
    });
    let provider = offers.clone();
    editor.update(cx, |editor, cx| {
        editor.set_completion_provider(Some(provider));
        editor
            .model_mut()
            .set_cursor(TEXT.find("va\n").unwrap() + 2);
        editor.changed(cx);
    });
    draw(cx);
    cx.simulate_keystrokes("ctrl-space");
    assert_eq!(said(cx), polite("3 suggestions, value"));
    cx.simulate_keystrokes("l u");
    assert_eq!(said(cx), polite("value"));
    // Back over the `u`: `val` refilters, the text as it was, and `value`
    // stays selected.
    mark(cx);
    cx.simulate_keystrokes("left");
    let spoken = editor.read_with(cx, |editor, _| {
        editor
            .completion_spoken()
            .map(|spoken| (spoken.count, spoken.label.to_owned()))
    });
    assert_eq!(spoken, Some((2, "value".to_owned())));
    assert_eq!(said(cx), polite("value"));
}
