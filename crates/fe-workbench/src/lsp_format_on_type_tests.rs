//! Formatting on type and on paste through the fake server: a trigger
//! character's edits joining its undo step, nothing asked with the setting
//! off, a stale answer dropped, a paste, a composition and two carets asking
//! nothing on type, and a paste of several rows range formatted.

use std::{fs, sync::Arc, time::Duration};

use fe_buffer::ResolvedSelection;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::Settings;
use fe_gpui::{EditorView, SettingsEvent, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{ClipboardItem, Entity, EntityInputHandler as _, Focusable as _, TestAppContext};
use lsp_types::{DocumentOnTypeFormattingOptions, OneOf, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use crate::tests::lsp::Lsp;

const ON_TYPE: &str = "textDocument/onTypeFormatting";
const RANGE: &str = "textDocument/rangeFormatting";
/// A `}` typed after the call is the server's to put on a row of its own
/// (the editor doesn't move a closer typed after text).
const TEXT: &str = "fn main() {\n    f();";
const DELAY: Duration = Duration::from_millis(300);

/// A server formatting on `}`, with range formatting if `range`.
fn script(range: bool) -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
            first_trigger_character: "}".into(),
            more_trigger_character: Some(vec!["\n".into()]),
        }),
        document_range_formatting_provider: range.then_some(OneOf::Left(true)),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

/// An edit of rows and columns, as the server sends it.
fn edit(start: (u32, u32), end: (u32, u32), text: &str) -> Value {
    json!({
        "range": {
            "start": { "line": start.0, "character": start.1 },
            "end": { "line": end.0, "character": end.1 },
        },
        "newText": text,
    })
}

fn reply(edits: Vec<Value>, delay: Duration) -> Scripted {
    Scripted {
        reply: Reply::Result(Value::Array(edits)),
        delay,
    }
}

/// The workbench with `src/main.rs` as `text` open and focused, the caret
/// at its end, both settings as given; rust-analyzer's fake has range
/// formatting, pyright's hasn't.
fn setup(
    cx: &mut TestAppContext,
    text: &str,
    on_type: bool,
    on_paste: bool,
) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(true), script(false));
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), text).unwrap();
    let editor = open(&mut s, "src/main.rs");
    set(&mut s, on_type, on_paste);
    caret(&mut s, &editor, text.len());
    (s, editor)
}

