//! `auto_pairs`: Python, Ruby, Swift and JSON close a typed backtick as VS Code's
//! language configurations do.

use fe_syntax::auto_pairs;

const WITH_BACKTICK: [(char, char); 6] = [
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('"', '"'),
    ('\'', '\''),
    ('`', '`'),
];

#[test]
fn python_ruby_swift_and_json_close_backticks() {
    for name in ["python", "ruby", "swift", "json"] {
        assert_eq!(auto_pairs(name), WITH_BACKTICK, "{name}");
    }
    for name in ["rust", "lua", "yaml", "css", "c", "java"] {
        assert!(!auto_pairs(name).contains(&('`', '`')), "{name}");
    }
}
