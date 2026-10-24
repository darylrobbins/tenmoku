//! rust-analyzer's `formatSpecifier` and `escapeSequence` tokens colour a
//! string's placeholders and escapes apart from the string.

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
    options.legend.token_types.extend([
        SemanticTokenType::new("formatSpecifier"),
        SemanticTokenType::new("escapeSequence"),
    ]);
    script
}

/// A log line: `{}` and `{error}` placeholders, `error` inlined, a `\n`.
const LINE: &str = r#"log::warn!("a {} {error}\n", x);"#;

#[gpui::test]
fn a_format_specifier_token_in_a_string_is_coloured_apart_from_the_string(cx: &mut TestAppContext) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    std::fs::write(s.path("src/main.rs"), format!("{LINE}\n")).unwrap();
    // Tokens as rust-analyzer sends them: braces 3 and 4 (specifier), `error`
    // 3 (variable), `\n` 5 (escape), left to right on one line.
    let at = |needle: &str, nth: usize| LINE.match_indices(needle).nth(nth).unwrap().0 as u32;
    let tokens = [
        (at("{", 0), 1, 3),
        (at("}", 0), 1, 3),
        (at("{", 1), 1, 3),
        (at("error", 0), 5, 2),
        (at("}", 1), 1, 3),
        (at("\\n", 0), 2, 4),
    ];
    let (mut data, mut prev) = (Vec::new(), 0);
    for (col, len, ty) in tokens {
        data.extend([0, col - prev, len, ty, 0]);
        prev = col;
    }
    s.fake.set_reply(
        FULL,
        Scripted::now(Reply::Result(json!({ "resultId": "1", "data": data }))),
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
    s.until("the tokens", |s| !painted(s, &editor).is_empty());
    let range = |needle: &str, nth: usize, len: usize| {
        let start = at(needle, nth) as usize;
        start..start + len
    };
    assert_eq!(
        painted(&s, &editor),
        vec![
            (range("{", 0, 1), "string.placeholder".to_owned()),
            (range("}", 0, 1), "string.placeholder".to_owned()),
            (range("{", 1, 1), "string.placeholder".to_owned()),
            (range("error", 0, 5), "variable".to_owned()),
            (range("}", 1, 1), "string.placeholder".to_owned()),
            (range("\\n", 0, 2), "escape".to_owned()),
        ]
    );
}
