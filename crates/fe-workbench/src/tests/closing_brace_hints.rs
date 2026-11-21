//! Closing-brace hints: `} fn render` after the last row of a block that
//! spans the minimum rows, from the outline, merged with the server's.

use std::{sync::Arc, time::Duration};

use fe_config::{Settings, SettingsEvent};
use fe_gpui::EditorView;
use fe_lsp::fake::{InitializeReply, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{InlayHintOptions, OneOf, PositionEncodingKind, ServerCapabilities};
use serde_json::json;

use crate::tests::lsp::Lsp;

/// `head`, `rows - 2` body rows, then `}`: `rows` rows in all.
fn block(head: &str, rows: usize) -> String {
    let mut text = format!("{head} {{\n");
    for n in 0..rows - 2 {
        text.push_str(&format!("    // row {n}\n"));
    }
    text.push_str("}\n");
    text
}

fn open(s: &mut Lsp, name: &str, text: &str) -> Entity<EditorView> {
    std::fs::write(s.path(name), text).unwrap();
    s.open(name);
    let editor = s
        .workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned());
    let editor = editor.expect("the file is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    editor
}

fn shown(s: &Lsp, editor: &Entity<EditorView>) -> Vec<(usize, String)> {
    editor.read_with(&s.cx, |view, _| view.inlay_hints())
}

/// The offset of the end of row `row` of `text`.
fn row_end(text: &str, row: usize) -> usize {
    text.split_inclusive('\n')
        .take(row + 1)
        .map(str::len)
        .sum::<usize>()
        - 1
}

fn wait_for(s: &mut Lsp, editor: &Entity<EditorView>, hints: Vec<(usize, String)>) {
    s.until("the hints", |s| shown(s, editor) == hints);
}

#[gpui::test]
fn a_long_function_gets_fn_name_after_its_last_row(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let text = block("fn render()", 30);
    let editor = open(&mut s, "src/long.rs", &text);
    wait_for(
        &mut s,
        &editor,
        vec![(row_end(&text, 29), "fn render".into())],
    );
}

#[gpui::test]
fn a_block_under_the_minimum_gets_none(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    // 24 rows: one short; the 25-row function after it gets one.
    let text = format!("{}\n{}", block("fn short()", 24), block("fn edge()", 25));
    let editor = open(&mut s, "src/edge.rs", &text);
    let last = text.lines().count() - 1;
    wait_for(
        &mut s,
        &editor,
        vec![(row_end(&text, last), "fn edge".into())],
    );
}

#[gpui::test]
fn impl_trait_and_mod_labels(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let text = [
        block("mod tests", 26),
        block("impl Foo", 26),
        block("trait Shape", 26),
        block("struct Big", 26),
    ]
    .join("\n");
    let editor = open(&mut s, "src/kinds.rs", &text);
    s.until("the hints", |s| shown(s, &editor).len() == 4);
    let labels: Vec<String> = shown(&s, &editor).into_iter().map(|(_, l)| l).collect();
    assert_eq!(
        labels,
        ["mod tests", "impl Foo", "trait Shape", "struct Big"]
    );
}

#[gpui::test]
fn a_server_hint_with_the_same_text_at_that_offset_wins(cx: &mut TestAppContext) {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        inlay_hint_provider: Some(OneOf::Right(
            lsp_types::InlayHintServerCapabilities::Options(InlayHintOptions {
                work_done_progress_options: Default::default(),
                resolve_provider: None,
            }),
        )),
        ..init.capabilities
    };
    let script = Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    };
    let mut s = Lsp::with_fake_clock(cx, script, Script::default());
    s.fake.set_reply(
        "textDocument/inlayHint",
        Scripted::now(Reply::Result(json!([
            { "position": { "line": 29, "character": 1 }, "label": "fn render" },
        ]))),
    );
    s.set_trust(true);
    let text = block("fn render()", 30);
    let editor = open(&mut s, "src/long.rs", &text);
    s.cx.executor().advance_clock(Duration::from_millis(300));
    s.draw();
    wait_for(
        &mut s,
        &editor,
        vec![(row_end(&text, 29), "fn render".into())],
    );
    // Still one, once the server's hints are in.
    s.until("the server's answer", |s| {
        s.clock.advance(Duration::from_millis(20));
        s.cx.executor().advance_clock(Duration::from_millis(300));
        s.draw();
        s.fake.records().iter().any(|r| {
            matches!(r, fe_lsp::fake::Record::Request { method, .. }
                if method == "textDocument/inlayHint")
        })
    });
    s.draw();
    assert_eq!(
        shown(&s, &editor),
        vec![(row_end(&text, 29), "fn render".into())]
    );
}

#[gpui::test]
fn the_setting_off_shows_none(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let text = block("fn render()", 30);
    let editor = open(&mut s, "src/long.rs", &text);
    wait_for(
        &mut s,
        &editor,
        vec![(row_end(&text, 29), "fn render".into())],
    );

    let mut settings = Settings::default();
    settings.editor.closing_brace_hints = false;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.until("no hints", |s| shown(s, &editor).is_empty());
}

#[gpui::test]
fn the_caret_skips_the_hint(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let text = block("fn render()", 30);
    let editor = open(&mut s, "src/long.rs", &text);
    wait_for(
        &mut s,
        &editor,
        vec![(row_end(&text, 29), "fn render".into())],
    );
    let end = row_end(&text, 29);
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(end - 1);
            cx.notify();
        })
    });
    s.draw();
    s.cx.simulate_keystrokes("right");
    let head = editor.read_with(&s.cx, |view, _| view.model().primary_selection().range.end);
    assert_eq!(head, end);
    // The hint is not in the buffer.
    let len = editor.read_with(&s.cx, |view, _| view.model().text().len());
    assert_eq!(len, text.len());
}
