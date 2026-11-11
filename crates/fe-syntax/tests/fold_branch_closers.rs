//! The closing row of a fold whose closer belongs to an enclosing statement
//! the body is reached through a branch wrapper of: a final `else` or
//! `elseif` body in Lua and PHP's alternative syntax.

use std::path::Path;

use fe_buffer::Buffer;
use fe_syntax::{Syntax, fold_candidate_closing, language_for_path};

/// `(fold range, closing row)` of the candidate headed by `row` in `text`,
/// parsed in the language `file` names.
fn closing(file: &str, text: &str, row: u32) -> Option<((u32, u32), u32)> {
    let language = language_for_path(Path::new(file))?;
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    let (range, closing) = fold_candidate_closing(&syntax, row)?;
    Some(((*range.start(), *range.end()), closing))
}

#[test]
fn lua_final_else_body_closes_on_the_if_end() {
    let text = "if a then\n  x()\nelseif b then\n  y()\nelse\n  z()\nend\n";
    // The if body and a non-final elseif body end where the next branch starts.
    assert_eq!(closing("a.lua", text, 0), Some(((0, 1), 1)));
    assert_eq!(closing("a.lua", text, 2), Some(((2, 3), 3)));
    assert_eq!(closing("a.lua", text, 4), Some(((4, 5), 6)));
}

#[test]
fn lua_elseif_body_closes_on_the_if_end() {
    let text = "if a then\n  x()\nelseif b then\n  y()\nend\n";
    assert_eq!(closing("a.lua", text, 2), Some(((2, 3), 4)));
}

#[test]
fn lua_if_body_closed_by_its_own_end_is_unchanged() {
    let text = "if a then\n  x()\nend\n";
    assert_eq!(closing("a.lua", text, 0), Some(((0, 1), 2)));
}

#[test]
fn python_if_else_has_no_closer() {
    let text = "if a:\n    x()\nelse:\n    y()\n";
    assert_eq!(closing("a.py", text, 0), Some(((0, 1), 1)));
    assert_eq!(closing("a.py", text, 2), Some(((2, 3), 3)));
}

#[cfg(feature = "php")]
#[test]
fn php_alternative_syntax_else_bodies_close_on_endif() {
    let text = "<?php\nif ($a):\n  x();\nelseif ($b):\n  y();\nelse:\n  z();\nendif;\n";
    assert_eq!(closing("a.php", text, 3), Some(((3, 4), 4)));
    assert_eq!(closing("a.php", text, 5), Some(((5, 6), 7)));
    let text = "<?php\nif ($a):\n  x();\nelseif ($b):\n  y();\nendif;\n";
    assert_eq!(closing("a.php", text, 3), Some(((3, 4), 5)));
}
