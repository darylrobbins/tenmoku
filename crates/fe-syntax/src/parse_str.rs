use fe_buffer::Text;

use crate::{Language, runtime::ThreadParser, syntax::parse_text};

/// Parses `text` in `language` on the calling thread, WebAssembly grammars
/// included, with the grammar deadlines and panic handling a [`Syntax`]
/// parse has. `out_of_time` is asked as the parse runs; once it says `true`
/// the parse stops. `None` if it stopped, a WebAssembly parse deadline
/// passed, or the language can't be set on a parser.
///
/// [`Syntax`]: crate::Syntax
pub fn parse_str(
    language: &Language,
    text: &str,
    out_of_time: impl FnMut() -> bool,
) -> Option<tree_sitter::Tree> {
    let mut parser = ThreadParser::for_language(language).ok()?;
    parse_text(&mut parser, &Text::new(text), None, out_of_time)
}

#[cfg(all(test, feature = "rust"))]
mod tests {
    use super::*;

    /// A grammar of ABI version 0, which no tree-sitter parser accepts.
    #[repr(align(8))]
    struct Fake([u8; 1024]);
    static FAKE: Fake = Fake([0; 1024]);

    #[test]
    fn parse_str_returns_none_for_a_language_with_no_grammar_set_error_free() {
        let mut language =
            crate::Language::new("fake", tree_sitter_rust::LANGUAGE.into(), "", false, None)
                .map_err(|(_, error)| error)
                .unwrap();
        // SAFETY: `Language` is one pointer to a `TSLanguage`, of which only
        // the version is read, and that is below the minimum. It is
        // forgotten below, so nothing frees the static.
        let fake: tree_sitter::Language = unsafe {
            std::mem::transmute::<*const tree_sitter::ffi::TSLanguage, tree_sitter::Language>(
                FAKE.0.as_ptr().cast(),
            )
        };
        let real = std::mem::replace(&mut language.grammar, fake);
        assert!(ThreadParser::for_language(&language).is_err());
        assert!(parse_str(&language, "fn main() {}", || false).is_none());
        std::mem::forget(std::mem::replace(&mut language.grammar, real));
    }
}
