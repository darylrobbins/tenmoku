//! The `mutable` modifier underlines a token, with or without a colour for
//! its type.

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
        .token_types
        .push(SemanticTokenType::new("label"));
    options
        .legend
        .token_modifiers
        .push(SemanticTokenModifier::new("mutable"));
    script
}

/// `fn main() {}`: `fn` a mutable variable, `main` a variable, `{}` a
/// mutable label (a type with no colour).
fn setup(cx: &mut TestAppContext) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    s.fake.set_reply(
        FULL,
        Scripted::now(Reply::Result(json!({
            "resultId": "1",
            "data": [0, 0, 2, 2, 2, 0, 3, 4, 2, 0, 0, 7, 2, 3, 2],
        }))),
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
    (s, editor)
}

/// The `Underline` decorations: bytes, wavy and tint.
fn underlined(s: &Lsp, editor: &Entity<EditorView>) -> Vec<(std::ops::Range<usize>, bool, Tint)> {
    editor.read_with(&s.cx, |view, _| {
        let model = view.model();
        let mut found: Vec<_> = (model
            .decorations(0..model.snapshot().text().len())
            .into_iter())
        .filter_map(|item| match item.kind {
            DecorationKind::Underline { wavy } => Some((item.range, wavy, item.tint)),
            _ => None,
        })
        .collect();
        found.sort_by_key(|(range, ..)| range.start);
        found
    })
}

#[gpui::test]
fn underline(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    s.until("the tokens", |s| !painted(s, &editor).is_empty());
    // `main` has no `mutable`, so no underline; `{}` has no colour.
    assert_eq!(
        underlined(&s, &editor),
        vec![(0..2, false, Tint::Muted), (10..12, false, Tint::Muted)]
    );
    assert_eq!(
        painted(&s, &editor),
        vec![(0..2, "variable".to_owned()), (3..7, "variable".to_owned()),]
    );

    set_setting(&mut s, false);
    assert_eq!(underlined(&s, &editor), vec![]);
}