/// Opens `name` once its server has it, focused.
fn open(s: &mut Lsp, name: &str) -> Entity<EditorView> {
    s.open(name);
    let fake = match name.ends_with(".py") {
        true => &s.python,
        false => &s.fake,
    };
    fake.wait_for_document(&s.uri(name), "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    editor
}

/// Sets `editor.format_on_type` and `editor.format_on_paste` as a settings
/// reload does.
fn set(s: &mut Lsp, on_type: bool, on_paste: bool) {
    let mut settings = Settings::default();
    settings.editor.format_on_type = on_type;
    settings.editor.format_on_paste = on_paste;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    (s.workbench).update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.draw();
}

fn caret(s: &mut Lsp, editor: &Entity<EditorView>, offset: usize) {
    s.cx.update(|_, cx| editor.update(cx, |view, _| view.model_mut().set_cursor(offset)));
    s.draw();
}

fn text(s: &Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

fn undo(s: &mut Lsp, editor: &Entity<EditorView>) {
    s.cx.update(|_, cx| editor.update(cx, |view, _| view.model_mut().undo().unwrap()));
    s.draw();
}

/// The requests of `method` that `records` hold.
fn sent(records: Vec<Record>, method: &str) -> Vec<Value> {
    (records.into_iter())
        .filter_map(|record| match record {
            Record::Request {
                method: m, params, ..
            } if m == method => Some(params),
            _ => None,
        })
        .collect()
}

fn requests(s: &Lsp, method: &str) -> Vec<Value> {
    sent(s.fake.records(), method)
}

/// Whether the fake has answered `count` requests of `method`.
fn replied(s: &Lsp, method: &str, count: usize) -> bool {
    (s.fake.records().iter())
        .filter(|record| matches!(record, Record::Replied { method: m, .. } if m == method))
        .count()
        >= count
}

/// Pastes `text` from the clipboard, as cmd-v in the focused editor.
fn paste(s: &mut Lsp, text: &str) {
    s.cx.write_to_clipboard(ClipboardItem::new_string(text.into()));
    let stack = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(fe_editor::commands::CONTEXT));
    let invocation = Invocation::of("editor.paste").unwrap().context(stack);
    s.cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    s.draw();
}

#[gpui::test]
fn typing_a_trigger_char_applies_the_edits_as_one_undo_step(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, TEXT, true, false);
    s.fake.set_reply(
        ON_TYPE,
        reply(vec![edit((1, 8), (1, 8), "\n")], Duration::ZERO),
    );
    s.cx.simulate_input("}");
    let formatted = "fn main() {\n    f();\n}";
    s.until("the format applied", |s| text(s, &editor) == formatted);
    let asked = requests(&s, ON_TYPE);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0]["textDocument"]["uri"], json!(s.uri("src/main.rs")));
    assert_eq!(asked[0]["position"], json!({ "line": 1, "character": 9 }));
    assert_eq!(asked[0]["ch"], json!("}"));
    assert_eq!(asked[0]["options"]["tabSize"], json!(4));
    assert_eq!(asked[0]["options"]["insertSpaces"], json!(true));
    // The caret stays after the brace.
    let at = editor.read_with(&s.cx, |view, _| view.model().primary_selection().head());
    assert_eq!(at, formatted.len());

    // One undo takes back the brace and its format.
    undo(&mut s, &editor);
    assert_eq!(text(&s, &editor), TEXT);

    // With the setting off, a brace asks nothing: the next request is the
    // one typed once it's back on.
    set(&mut s, false, false);
    caret(&mut s, &editor, TEXT.len());
    s.cx.simulate_input("}");
    s.draw();
    assert_eq!(text(&s, &editor), format!("{TEXT}}}"));
    set(&mut s, true, false);
    s.cx.simulate_input("}");
    s.until("the second request", |s| requests(s, ON_TYPE).len() >= 2);
    s.until("its answer", |s| replied(s, ON_TYPE, 2));
    s.draw();
    let asked = requests(&s, ON_TYPE);
    assert_eq!(asked.len(), 2);
    assert_eq!(asked[1]["position"], json!({ "line": 1, "character": 10 }));
    // Its edit put both braces on a row of their own.
    assert_eq!(text(&s, &editor), "fn main() {\n    f();\n}}");
}

#[gpui::test]
fn stale_answers_and_pastes_are_ignored(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, TEXT, true, false);
    // An answer that comes after more typing is dropped.
    s.fake
        .set_reply(ON_TYPE, reply(vec![edit((1, 8), (1, 8), "\n")], DELAY));
    s.cx.simulate_input("}");
    s.until("the request", |s| requests(s, ON_TYPE).len() == 1);
    s.clock.wait_for_sleep(DELAY);
    s.cx.simulate_input("x");
    s.clock.advance(DELAY);
    s.until("the answer", |s| replied(s, ON_TYPE, 1));
    s.draw();
    assert_eq!(text(&s, &editor), format!("{TEXT}}}x"));

    s.fake.set_reply(
        ON_TYPE,
        reply(vec![edit((1, 8), (1, 8), "\n")], Duration::ZERO),
    );
    // A pasted brace asks nothing.
    paste(&mut s, "}");
    assert_eq!(text(&s, &editor), format!("{TEXT}}}x}}"));
    // Nor does one an input method composes and commits.
    s.cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.replace_and_mark_text_in_range(None, "}", None, window, cx);
            view.replace_text_in_range(None, "}", window, cx);
        })
    });
    s.draw();
    assert_eq!(text(&s, &editor), format!("{TEXT}}}x}}}}"));
    // Nor one typed at two carets.
    s.cx.update(|_, cx| {
        editor.update(cx, |view, _| {
            let model = view.model_mut();
            let end = model.text().len();
            model.set_selections([0..0, end..end].map(|range| ResolvedSelection {
                range,
                reversed: false,
            }));
        })
    });
    s.cx.simulate_input("}");
    s.draw();
    let typed = format!("}}{TEXT}}}x}}}}}}");
    assert_eq!(text(&s, &editor), typed);

    // The next request is for a brace typed at one caret.
    caret(&mut s, &editor, typed.len());
    s.cx.simulate_input("}");
    s.until("the second request", |s| requests(s, ON_TYPE).len() >= 2);
    s.until("its answer", |s| replied(s, ON_TYPE, 2));
    s.draw();
    let asked = requests(&s, ON_TYPE);
    assert_eq!(asked.len(), 2);
    assert_eq!(asked[1]["position"], json!({ "line": 1, "character": 14 }));
    assert_eq!(text(&s, &editor), "}fn main() {\n    f();\n}x}}}}");
}

