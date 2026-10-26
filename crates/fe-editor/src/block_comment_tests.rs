use super::*;
use crate::{
    commands::{self, EditorHost, EditorTarget},
    tests::{parse, render},
};
use fe_command::Invocation;
use std::ops::RangeInclusive;

const C: CommentTokens = CommentTokens {
    line: Some("//"),
    block: Some(("/*", "*/")),
};
const HTML: CommentTokens = CommentTokens {
    line: None,
    block: Some(("<!--", "-->")),
};
const NONE: CommentTokens = CommentTokens {
    line: Some("#"),
    block: None,
};

struct TokenHost(Option<CommentTokens>);

impl EditorHost for TokenHost {
    fn page_rows(&self) -> u32 {
        20
    }
    fn read_clipboard(&mut self) -> Option<String> {
        None
    }
    fn write_clipboard(&mut self, _: String) {}
    fn fold_range_at(&self, _: u32) -> Option<RangeInclusive<u32>> {
        None
    }
    fn fold_ranges(&self, _: Range<u32>) -> Vec<RangeInclusive<u32>> {
        Vec::new()
    }
    fn comment_tokens(&self) -> Option<CommentTokens> {
        self.0
    }
}

fn run(model: &mut EditorModel, tokens: CommentTokens) -> Effect {
    let mut host = TokenHost(Some(tokens));
    let invocation = Invocation::of("editor.toggle_block_comment")
        .unwrap()
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(model, &mut host), &invocation)
        .unwrap()
        .effect
}

fn toggled(marked: &str, tokens: CommentTokens) -> String {
    let mut model = parse(marked);
    run(&mut model, tokens);
    render(&model)
}

#[test]
fn wraps_the_selection() {
    let mut model = parse("a [b] c");
    run(&mut model, C);
    assert_eq!(render(&model), "a /* [b] */ c");
    run(&mut model, C);
    assert_eq!(render(&model), "a [b] c");
    assert_eq!(toggled("a [/* b */] c", C), "a [b] c");
    assert_eq!(toggled("a /*[ b ]*/ c", C), "a [b] c");
    assert_eq!(toggled("a /* [b] */ c", C), "a [b] c");
    assert_eq!(toggled("a/*[b]*/c", C), "a[b]c");
}

#[test]
fn caret_wraps_the_row() {
    assert_eq!(toggled("  a| b", C), "  /* a| b */");
    assert_eq!(toggled("  |a b  ", C), "  /* |a b */  ");
    assert_eq!(toggled("  /* a| b */", C), "  a| b");
    assert_eq!(toggled("x /* a */ y|", C), "/* x /* a */ y| */");
    assert_eq!(toggled("x /* a| */ y", C), "x a| y");
    assert_eq!(toggled("  |", C), "  |");
}

#[test]
fn no_block_tokens() {
    let mut model = parse("[a] b");
    assert_eq!(run(&mut model, NONE), Effect::Nothing);
    assert_eq!(render(&model), "[a] b");
    assert_eq!(toggled("a| b", HTML), "<!-- a| b -->");
    assert_eq!(toggled("<!-- a| b -->", HTML), "a| b");
    let mut model = parse("|a");
    let mut host = TokenHost(None);
    let invocation = Invocation::of("editor.toggle_block_comment")
        .unwrap()
        .context(commands::context());
    let effect = commands::registry()
        .dispatch(&mut EditorTarget::new(&mut model, &mut host), &invocation)
        .unwrap()
        .effect;
    assert_eq!(effect, Effect::Nothing);
    assert_eq!(render(&model), "|a");
}

#[test]
fn many_selections() {
    let mut model = parse("[a] b [c]");
    assert_eq!(run(&mut model, C), Effect::Changed);
    assert_eq!(render(&model), "/* [a] */ b /* [c] */");
    model.undo().unwrap();
    assert_eq!(model.text().to_string(), "a b c");
    // Two carets on one row wrap it once.
    assert_eq!(toggled("|a |b", C), "/* |a |b */");
    // Each selection decides for itself.
    assert_eq!(toggled("/* [a] */\n[b]", C), "[a]\n/* [b] */");
}

#[test]
fn multibyte_next_to_the_delimiters() {
    assert_eq!(toggled("/*é*|/", C), "é|");
    assert_eq!(toggled("[/*é*/]", C), "[é]");
    assert_eq!(toggled("/* a é*/|", C), "a é|");
}

#[test]
fn adjacent_selections() {
    let mut model = parse("[a][b]");
    run(&mut model, C);
    let text = model.text().to_string();
    assert_eq!(text, "/* a *//* b */");
    let selected: Vec<_> = model
        .selections()
        .iter()
        .map(|s| text[s.range.clone()].to_string())
        .collect();
    assert_eq!(selected, ["a", "b"]);
    run(&mut model, C);
    assert_eq!(model.text().to_string(), "ab");
    model.undo().unwrap();
    assert_eq!(model.text().to_string(), "/* a *//* b */");
    model.undo().unwrap();
    assert_eq!(model.text().to_string(), "ab");
}
