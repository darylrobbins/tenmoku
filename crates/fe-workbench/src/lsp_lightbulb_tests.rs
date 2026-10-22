//! The lightbulb through the fake server: asked once the caret has rested,
//! shown for a quick fix, gone for source actions alone, opening Quick Fix
//! on a click, and cleared by the setting or another editor.

use std::{sync::Arc, time::Duration};

use fe_config::{Settings, SettingsEvent};
use fe_gpui::{EditorView, GlyphKind, GutterGlyph};
use gpui::Entity;
use lsp_types::request::{CodeActionRequest, Request};
use serde_json::json;

use super::{asked, fix, reply, setup, showing, titles};
use crate::tests::lsp::Lsp;

const REST: Duration = Duration::from_millis(250);

fn rest(s: &mut Lsp) {
    s.cx.executor()
        .advance_clock(REST + Duration::from_millis(10));
    s.draw();
}

fn glyph(s: &Lsp, editor: &Entity<EditorView>) -> Option<GutterGlyph> {
    editor.read_with(&s.cx, |view, _| view.gutter_glyph())
}

fn move_caret(s: &mut Lsp, editor: &Entity<EditorView>, offset: usize) {
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(offset);
            cx.notify();
        })
    });
    s.draw();
}

fn quick_fix(s: &Lsp) {
    reply(
        s,
        CodeActionRequest::METHOD,
        json!([{ "title": "Fix helper", "kind": "quickfix", "isPreferred": true,
                 "edit": fix(s) }]),
    );
}

fn lightbulb_setting(s: &mut Lsp, on: bool) {
    let mut settings = Settings::default();
    settings.editor.lightbulb = on;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.draw();
}

#[gpui::test]
fn shows_where_actions_exist_after_the_caret_rests(cx: &mut gpui::TestAppContext) {
    let (mut s, editor) = setup(cx);
    quick_fix(&s);
    let helper = super::MAIN.find("helper").unwrap();

    // Two moves inside the wait: one request, after it.
    move_caret(&mut s, &editor, helper + 1);
    move_caret(&mut s, &editor, helper + 3);
    s.cx.executor().advance_clock(Duration::from_millis(200));
    s.draw();
    assert_eq!(glyph(&s, &editor), None);
    assert!(asked(&s, CodeActionRequest::METHOD).is_empty());
    rest(&mut s);
    s.until("the lightbulb", |s| glyph(s, &editor).is_some());
    assert_eq!(
        glyph(&s, &editor),
        Some(GutterGlyph {
            row: 1,
            kind: GlyphKind::LightbulbAutofix
        })
    );
    let sent = asked(&s, CodeActionRequest::METHOD);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["context"]["triggerKind"], 2);
    assert!(
        sent[0]["context"]
            .get("only")
            .is_none_or(|only| only.is_null())
    );

    // A move hides it at once.
    move_caret(&mut s, &editor, helper + 2);
    assert_eq!(glyph(&s, &editor), None);

    // Only a source action shows nothing.
    reply(
        &s,
        CodeActionRequest::METHOD,
        json!([{ "title": "Organize imports", "kind": "source.organizeImports" }]),
    );
    rest(&mut s);
    s.until("the request", |s| {
        asked(s, CodeActionRequest::METHOD).len() == 2
    });
    rest(&mut s);
    assert_eq!(glyph(&s, &editor), None);

    // Nor does none; a plain refactor is the plain lightbulb.
    move_caret(&mut s, &editor, helper + 1);
    reply(&s, CodeActionRequest::METHOD, json!([]));
    rest(&mut s);
    s.until("the request", |s| {
        asked(s, CodeActionRequest::METHOD).len() == 3
    });
    rest(&mut s);
    assert_eq!(glyph(&s, &editor), None);
    move_caret(&mut s, &editor, helper + 2);
    reply(
        &s,
        CodeActionRequest::METHOD,
        json!([{ "title": "Extract", "kind": "refactor.extract" }]),
    );
    rest(&mut s);
    s.until("the lightbulb", |s| glyph(s, &editor).is_some());
    assert_eq!(
        glyph(&s, &editor).map(|g| g.kind),
        Some(GlyphKind::Lightbulb)
    );
}