#[gpui::test]
fn paste_formats_the_pasted_range_only(cx: &mut TestAppContext) {
    let text_before = "fn main() {\n    \n}\n";
    let (mut s, editor) = setup(cx, text_before, false, true);
    let at = "fn main() {\n    ".len();
    caret(&mut s, &editor, at);

    // Three rows pasted at column 4 of row 1, ending at the start of row 4.
    s.fake.set_reply(
        RANGE,
        reply(
            vec![edit((2, 0), (2, 0), "    "), edit((3, 0), (3, 0), "    ")],
            Duration::ZERO,
        ),
    );
    paste(&mut s, "a();\nb();\nc();\n");
    let formatted = "fn main() {\n    a();\n    b();\n    c();\n\n}\n";
    s.until("the paste formatted", |s| text(s, &editor) == formatted);
    let asked = requests(&s, RANGE);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0]["range"],
        json!({
            "start": { "line": 1, "character": 4 },
            "end": { "line": 4, "character": 0 },
        })
    );
    assert_eq!(asked[0]["options"]["tabSize"], json!(4));
    assert!(requests(&s, "textDocument/formatting").is_empty());
    assert!(requests(&s, ON_TYPE).is_empty());
    // One undo takes back the paste and its format.
    undo(&mut s, &editor);
    assert_eq!(text(&s, &editor), text_before);

    // One row, its line break and all, asks nothing.
    caret(&mut s, &editor, at);
    paste(&mut s, "x();\n");
    assert_eq!(text(&s, &editor), "fn main() {\n    x();\n\n}\n");
    undo(&mut s, &editor);

    // Nor does a server without range formatting.
    let python = open(&mut s, "tool.py");
    caret(&mut s, &python, 0);
    paste(&mut s, "a = 1\nb = 2\n");
    assert_eq!(text(&s, &python), "a = 1\nb = 2\nprint(1)\n");
    s.draw();
    assert!(sent(s.python.records(), RANGE).is_empty());

    // The next request is for three rows in main.rs again.
    let editor = open(&mut s, "src/main.rs");
    caret(&mut s, &editor, at);
    paste(&mut s, "a();\nb();\nc();\n");
    s.until("the paste formatted", |s| text(s, &editor) == formatted);
    assert_eq!(requests(&s, RANGE).len(), 2);
}

fn vim(s: &mut Lsp, on: bool) {
    s.cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, on));
    s.draw();
}

