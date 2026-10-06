//! TypeScript instantiation expressions around a variable's function
//! initializer: `(function <T>(x: T) {})<string>` is still a function, so
//! symbols don't list a Variable for it, as for the other wrappers, and the
//! outline's Functions for unwrapped generic initializers stay.

#![cfg(feature = "typescript")]

use fe_buffer::Buffer;
use fe_syntax::{Outline, OutlineKind, Syntax, language_by_name, outline, symbols};

/// `text` parsed whole as `language`.
fn parsed(language: &str, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run());
    assert!(
        !syntax.tree().unwrap().root_node().has_error(),
        "the {language} text parses without errors"
    );
    syntax
}

/// The names of `outline`'s items of kind `kind`, in order.
fn names(outline: &Outline, kind: OutlineKind) -> Vec<&str> {
    (outline.items.iter())
        .filter(|item| item.kind == kind)
        .map(|item| item.name.as_str())
        .collect()
}

// The arrow's type parameter list takes a trailing comma so TSX doesn't
// read it as an element.
const TEXT: &str = "\
const f = (function <T>(x: T) { return x; })<string>;
export const ef = (function <T>(x: T) { return x; })<string>;
const a = (<T,>(x: T) => x)<number>;
export const ea = (<T,>(x: T) => x)<number>;
let g = (function* <T>(x: T) { yield x; })<boolean>;
export var eg = (function* <T>(x: T) { yield x; })<boolean>;
const nested = ((<T,>(x: T) => x)<number>);
const asserted = ((<T,>(x: T) => x) as F)<number>;
const plain = id<number>;
const direct = <T,>(x: T) => x;
export const edirect = function <T>(x: T) { return x; };
";

#[test]
fn instantiated_function_initializers_are_not_variables() {
    for language in ["typescript", "tsx"] {
        let syntax = parsed(language, TEXT);
        let symbols = symbols(&syntax).unwrap();
        assert_eq!(
            names(&symbols, OutlineKind::Variable),
            ["plain"],
            "{language}"
        );
        let outline = outline(&syntax).unwrap();
        assert_eq!(
            names(&outline, OutlineKind::Function),
            ["direct", "edirect"],
            "{language}"
        );
        assert_eq!(
            names(&symbols, OutlineKind::Function),
            names(&outline, OutlineKind::Function),
            "{language}"
        );
    }
}
