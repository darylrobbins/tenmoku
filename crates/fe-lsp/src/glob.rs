//! LSP glob patterns (`FileSystemWatcher.globPattern`,
//! `FileOperationPattern.glob`), matched as VS Code matches them: `*`
//! within a path segment, `**` across segments, `?`, `{a,b}`, `[a-z]` and
//! `[!a-z]`. Case-sensitive, as VS Code's glob matching is: on a
//! case-insensitive volume a path spelled differently from the pattern
//! doesn't match.
//!
//! A limit of globset, which matches bytes: `?` and `[...]` stand for one
//! byte, so `?` doesn't match a character of two or more bytes (`é`).
//! A backslash is an ordinary character, as in VS Code's matching.

use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobMatcher};
use lsp_types::{GlobPattern, OneOf};

/// A compiled LSP glob.
pub(crate) struct LspGlob {
    matcher: GlobMatcher,
    /// A relative pattern's base: paths are matched under it.
    base: Option<PathBuf>,
}

impl LspGlob {
    /// A string pattern is matched against the whole absolute path; a
    /// relative pattern against the path under its `baseUri`'s path.
    pub(crate) fn new(pattern: &GlobPattern) -> Result<LspGlob, String> {
        let (glob, base) = match pattern {
            GlobPattern::String(glob) => (glob.as_str(), None),
            GlobPattern::Relative(relative) => {
                let base_uri = match &relative.base_uri {
                    OneOf::Left(folder) => &folder.uri,
                    OneOf::Right(uri) => uri,
                };
                let base = base_uri
                    .to_file_path()
                    .map_err(|()| format!("base {base_uri} is not a file URI"))?;
                (relative.pattern.as_str(), Some(base))
            }
        };
        let matcher = GlobBuilder::new(glob)
            .literal_separator(true)
            .backslash_escape(false)
            .empty_alternates(true)
            .build()
            .map_err(|err| format!("invalid glob {glob:?}: {err}"))?
            .compile_matcher();
        Ok(LspGlob { matcher, base })
    }

    pub(crate) fn matches(&self, path: &Path) -> bool {
        match &self.base {
            None => self.is_match(path),
            Some(base) => path
                .strip_prefix(base)
                .is_ok_and(|rest| self.is_match(rest)),
        }
    }

    /// Globset wants a `/` before a trailing `/**`, where VS Code's
    /// `a/**` also matches `a` itself: so the path is tried as a
    /// directory too.
    fn is_match(&self, path: &Path) -> bool {
        self.matcher.is_match(path) || {
            let mut directory = path.as_os_str().to_owned();
            directory.push("/");
            self.matcher.is_match(Path::new(&directory))
        }
    }
}
