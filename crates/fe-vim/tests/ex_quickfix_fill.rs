//! `:vimgrep`'s argument split into its pattern, flags and globs, as nvim
//! 0.12.4's `skip_vimgrep_pat()` splits it, for the host to search with.

use fe_vim::{ExError, QuickfixAction};

fn split(args: &str) -> Result<(String, String, Vec<String>), ExError> {
    QuickfixAction::parse_vimgrep(args)
}

fn parts(
    pattern: &str,
    flags: &str,
    globs: &[&str],
) -> Result<(String, String, Vec<String>), ExError> {
    let globs = globs.iter().map(|glob| (*glob).to_owned()).collect();
    Ok((pattern.to_owned(), flags.to_owned(), globs))
}

#[test]
fn vimgrep_splits_pattern_flags_and_globs() {
    assert_eq!(split("/foo/ **/*.rs"), parts("foo", "", &["**/*.rs"]));
    assert_eq!(
        split("/foo/g **/*.rs *.md"),
        parts("foo", "g", &["**/*.rs", "*.md"])
    );
    assert_eq!(split("/foo/jg\t*.md"), parts("foo", "jg", &["*.md"]));
    assert_eq!(split("/a b/j"), parts("a b", "j", &[]));
    assert_eq!(split("/foo/"), parts("foo", "", &[]));
    // Nothing need come between the flags and a glob.
    assert_eq!(split("/foo/gx"), parts("foo", "g", &["x"]));
    // A pattern starting with a word char is the first word, no flags.
    assert_eq!(split("foo **/*.rs"), parts("foo", "", &["**/*.rs"]));
    assert_eq!(split("_x9 g"), parts("_x9", "", &["g"]));
    assert_eq!(split("foo"), parts("foo", "", &[]));
    // Escaped delimiters and collections stay in the pattern, as typed.
    assert_eq!(split(r"/a\/b/ x"), parts(r"a\/b", "", &["x"]));
    assert_eq!(split("/[/]x/ y"), parts("[/]x", "", &["y"]));
    assert_eq!(split("/[]/]/ y"), parts("[]/]", "", &["y"]));
    assert_eq!(split("/[^]/]/ y"), parts("[^]/]", "", &["y"]));
    // An unclosed `[` is a plain one.
    assert_eq!(split("/a[b/ y"), parts("a[b", "", &["y"]));
    // An empty pattern is the host's to fill from the last search.
    assert_eq!(split("// *.rs"), parts("", "", &["*.rs"]));
    // An escaped blank stays in its glob.
    assert_eq!(
        split(r"/x/ a\ b.txt c"),
        parts("x", "", &[r"a\ b.txt", "c"])
    );
    assert_eq!(
        split("/foo"),
        Err(ExError::Pattern(
            "E682: Invalid search pattern or delimiter".into()
        ))
    );
    assert!(matches!(split("/foo/f x"), Err(ExError::Unsupported(_))));
    assert_eq!(split(""), Err(ExError::ArgumentRequired));
}

#[test]
fn vimgrep_accepts_other_delimiters() {
    assert_eq!(split("#foo#g *.rs"), parts("foo", "g", &["*.rs"]));
    assert_eq!(split("!a/b! **"), parts("a/b", "", &["**"]));
    assert_eq!(split("|a|b| x"), parts("a", "", &["b|", "x"]));
    assert_eq!(split(r"+a\+b+j"), parts(r"a\+b", "j", &[]));
    assert_eq!(split("ébé x"), parts("ébé", "", &["x"]), "é is a word char");
    assert_eq!(split("→a→ x"), parts("a", "", &["x"]));
}
