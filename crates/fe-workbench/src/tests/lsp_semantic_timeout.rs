//! An ask for semantic tokens that times out is asked again, so the tokens
//! a busy server (rust-analyzer analysing) answers later still land.

use super::*;

fn script() -> Script {
    let mut script = super::script();
    let init = match &mut script.initialize {
        InitializeReply::Result(init) => init,
        _ => unreachable!(),
    };
    let Some(SemanticTokensServerCapabilities::SemanticTokensOptions(options)) =
        &mut init.capabilities.semantic_tokens_provider
    else {
        unreachable!()
    };
    options
        .legend
        .token_modifiers
        .push(SemanticTokenModifier::new("mutable"));
    script
}

#[gpui::test]
fn a_timed_out_ask_is_asked_again_and_its_mutable_underline_lands(cx: &mut TestAppContext) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    // The first answer comes after `lsp.timeouts.hover` (3 s).
    s.fake.set_reply(
        FULL,
        Scripted {
            reply: Reply::Result(json!({ "resultId": "1", "data": [0, 0, 2, 1, 0] })),
            delay: Duration::from_secs(10),
        },
    );
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    s.until("the first ask", |s| {
        s.clock.advance(Duration::from_millis(20));
        s.cx.executor().advance_clock(REST);
        !asked(s).is_empty()
    });

    // The next ask is answered at once, with `fn` a mutable variable.
    s.fake.set_reply(
        FULL,
        Scripted::now(Reply::Result(json!({
            "resultId": "2",
            "data": [0, 0, 2, 2, 2],
        }))),
    );
    s.until("the second ask", |s| {
        s.clock.advance(Duration::from_millis(500));
        s.cx.executor().advance_clock(REST);
        asked(s).len() >= 2
    });
    s.until("the second answer", |s| !painted(s, &editor).is_empty());
    assert_eq!(painted(&s, &editor), vec![(0..2, "variable".to_owned())]);
    let underlines = editor.read_with(&s.cx, |view, _| {
        let model = view.model();
        (model
            .decorations(0..model.snapshot().text().len())
            .into_iter())
        .filter(|item| matches!(item.kind, DecorationKind::Underline { .. }))
        .map(|item| item.range)
        .collect::<Vec<_>>()
    });
    assert_eq!(underlines, vec![0..2]);
}