#[gpui::test]
fn clicking_the_lightbulb_lists_the_quick_fixes(cx: &mut gpui::TestAppContext) {
    let (mut s, editor) = setup(cx);
    quick_fix(&s);
    let helper = super::MAIN.find("helper").unwrap();
    move_caret(&mut s, &editor, helper + 1);
    rest(&mut s);
    s.until("the lightbulb", |s| glyph(s, &editor).is_some());

    let click = s
        .workbench
        .read_with(&s.cx, |w, _| w.lightbulb_click())
        .expect("the glyph has a click");
    s.cx.update(|window, cx| click(window, cx));
    s.until("the picker", showing);
    assert_eq!(titles(&mut s), ["Fix helper"]);
}

#[gpui::test]
fn the_setting_and_other_editors_clear_it(cx: &mut gpui::TestAppContext) {
    let (mut s, editor) = setup(cx);
    quick_fix(&s);
    let helper = super::MAIN.find("helper").unwrap();
    move_caret(&mut s, &editor, helper + 1);
    rest(&mut s);
    s.until("the lightbulb", |s| glyph(s, &editor).is_some());

    // Off: gone, and nothing is asked after further moves.
    lightbulb_setting(&mut s, false);
    assert_eq!(glyph(&s, &editor), None);
    let before = asked(&s, CodeActionRequest::METHOD).len();
    move_caret(&mut s, &editor, helper + 2);
    rest(&mut s);
    rest(&mut s);
    assert_eq!(glyph(&s, &editor), None);
    assert_eq!(asked(&s, CodeActionRequest::METHOD).len(), before);

    // On again, then a split becomes active: the first's is cleared.
    lightbulb_setting(&mut s, true);
    move_caret(&mut s, &editor, helper + 1);
    rest(&mut s);
    s.until("the lightbulb", |s| glyph(s, &editor).is_some());
    s.run("pane.split_right");
    let second = s
        .workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    assert_ne!(second, editor);
    assert_eq!(glyph(&s, &editor), None);
}

#[gpui::test]
fn flipping_a_selection_moves_the_lightbulb_to_the_new_caret_row(cx: &mut gpui::TestAppContext) {
    let (mut s, editor) = setup(cx);
    quick_fix(&s);
    let select = |s: &mut Lsp, reversed: bool| {
        s.cx.update(|_, cx| {
            editor.update(cx, |view, cx| {
                view.model_mut()
                    .select_range(0..super::MAIN.len() - 2, reversed);
                cx.notify();
            })
        });
        s.draw();
    };
    select(&mut s, false);
    rest(&mut s);
    s.until("the lightbulb", |s| glyph(s, &editor).is_some());
    assert_eq!(glyph(&s, &editor).map(|g| g.row), Some(2));

    // The same range with the caret at its start: hidden at once, and shown
    // on the first row once it rests.
    select(&mut s, true);
    assert_eq!(glyph(&s, &editor), None);
    rest(&mut s);
    s.until("the lightbulb", |s| glyph(s, &editor).is_some());
    assert_eq!(glyph(&s, &editor).map(|g| g.row), Some(0));
}

#[gpui::test]
fn a_server_without_code_actions_is_not_asked(cx: &mut gpui::TestAppContext) {
    use fe_lsp::fake::{InitializeReply, Script, initialize_result};
    use gpui::Focusable as _;
    use lsp_types::{CodeActionProviderCapability, ServerCapabilities};

    let mut init = initialize_result(Some(lsp_types::PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        code_action_provider: Some(CodeActionProviderCapability::Simple(false)),
        ..init.capabilities
    };
    let script = Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    };
    let mut s = Lsp::with_scripts(cx, script, Script::default());
    s.set_trust(true);
    std::fs::write(s.path("src/main.rs"), super::MAIN).unwrap();
    s.open("src/main.rs");
    let uri = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some())
        .unwrap();
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    move_caret(&mut s, &editor, 3);
    rest(&mut s);
    rest(&mut s);
    assert_eq!(glyph(&s, &editor), None);
    assert!(asked(&s, CodeActionRequest::METHOD).is_empty());
}