#[gpui::test]
fn enter_formats_on_a_newline_trigger_with_vim_off_and_in_insert_mode(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, TEXT, true, false);
    // The server respells the row the line break left.
    s.fake.set_reply(
        ON_TYPE,
        reply(vec![edit((1, 4), (1, 8), "g();")], Duration::ZERO),
    );
    s.cx.simulate_keystrokes("enter");
    let formatted = "fn main() {\n    g();\n    ";
    s.until("the format applied", |s| text(s, &editor) == formatted);
    let asked = requests(&s, ON_TYPE);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0]["ch"], json!("\n"));
    assert_eq!(asked[0]["position"], json!({ "line": 2, "character": 4 }));
    undo(&mut s, &editor);
    assert_eq!(text(&s, &editor), TEXT);

    // In vim's insert mode too, and `u` takes back the session with it.
    vim(&mut s, true);
    caret(&mut s, &editor, TEXT.len());
    s.cx.simulate_keystrokes("A enter");
    s.until("the second format", |s| text(s, &editor) == formatted);
    let asked = requests(&s, ON_TYPE);
    assert_eq!(asked.len(), 2);
    assert_eq!(asked[1]["ch"], json!("\n"));
    assert_eq!(asked[1]["position"], json!({ "line": 2, "character": 4 }));
    s.cx.simulate_keystrokes("escape u");
    s.draw();
    assert_eq!(text(&s, &editor), TEXT);
}

#[gpui::test]
fn vim_puts_at_the_end_format_the_rows_put_only(cx: &mut TestAppContext) {
    let start = "a();\nb();";
    let (mut s, editor) = setup(cx, start, false, true);
    vim(&mut s, true);
    s.fake.set_reply(
        RANGE,
        reply(
            vec![edit((2, 0), (2, 0), "    "), edit((3, 0), (3, 0), "    ")],
            Duration::ZERO,
        ),
    );
    // One row put below the last asks nothing, though a line break goes in
    // before it.
    s.cx.simulate_keystrokes("g g y y G p");
    s.draw();
    assert_eq!(text(&s, &editor), "a();\nb();\na();");
    s.cx.simulate_keystrokes("u");
    s.draw();
    assert_eq!(text(&s, &editor), start);

    // Two rows put there ask for exactly those rows.
    s.cx.simulate_keystrokes("g g y j G p");
    let formatted = "a();\nb();\n    a();\n    b();";
    s.until("the put formatted", |s| text(s, &editor) == formatted);
    let asked = requests(&s, RANGE);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0]["range"],
        json!({
            "start": { "line": 2, "character": 0 },
            "end": { "line": 3, "character": 4 },
        })
    );
    // One `u` takes back the put and its format.
    s.cx.simulate_keystrokes("u");
    s.draw();
    assert_eq!(text(&s, &editor), start);

    // A visual put over the last row, of two rows, asks for them.
    s.fake.set_reply(
        RANGE,
        reply(vec![edit((1, 0), (1, 0), "    ")], Duration::ZERO),
    );
    s.cx.simulate_keystrokes("g g y j G shift-v p");
    s.until("the visual put formatted", |s| {
        text(s, &editor) == "a();\n    a();\nb();"
    });
    let asked = requests(&s, RANGE);
    assert_eq!(
        asked[1]["range"],
        json!({
            "start": { "line": 1, "character": 0 },
            "end": { "line": 2, "character": 4 },
        })
    );
}

#[gpui::test]
fn vim_puts_count_the_register_rows_not_their_line_breaks(cx: &mut TestAppContext) {
    // One row put over characters, with line breaks before and after it,
    // asks nothing.
    let (mut s, editor) = setup(cx, "x", false, true);
    vim(&mut s, true);
    s.cx.simulate_keystrokes("g g y y v p");
    s.draw();
    assert_eq!(text(&s, &editor), "\nx\n");
    s.cx.simulate_keystrokes("u");
    s.draw();
    assert_eq!(text(&s, &editor), "x");

    // Two rows, the second empty, put below the last row ask for both.
    let start = "a\n\nz";
    s.cx.update(|_, cx| {
        editor.update(cx, |view, _| {
            let model = view.model_mut();
            let all = 0..model.text().len();
            model.select_range(all, false);
            model.insert(start);
        })
    });
    s.draw();
    s.fake.set_reply(
        RANGE,
        reply(vec![edit((3, 0), (3, 1), "b")], Duration::ZERO),
    );
    s.cx.simulate_keystrokes("escape g g y j G p");
    s.until("the put formatted", |s| text(s, &editor) == "a\n\nz\nb\n");
    let asked = requests(&s, RANGE);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0]["range"],
        json!({
            "start": { "line": 3, "character": 0 },
            "end": { "line": 4, "character": 0 },
        })
    );
}

