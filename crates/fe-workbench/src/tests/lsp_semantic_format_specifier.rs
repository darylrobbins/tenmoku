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

/// What the editor draws under each bundled theme: the surrounding string in
/// the string colour, the placeholders and escape in theirs, and `error`
/// (a variable, which no theme colours) in the editor foreground.
#[gpui::test]
fn the_tokens_draw_in_their_resolved_colours_under_every_theme(cx: &mut TestAppContext) {
    use fe_config::EditorPalette;
    use fe_syntax::Highlight;
    use kiln::core::theme::ThemeData;

    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    std::fs::write(s.path("src/main.rs"), format!("{LINE}\n")).unwrap();
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

    let ember = {
        let theme = fe_config::themes::find("ember").expect("a bundled theme");
        EditorPalette::for_theme(&ThemeData::from_file(&theme.file())).0
    };
    let themes = [
        ("dark", EditorPalette::fastedit_dark().clone()),
        ("light", EditorPalette::fastedit_light().clone()),
        ("ember", ember),
    ];
    // `a` of the string's text, which no token covers.
    let string_text = LINE.find("\"a ").unwrap() + 1;
    for (name, palette) in themes {
        editor.update(&mut s.cx, |editor, cx| editor.set_colors(&palette, cx));
        s.draw();
        let (foreground, resolved) = editor.read_with(&s.cx, |editor, _| {
            let theme = editor.theme();
            let colour = |capture: &str| {
                let highlight = Highlight::from_capture(capture).expect("in the vocabulary");
                theme.syntax_color(highlight).unwrap_or(theme.foreground)
            };
            (
                theme.foreground,
                ["string", "string.placeholder", "escape"].map(colour),
            )
        });
        let [string, placeholder, escape] = resolved;
        assert_ne!(string, foreground, "{name}: the theme colours strings");
        assert_ne!(placeholder, string, "{name}: placeholders stand apart");
        assert_ne!(escape, string, "{name}: escapes stand apart");
        let drawn = |offset: usize| {
            editor
                .read_with(&s.cx, |editor, _| editor.drawn_color(offset))
                .unwrap_or_else(|| panic!("{name}: offset {offset} was not drawn"))
        };
        assert_eq!(drawn(string_text), string, "{name}: the string");
        for (needle, nth) in [("{", 0), ("}", 0), ("{", 1), ("}", 1)] {
            let offset = at(needle, nth) as usize;
            assert_eq!(drawn(offset), placeholder, "{name}: `{needle}` #{nth}");
        }
        let error = at("error", 0) as usize;
        for offset in error..error + 5 {
            assert_eq!(drawn(offset), foreground, "{name}: `error` at {offset}");
        }
        let escape_at = at("\\n", 0) as usize;
        for offset in escape_at..escape_at + 2 {
            assert_eq!(drawn(offset), escape, "{name}: the escape at {offset}");
        }
        // The text between the tokens keeps the string colour.
        assert_eq!(drawn(at(" ", 1) as usize), string, "{name}: between");
    }
}
