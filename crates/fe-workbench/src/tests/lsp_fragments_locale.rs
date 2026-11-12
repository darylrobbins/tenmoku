//! A user snippet's detail in Insert Snippet and the empty answer of the
//! kind-specific code action commands come whole from the catalogue
//! (T356): under the fake locale, `qps-ploc`, no English fragment is left.

use std::fs;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_lsp::fake::{InitializeReply, Reply, Script, Scripted, initialize_result};
use gpui::{Focusable as _, TestAppContext};
use lsp_types::{
    CodeActionProviderCapability, ServerCapabilities,
    request::{CodeActionRequest, Request},
};
use serde_json::json;

use crate::snippet_picker::snippet_rows;
use crate::tests::lsp::Lsp;
use crate::user_snippets_tests::write_snippets;

static FAKE_LOCALE: kiln_l10n::Resources = kiln_l10n::Resources {
    crate_name: "fe-workbench-lsp-fragments-fake-locale",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[kiln_l10n::ResourceFile {
        locale: "qps-ploc",
        path: "qps-ploc/lsp-fragments.ftl",
        source: "workbench-extensions-user-snippet = ⟦Úsér sñíppét⟧\n\
                 workbench-lsp-code-actions-none-of-kind =\n    \
                 { $kind ->\n        \
                 [organize-imports] ⟦Ñó órgáñízé ímpórts⟧\n        \
                 [fix-all] ⟦Ñó fíx áll⟧\n        \
                 [refactor] ⟦Ñó réfáctór⟧\n       \
                 *[source] ⟦Ñó sóúrcé⟧\n    }\n",
    }],
    dynamic_prefixes: &[],
};

kiln_l10n::__private::inventory::submit! { kiln_l10n::Registered(&FAKE_LOCALE) }

fn fake_locale(s: &mut Lsp) {
    let tags: Vec<kiln_l10n::LanguageIdentifier> = vec!["qps-ploc".parse().unwrap()];
    s.cx.update(|_, cx| kiln_platform::locale::set_locale(cx, &tags));
    s.draw();
}

#[gpui::test]
fn a_user_snippet_without_a_description_has_the_fake_detail(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    write_snippets(
        &s,
        "global.code-snippets",
        r#"{ "Todo": { "prefix": "todo", "body": "TODO: $0" } }"#,
    );
    fake_locale(&mut s);
    let details: Vec<String> = s.cx.update(|_, cx| {
        snippet_rows(None, cx)
            .into_iter()
            .map(|r| r.detail)
            .collect()
    });
    assert_eq!(details, ["⟦Úsér sñíppét⟧"]);
}

#[gpui::test]
fn an_empty_code_action_answer_is_a_whole_fake_message(cx: &mut TestAppContext) {
    let mut init = initialize_result(Some(lsp_types::PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        ..init.capabilities
    };
    let script = Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    };
    let mut s = Lsp::with_scripts(cx, script, Script::default());
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), "fn main() {}\n").unwrap();
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
    s.fake.set_reply(
        CodeActionRequest::METHOD,
        Scripted::now(Reply::Result(json!([]))),
    );
    fake_locale(&mut s);

    for (command, message) in [
        ("lsp.organize_imports", "⟦Ñó órgáñízé ímpórts⟧"),
        ("lsp.fix_all", "⟦Ñó fíx áll⟧"),
        ("lsp.refactor", "⟦Ñó réfáctór⟧"),
        ("lsp.source_action", "⟦Ñó sóúrcé⟧"),
    ] {
        let context = ContextStack::new()
            .with(ContextFrame::new(crate::KEY_CONTEXT))
            .with(ContextFrame::new(fe_editor::commands::CONTEXT));
        let run = RunCommand(Invocation::of(command).unwrap().context(context));
        s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
        s.until(command, |s| s.cx.pending_prompt().is_some());
        let (question, _) = s.cx.pending_prompt().unwrap();
        assert_eq!(question, message, "{command}");
        s.cx.simulate_prompt_answer("OK");
        s.draw();
    }
}