/// Middle-clicks `editor` at byte `offset`, pasting the primary selection.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn middle_click(s: &mut Lsp, editor: &Entity<EditorView>, offset: usize) {
    let caret =
        s.cx.update(|_, cx| editor.update(cx, |view, _| view.caret_bounds_at(offset)));
    let caret = caret.expect("laid out");
    let at = gpui::point(caret.left() + gpui::px(1.), caret.center().y);
    let none = gpui::Modifiers::none();
    s.cx.simulate_mouse_down(at, gpui::MouseButton::Middle, none);
    s.cx.simulate_mouse_up(at, gpui::MouseButton::Middle, none);
    s.draw();
}

/// GPUI has a primary selection on Linux and FreeBSD only.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[gpui::test]
fn middle_click_pastes_format_and_linked_mirrors_do_not_count(cx: &mut TestAppContext) {
    let start = "fn main() {\n}\n";
    let (mut s, editor) = setup(cx, start, false, true);
    s.cx.update(|_, cx| editor.update(cx, |view, _| view.set_primary_selection(true)));
    s.draw();
    s.fake.set_reply(
        RANGE,
        reply(
            vec![edit((1, 0), (1, 0), "    "), edit((2, 0), (2, 0), "    ")],
            Duration::ZERO,
        ),
    );
    // A middle click pastes two rows at the start of row 1.
    s.cx.update(|_, cx| cx.write_to_primary(ClipboardItem::new_string("a();\nb();\n".into())));
    middle_click(&mut s, &editor, "fn main() {\n".len());
    let formatted = "fn main() {\n    a();\n    b();\n}\n";
    s.until("the middle click formatted", |s| {
        text(s, &editor) == formatted
    });
    let asked = requests(&s, RANGE);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0]["range"],
        json!({
            "start": { "line": 1, "character": 0 },
            "end": { "line": 3, "character": 0 },
        })
    );
    // One undo takes back the paste and its format.
    undo(&mut s, &editor);
    assert_eq!(text(&s, &editor), start);

    // One row pasted in a linked tag name, whose pair rows below changes
    // too, asks nothing.
    let tags = "<a>\nbody\n</a>";
    s.cx.update(|_, cx| {
        editor.update(cx, |view, _| {
            let model = view.model_mut();
            let all = 0..model.text().len();
            model.select_range(all, false);
            model.insert(tags);
            model.set_cursor(2);
            model.set_linked_ranges(vec![1..2, 11..12]);
        })
    });
    paste(&mut s, "bc");
    assert_eq!(text(&s, &editor), "<abc>\nbody\n</abc>");

    // The next request is the next paste of two rows'.
    caret(&mut s, &editor, 0);
    paste(&mut s, "x\ny\n");
    s.until("the second request", |s| requests(s, RANGE).len() >= 2);
    let asked = requests(&s, RANGE);
    assert_eq!(asked.len(), 2);
    assert_eq!(
        asked[1]["range"],
        json!({
            "start": { "line": 0, "character": 0 },
            "end": { "line": 2, "character": 0 },
        })
    );
}

#[gpui::test]
fn characters_put_over_a_block_format_its_rows(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, "ab\ncd", false, true);
    vim(&mut s, true);
    s.fake.set_reply(
        RANGE,
        reply(vec![edit((1, 1), (1, 2), "D")], Duration::ZERO),
    );
    s.cx.simulate_keystrokes("g g 0 y l ctrl-v j p");
    s.until("the put formatted", |s| text(s, &editor) == "ab\naD");
    let asked = requests(&s, RANGE);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0]["range"],
        json!({
            "start": { "line": 0, "character": 0 },
            "end": { "line": 1, "character": 1 },
        })
    );
    s.cx.simulate_keystrokes("u");
    s.draw();
    assert_eq!(text(&s, &editor), "ab\ncd");
}
