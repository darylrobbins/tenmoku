//! A file of a language fe-syntax has no grammar for goes to the language
//! server configured for it, by its extension: it's opened, synced and asked
//! as any other (T221).

use std::{path::Path, sync::Arc, time::Duration};

use fe_command::Invocation;
use fe_config::{Settings, SettingsEvent, settings::LspServerSettings};
use fe_gpui::commands::RunCommand;
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Focusable as _, TestAppContext};
use lsp_types::{FoldingRangeProviderCapability, PositionEncodingKind, ServerCapabilities};
use serde_json::json;

use crate::tests::lsp::Lsp;

const FOLDING: &str = "textDocument/foldingRange";
const NOTES: &str = "begin\n  one\n  two\nend\n";

fn script() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

/// Reloads `s`'s settings with a server `zzls` for the `zz` language, in
/// the user's file too, which a trusted root's are read from.
fn configure_zzls(s: &mut Lsp) {
    s.write_user_settings("[lsp.servers.zzls]\ncommand = \"zzls\"\nlanguages = [\"zz\"]\n");
    let mut settings = Settings::default();
    settings.lsp.servers.insert(
        "zzls".to_owned(),
        LspServerSettings {
            command: Some("zzls".into()),
            languages: Some(vec!["zz".into()]),
            ..LspServerSettings::default()
        },
    );
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.draw();
}

fn requests(s: &Lsp) -> usize {
    (s.fake.records().iter())
        .filter(|record| matches!(record, Record::Request { method, .. } if method == FOLDING))
        .count()
}

fn run(s: &mut Lsp, command: &str) {
    let invocation = Invocation::of(command).unwrap();
    s.cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    s.draw();
}

/// What Fold All folds, undone again.
fn fold_all(s: &mut Lsp) -> Vec<(u32, u32)> {
    run(s, "editor.fold_all");
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("shown");
    let folded = editor.read_with(&s.cx, |view, _| {
        let folds = view.model().display().folds();
        folds.iter().map(|fold| (fold.start, fold.end)).collect()
    });
    run(s, "editor.unfold_all");
    folded
}

#[gpui::test]
fn a_file_without_a_grammar_goes_to_its_configured_server(cx: &mut TestAppContext) {
    assert_eq!(
        fe_syntax::language_name_for_path(Path::new("notes.zz")),
        None
    );
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    s.fake.set_reply(
        FOLDING,
        Scripted::now(Reply::Result(json!([{ "startLine": 0, "endLine": 3 }]))),
    );
    configure_zzls(&mut s);
    s.set_trust(true);
    std::fs::write(s.path("notes.zz"), NOTES).unwrap();
    s.open("notes.zz");
    let notes = s.uri("notes.zz");

    // Opened on the configured server, under the language id it lists.
    let opened = s
        .fake
        .wait_for_document(&notes, "didOpen", |r| r.is_some_and(|r| r.open))
        .expect("opened");
    assert_eq!(opened.language_id, "zz");
    assert_eq!(opened.text, NOTES);
    assert_eq!(s.fake.launched()[0].0.name, "zzls");
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("notes.zz is shown");
    assert!(editor.read_with(&s.cx, |view, _| view.syntax().is_none()));

    // Its folds are the server's.
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.until("the server's folds", |s| {
        s.clock.advance(Duration::from_millis(20));
        fold_all(s) == [(0, 3)]
    });
    assert_eq!(requests(&s), 1);

    // Edits are synced.
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(0);
            view.model_mut().insert("x");
            cx.notify();
        })
    });
    s.draw();
    s.fake.wait_for_document(&notes, "didChange", |r| {
        r.is_some_and(|r| r.text == format!("x{NOTES}"))
    });
}

#[gpui::test]
fn a_file_without_a_grammar_or_a_server_starts_none(cx: &mut TestAppContext) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    configure_zzls(&mut s);
    s.set_trust(true);
    std::fs::write(s.path("notes.yy"), NOTES).unwrap();
    s.open("notes.yy");
    for _ in 0..5 {
        s.clock.advance(Duration::from_millis(20));
        s.draw();
    }
    assert_eq!(s.fake.launches(), 0);
}

/// `s` with `zzls` configured, trusted, and `notes.zz` open on it.
fn notes_on_zzls(cx: &mut TestAppContext) -> (Lsp, gpui::Entity<fe_gpui::EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    configure_zzls(&mut s);
    s.set_trust(true);
    std::fs::write(s.path("notes.zz"), NOTES).unwrap();
    s.open("notes.zz");
    let notes = s.uri("notes.zz");
    s.fake
        .wait_for_document(&notes, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("notes.zz is shown");
    (s, editor)
}

fn set_language(s: &mut Lsp, editor: &gpui::Entity<fe_gpui::EditorView>, name: Option<&str>) {
    s.cx.update(|_, cx| editor.update(cx, |view, cx| view.set_language(name, cx)));
    s.draw();
}

/// A few frames on the manager's clock, for anything that would reopen a
/// document to have.
fn settle(s: &mut Lsp) {
    for _ in 0..5 {
        s.clock.advance(Duration::from_millis(20));
        s.draw();
    }
}

#[gpui::test]
fn plain_text_by_hand_closes_a_file_without_a_grammar(cx: &mut TestAppContext) {
    let (mut s, editor) = notes_on_zzls(cx);
    let notes = s.uri("notes.zz");
    set_language(&mut s, &editor, None);
    s.fake
        .wait_for_document(&notes, "didClose", |r| r.is_some_and(|r| !r.open));
    settle(&mut s);
    let replica = s.fake.wait_for_document(&notes, "closed", |r| r.is_some());
    assert!(!replica.expect("opened once").open);
    assert_eq!(s.fake.launches(), 1);
}

#[gpui::test]
fn plain_text_after_a_grammar_by_hand_attaches_none(cx: &mut TestAppContext) {
    assert!(fe_syntax::language_by_name("python").is_some());
    let (mut s, editor) = notes_on_zzls(cx);
    let notes = s.uri("notes.zz");

    // Python by hand: off zzls, onto pyright.
    set_language(&mut s, &editor, Some("python"));
    s.fake
        .wait_for_document(&notes, "didClose", |r| r.is_some_and(|r| !r.open));
    s.python
        .wait_for_document(&notes, "didOpen", |r| r.is_some_and(|r| r.open));

    // Then plain text by hand: off pyright, and not back on zzls.
    set_language(&mut s, &editor, None);
    s.python
        .wait_for_document(&notes, "didClose", |r| r.is_some_and(|r| !r.open));
    settle(&mut s);
    let zz = s.fake.wait_for_document(&notes, "closed", |r| r.is_some());
    assert!(!zz.expect("opened once").open);
    let py = s
        .python
        .wait_for_document(&notes, "closed", |r| r.is_some());
    assert!(!py.expect("opened once").open);
}
