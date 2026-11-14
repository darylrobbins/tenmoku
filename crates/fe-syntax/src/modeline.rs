//! Modelines: a file naming its own language in a comment, as Vim's
//! `vim: ft=xml`, Emacs's `-*- mode: xml -*-` and our `tenmoku: language=xml`
//! lines do.

use crate::language::{Injected, injected_language, language_names};

/// How many lines at each end of a file are read for a modeline: Vim's
/// default `modelines`.
const MODELINE_LINES: usize = 5;

/// Vim filetype and Emacs mode names, lowercase, and the language each is
/// ours. A name not here is looked up as a language name, file extension or
/// fence alias.
const NAMES: &[(&str, &str)] = &[
    ("sh", "bash"),
    ("bash", "bash"),
    ("zsh", "bash"),
    ("ksh", "bash"),
    ("shell-script", "bash"),
    ("sh-mode", "bash"),
    ("c++", "cpp"),
    ("cpp", "cpp"),
    ("cs", "csharp"),
    ("csharp", "csharp"),
    ("js", "javascript"),
    ("javascript", "javascript"),
    ("javascriptreact", "javascript"),
    ("jsx", "javascript"),
    ("ts", "typescript"),
    ("typescript", "typescript"),
    ("typescriptreact", "tsx"),
    ("py", "python"),
    ("python", "python"),
    ("rb", "ruby"),
    ("ruby", "ruby"),
    ("md", "markdown"),
    ("markdown", "markdown"),
    ("yml", "yaml"),
    ("yaml", "yaml"),
    ("make", "make"),
    ("makefile", "make"),
    ("dockerfile", "dockerfile"),
    ("nxml", "xml"),
    ("xml", "xml"),
    ("xsd", "xml"),
    ("xslt", "xml"),
    ("svg", "xml"),
    ("json", "json"),
    ("jsonc", "json"),
    ("toml", "toml"),
    ("ini", "ini"),
    ("dosini", "ini"),
    ("conf", "ini"),
    ("gitcommit", "gitcommit"),
    ("diff", "diff"),
];

/// The language a modeline in the first or last lines of a file names, if it
/// is compiled in or [registered](crate::register_language). `head` is the
/// start of the file and `tail` its end, starting at a line start; they may
/// overlap.
///
/// An Emacs `-*- mode: xml -*-` (or `-*- xml -*-`) line, first or after a
/// `#!` line, wins; then a Vim modeline (`vim: set ft=xml:`, `vi: ft=xml`,
/// `ex:`, `vim700:`...) in head's first five lines or tail's last five, the
/// first such line that names a language, of which the last `ft`,
/// `filetype`, `syntax` or `syn` counts; then a `tenmoku: language=xml` line
/// in the same lines, for a language neither editor has a name for.
pub fn language_name_for_modeline(head: &str, tail: &str) -> Option<&'static str> {
    let first: Vec<&str> = head.lines().take(MODELINE_LINES).collect();
    let last: Vec<&str> = {
        let mut lines: Vec<&str> = tail.lines().rev().take(MODELINE_LINES).collect();
        lines.reverse();
        lines
    };
    let lines = || first.iter().chain(&last).copied();
    let emacs_line = match first.first() {
        Some(line) if line.starts_with("#!") => first.get(1),
        line => line,
    };
    emacs_line
        .and_then(|line| emacs_mode(line))
        .and_then(resolve)
        .or_else(|| lines().find_map(|line| vim_filetype(line).and_then(resolve)))
        .or_else(|| lines().find_map(|line| tenmoku_language(line).and_then(resolve)))
}

/// Our language for a modeline's name for one, if it's compiled in or
/// registered: through [`NAMES`], else as an injection's name is.
fn resolve(name: &str) -> Option<&'static str> {
    // One word, as the editors read it: an injection's name is trimmed.
    if name.contains(char::is_whitespace) {
        return None;
    }
    let name = name.to_ascii_lowercase();
    let name = NAMES
        .iter()
        .find(|(modeline, _)| *modeline == name)
        .map_or(name.as_str(), |(_, ours)| ours);
    let found = match injected_language(name) {
        Injected::Ready(language) => language.name(),
        Injected::Missing(name) => name,
        Injected::Unknown => return None,
    };
    // Only reached through injections, as Markdown's inline grammar is.
    language_names().find(|&listed| listed == found)
}

/// Where a modeline marker can start in `line`: at its start, or after
/// whitespace.
fn word_starts(line: &str) -> impl Iterator<Item = usize> + '_ {
    line.char_indices()
        .filter(|&(at, _)| at == 0 || line[..at].ends_with(char::is_whitespace))
        .map(|(at, _)| at)
}

