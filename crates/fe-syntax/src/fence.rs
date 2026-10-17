use crate::{
    Language,
    language::{Injected, injected_language, language_by_name},
};

/// The language a Markdown fence's info string names, as fences in a
/// Markdown buffer are highlighted: by name, file extension or alias (`py3`,
/// `shell`, `c#`), ignoring case. Only the first word counts, and anything
/// after a comma or colon in it, as in ```` ```rust,ignore ```` or
/// ```` ```js:app.js ````. Compiles the language's query on first use.
pub fn fence_language(info: &str) -> Option<&'static Language> {
    let name = info.split_whitespace().next()?;
    match injected_language(name) {
        Injected::Ready(language) => Some(language),
        Injected::Missing(name) => language_by_name(name),
        Injected::Unknown => None,
    }
}
