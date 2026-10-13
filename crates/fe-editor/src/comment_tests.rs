use super::*;
use crate::{
    commands::{self, EditorHost, EditorTarget},
    tests::{parse, render},
};
use fe_command::Invocation;
use std::ops::{Range, RangeInclusive};

const LINE: CommentTokens = CommentTokens {
    line: Some("//"),
    block: Some(("/*", "*/")),
};
const BLOCK: CommentTokens = CommentTokens {
    line: None,
    block: Some(("/*", "*/")),
};

/// A host that knows a language's comment tokens.
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

fn toggled(marked: &str, tokens: CommentTokens) -> String {
    let mut model = parse(marked);
    model.toggle_comment(tokens);
    render(&model)
}

#[test]
fn line_comments() {
    let mut model = parse("  [a\n\n    b]");
    model.toggle_comment(LINE);
    assert_eq!(render(&model), "  // [a\n\n  //   b]");
    model.toggle_comment(LINE);
    assert_eq!(render(&model), "  [a\n\n    b]");
    // A caret at the inserted token's position stays on its character.
    assert_eq!(toggled("  |a", LINE), "  // |a");
    assert_eq!(toggled("  a|", LINE), "  // a|");
    // Only the touched rows change.
    assert_eq!(toggled("a\n|b\nc", LINE), "a\n// |b\nc");
    assert_eq!(toggled("// |a", LINE), "|a");
}

#[test]
fn mixed_rows_comment_all() {
    assert_eq!(toggled("[// a\nb]", LINE), "// [// a\n// b]");
}

#[test]
fn block_only_and_none() {
    let mut model = parse("[a\n  b]");
    model.toggle_comment(BLOCK);
    assert_eq!(render(&model), "/* [a */\n  /* b */]");
    model.toggle_comment(BLOCK);
    assert_eq!(render(&model), "[a\n  b]");

    let none = CommentTokens {
        line: None,
        block: None,
    };
    let run = |tokens: Option<CommentTokens>| {
        let mut model = parse("|a");
        let mut host = TokenHost(tokens);
        let invocation = Invocation::of("editor.toggle_comment")
            .unwrap()
            .context(commands::context());
        let effect = commands::registry()
            .dispatch(&mut EditorTarget::new(&mut model, &mut host), &invocation)
            .unwrap()
            .effect;
        (model, effect)
    };
    for tokens in [None, Some(none)] {
        let (model, effect) = run(tokens);
        assert_eq!(render(&model), "|a");
        assert_eq!(effect, Effect::Nothing);
    }

    let (mut model, effect) = run(Some(LINE));
    assert_eq!(render(&model), "// |a");
    assert_eq!(effect, Effect::Changed);
    model.undo().unwrap();
    assert_eq!(render(&model), "|a");
}