/// The filetype a Vim modeline on `line` sets: the value of its last `ft`,
/// `filetype`, `syntax` or `syn` option. Dotted filetypes (`cpp.doxygen`)
/// give their first.
fn vim_filetype(line: &str) -> Option<&str> {
    let options = word_starts(line).find_map(|at| vim_options(&line[at..], at == 0))?;
    // `se[t] ... :` runs to the first `:` that isn't escaped, its options
    // split by whitespace; otherwise colons separate them too.
    let set_form = options
        .strip_prefix("set ")
        .or_else(|| options.strip_prefix("se "));
    let options = match set_form {
        Some(rest) => &rest[..unescaped_colon(rest)?],
        None => options,
    };
    let separators: &[char] = if set_form.is_some() { &[] } else { &[':'] };
    options
        .split(|c: char| c.is_whitespace() || separators.contains(&c))
        .filter_map(|option| option.split_once('='))
        .filter(|(key, _)| matches!(*key, "ft" | "filetype" | "syntax" | "syn"))
        .map(|(_, value)| value.split('.').next().unwrap_or(value))
        .rfind(|value| !value.is_empty())
}

/// The options after a Vim modeline marker at the start of `text`, with
/// leading whitespace gone: `vi:`, `vim:`, `Vim:` (whose options must use
/// `set`), `vim700:`, `vim>600:`, or, away from the line's start, `ex:`.
fn vim_options(text: &str, line_start: bool) -> Option<&str> {
    let rest = if let Some(rest) = text.strip_prefix("vi:") {
        rest
    } else if let Some(rest) = text.strip_prefix("ex:").filter(|_| !line_start) {
        rest
    } else {
        let upper = text.starts_with('V');
        let rest = text
            .strip_prefix("vim")
            .or_else(|| text.strip_prefix("Vim"))?;
        let rest = rest.strip_prefix(['<', '=', '>']).unwrap_or(rest);
        let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        let rest = rest.strip_prefix(':')?;
        if upper && !rest.trim_start().starts_with("set") {
            return None;
        }
        rest
    };
    Some(rest.trim_start())
}

/// The byte offset of the first `:` in `text` not after a backslash.
fn unescaped_colon(text: &str) -> Option<usize> {
    text.match_indices(':')
        .map(|(at, _)| at)
        .find(|&at| !text[..at].ends_with('\\'))
}

/// `text` without the spaces and tabs around it, which is all Emacs skips.
fn trim(text: &str) -> &str {
    text.trim_matches([' ', '\t'])
}

/// The major mode an Emacs `-*- ... -*-` line names: its `mode:` value, its
/// key in any case, or the lone word between the markers.
fn emacs_mode(line: &str) -> Option<&str> {
    let (_, rest) = line.split_once("-*-")?;
    let (body, _) = rest.split_once("-*-")?;
    let body = trim(body);
    if !body.contains(':') {
        return (!body.is_empty() && !body.contains(char::is_whitespace)).then_some(body);
    }
    body.split(';').find_map(|pair| {
        let (key, value) = pair.split_once(':')?;
        let value = trim(value);
        (trim(key).eq_ignore_ascii_case("mode") && !value.is_empty()).then_some(value)
    })
}

/// The language a `tenmoku: language=xml` line names (`filetype=` and `ft=`
/// too), after any comment leader: the last such option on the line.
fn tenmoku_language(line: &str) -> Option<&str> {
    const MARKER: &str = "tenmoku:";
    let at = line.char_indices().map(|(at, _)| at).find(|&at| {
        line.as_bytes()
            .get(at..at + MARKER.len())
            .is_some_and(|word| word.eq_ignore_ascii_case(MARKER.as_bytes()))
            && !line[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_')
    })?;
    line[at + MARKER.len()..]
        .split(|c: char| c.is_whitespace() || matches!(c, ':' | ';' | ','))
        .filter_map(|option| option.split_once('='))
        .filter(|(key, _)| matches!(*key, "language" | "filetype" | "ft"))
        .map(|(_, value)| value)
        .rfind(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vim_markers() {
        let cases = [
            ("vim: ft=xml", Some("ft=xml")),
            ("# vi:ft=xml", Some("ft=xml")),
            ("ex: ft=xml", None),
            ("# ex: ft=xml", Some("ft=xml")),
            ("#vim: ft=xml", None),
            ("Vim: ft=xml", None),
            ("Vim: set ft=xml:", Some("set ft=xml:")),
            ("vim700: ft=xml", Some("ft=xml")),
            ("vim>600: ft=xml", Some("ft=xml")),
            ("vimx: ft=xml", None),
        ];
        for (line, want) in cases {
            let got = word_starts(line).find_map(|at| vim_options(&line[at..], at == 0));
            assert_eq!(got, want, "{line:?}");
        }
    }

    #[test]
    fn set_form_needs_its_closing_colon() {
        assert_eq!(vim_filetype("# vim: set ft=xml"), None);
        assert_eq!(vim_filetype("# vim: set ft=xml ts=2: ft=rust"), Some("xml"));
        assert_eq!(vim_filetype("# vim: se ft=a\\:b:"), Some("a\\:b"));
    }

    #[test]
    fn tenmoku_marker_follows_a_comment_leader() {
        assert_eq!(tenmoku_language("//tenmoku: language=xml"), Some("xml"));
        assert_eq!(tenmoku_language("<!-- Tenmoku: ft=xml -->"), Some("xml"));
        assert_eq!(tenmoku_language("nottenmoku: language=xml"), None);
        assert_eq!(tenmoku_language("tenmoku: colour=red"), None);
    }
}
