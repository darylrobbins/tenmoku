//! Finding a body below its header past a large prefix of comments, through
//! both candidate APIs. Stepping back over each comment to the header once
//! rescanned the comments before it, so these exercise that lookup at scale.

use std::path::Path;

use fe_buffer::Buffer;

use super::*;
use crate::language_for_path;

/// Comments between the header and its body.
const COMMENTS: usize = 10_000;

/// `text` parsed in the language `file` names, if it's in this build. The
/// parse isn't capped: tree-sitter-python takes time quadratic in the
/// comments between a `:` and its block, seconds for these in a debug
/// build, which a loaded machine can push past the cap.
fn parsed(file: &str, text: &str) -> Option<Syntax> {
    let language = language_for_path(Path::new(file))?;
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run_until(|_| || false));
    Some(syntax)
}

/// Asserts that a header, `COMMENTS` comments and a body, its last row of
/// text right after them, fold from the header to the body, both from the
/// header row alone and from rows along the way.
fn assert_body_past_comments(file: &str, header: &str, comment: &str, body: &str) {
    let text = format!("{header}{}{body}", comment.repeat(COMMENTS));
    let Some(syntax) = parsed(file, &text) else {
        return;
    };
    assert!(syntax.tree().is_some(), "{file}: parsed");
    let last = COMMENTS as u32 + 1;
    let header_row: Vec<_> = fold_ranges(&syntax, 0..1)
        .into_iter()
        .map(|range| (*range.start(), *range.end()))
        .collect();
    assert_eq!(header_row, [(0, last)], "{file}: header row");
    // The header, the first, a middle and the last comment, and the body.
    for row in [0, 1, last / 2, last - 1, last] {
        assert_eq!(
            fold_range_at(&syntax, row),
            Some(0..=last),
            "{file} row {row}"
        );
    }
}

#[test]
fn python_body_past_many_comments() {
    assert_body_past_comments("a.py", "def f():\n", "    # c\n", "    x()\n");
}

#[test]
fn lua_body_past_many_comments() {
    assert_body_past_comments("a.lua", "function f()\n", "-- c\n", "x()\nend");
}
