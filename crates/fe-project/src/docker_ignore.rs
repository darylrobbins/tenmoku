//! Docker's `.dockerignore` matching (moby/patternmatcher, as `docker build`
//! reads the file), kept apart from git's engine because the rules differ:
//!
//! - Every pattern is anchored at the build context root: `*.log` matches only
//!   top-level logs, `**/*.log` any depth.
//! - A pattern is cleaned like Go's `filepath.Clean` first: a leading `/` and
//!   `./` go, a trailing `/` goes, `a//b` collapses.
//! - Matching is Go's `filepath.Match` per segment (`*`, `?`, `[a-z]`, `[^a-z]`,
//!   `\c`) plus `**`, which matches any run of directories, none included.
//! - A path is excluded if it or a parent directory matches; the last matching
//!   line decides, and a `!` line re-includes even inside an excluded directory.

use std::{
    collections::HashMap,
    ffi::OsStr,
    ops::Range,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use ignore::WalkBuilder;

use crate::{
    ignore_check::{
        IgnoreFinding, IgnoreFindingKind, IgnoreReport, IgnoreSeverity, MAX_LISTED_PATHS,
        MatchedPaths,
    },
    listing::is_git_dir,
};

/// Go's `filepath.Clean` on a slash path, then the leading `/` removed, as
/// Docker prepares a pattern.
pub(crate) fn clean(pattern: &str) -> String {
    let rooted = pattern.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in pattern.split('/') {
        match part {
            "" | "." => {}
            ".." => match parts.last() {
                Some(&last) if last != ".." => {
                    parts.pop();
                }
                _ if rooted => {}
                _ => parts.push(".."),
            },
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    match (joined.is_empty(), rooted) {
        (true, true) => "/".to_string(),
        (true, false) => ".".to_string(),
        (false, _) => joined,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Literal(char),
    /// `?`
    One,
    /// `*`
    Star,
    /// `**` at the end of the pattern: `.*`.
    Deep,
    /// `**/` before more pattern: `(.*/)?`.
    DeepDir,
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

/// One line of a `.dockerignore`, compiled.
#[derive(Clone, Debug)]
pub(crate) struct Pattern {
    pub(crate) negated: bool,
    /// The line as Docker sees it: its `!` kept, the rest cleaned. Equal for
    /// lines that are the same pattern.
    pub(crate) key: String,
    tokens: Vec<Token>,
    /// moby's shortcut for `**` followed by plain text (not starting with
    /// `/`): the path ends with it, whatever is before.
    suffix: Option<String>,
}

impl Pattern {
    /// Compiles `line`, already trimmed of whitespace.
    pub(crate) fn new(line: &str) -> Result<Pattern, String> {
        let (negated, body) = match line.strip_prefix('!') {
            Some(rest) => (true, rest.trim()),
            None => (false, line),
        };
        if negated && body.is_empty() {
            return Err("illegal exclusion pattern".to_string());
        }
        let cleaned = clean(body);
        let key = if negated {
            format!("!{cleaned}")
        } else {
            cleaned.clone()
        };
        let tokens = compile(&cleaned).map_err(|()| "syntax error in pattern".to_string())?;
        let suffix = cleaned
            .strip_prefix("**")
            .filter(|rest| {
                !rest.is_empty() && !rest.starts_with('/') && !rest.contains(['*', '?', '[', '\\'])
            })
            .map(str::to_owned);
        Ok(Pattern {
            negated,
            key,
            tokens,
            suffix,
        })
    }

    /// Whether the pattern matches `path` itself, a slash path relative to
    /// the context.
    pub(crate) fn matches(&self, path: &str) -> bool {
        if let Some(suffix) = &self.suffix {
            return path.ends_with(suffix.as_str());
        }
        let path: Vec<char> = path.chars().collect();
        let (n, m) = (self.tokens.len(), path.len());
        let mut dp = vec![vec![false; m + 1]; n + 1];
        dp[n][m] = true;
        for i in (0..n).rev() {
            for j in (0..=m).rev() {
                let here = path.get(j).copied();
                dp[i][j] = match &self.tokens[i] {
                    Token::Literal(c) => here == Some(*c) && dp[i + 1][j + 1],
                    Token::One => here.is_some_and(|c| c != '/') && dp[i + 1][j + 1],
                    Token::Class { negated, ranges } => {
                        here.is_some_and(|c| {
                            c != '/'
                                && ranges.iter().any(|&(lo, hi)| lo <= c && c <= hi) != *negated
                        }) && dp[i + 1][j + 1]
                    }
                    Token::Star => dp[i + 1][j] || (here.is_some_and(|c| c != '/') && dp[i][j + 1]),
                    Token::Deep => dp[i + 1][j] || (here.is_some() && dp[i][j + 1]),
                    Token::DeepDir => {
                        dp[i + 1][j] || (j + 1..=m).any(|k| path[k - 1] == '/' && dp[i + 1][k])
                    }
                };
            }
        }
        dp[0][0]
    }
}

/// `pattern` (without a leading `!`) against `path`, cleaning the pattern
/// first. A malformed pattern matches nothing.
#[cfg(test)]
pub(crate) fn matches(pattern: &str, path: &str) -> bool {
    Pattern::new(pattern).is_ok_and(|pattern| pattern.matches(path))
}

/// Go's `filepath.Match` syntax with moby's `**`; `Err` where Go reports
/// `ErrBadPattern`.
fn compile(pattern: &str) -> Result<Vec<Token>, ()> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                // As moby translates it: `**` takes a following `/`, and is
                // any run of characters at the end, else an optional run of
                // directories.
                i += 1;
                if chars.get(i + 1) == Some(&'/') {
                    i += 1;
                }
                tokens.push(if i + 1 == chars.len() {
                    Token::Deep
                } else {
                    Token::DeepDir
                });
            }
            '*' => tokens.push(Token::Star),
            '?' => tokens.push(Token::One),
            '\\' => {
                i += 1;
                tokens.push(Token::Literal(*chars.get(i).ok_or(())?));
            }
            '[' => {
                i += 1;
                let negated = chars.get(i) == Some(&'^');
                if negated {
                    i += 1;
                }
                let mut ranges = Vec::new();
                loop {
                    if chars.get(i) == Some(&']') && !ranges.is_empty() {
                        break;
                    }
                    let lo = class_char(&chars, &mut i)?;
                    let mut hi = lo;
                    if chars.get(i) == Some(&'-') {
                        i += 1;
                        hi = class_char(&chars, &mut i)?;
                    }
                    ranges.push((lo, hi));
                }
                tokens.push(Token::Class { negated, ranges });
            }
            c => tokens.push(Token::Literal(c)),
        }
        i += 1;
    }
    Ok(tokens)
}

/// Go's `getEsc`: one class member, which can't be `-` or `]` bare.
fn class_char(chars: &[char], i: &mut usize) -> Result<char, ()> {
    let mut c = *chars.get(*i).ok_or(())?;
    if c == '-' || c == ']' {
        return Err(());
    }
    if c == '\\' {
        *i += 1;
        c = *chars.get(*i).ok_or(())?;
    }
    *i += 1;
    Ok(c)
}

/// Whether the walk of `paths` in order leaves `path` excluded: the last line
/// matching it or a parent directory decides, and `!` includes.
#[cfg(test)]
pub(crate) fn excluded(patterns: &[Pattern], path: &str) -> bool {
    let mut excluded = false;
    for pattern in patterns {
        let covered = std::iter::successors(Some(path), |p| p.rsplit_once('/').map(|(d, _)| d))
            .any(|p| pattern.matches(p));
        if covered {
            excluded = !pattern.negated;
        }
    }
    excluded
}

/// Whether `path` names a file Docker or Podman read as an ignore file.
pub(crate) fn is_container_ignore(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .is_some_and(|name| name.ends_with(".dockerignore") || name.ends_with(".containerignore"))
}

struct Line {
    line: usize,
    range: Range<usize>,
    pattern: Result<Pattern, String>,
    key: String,
    matched: MatchedPaths,
    effective: bool,
    overridden_by: Option<usize>,
}

impl Line {
    fn negated(&self) -> bool {
        self.pattern.as_ref().is_ok_and(|p| p.negated)
    }

    fn matches(&self, path: &str) -> bool {
        self.pattern.as_ref().is_ok_and(|p| p.matches(path))
    }
}

/// The lines of `text` that hold a pattern, as Docker reads them: a byte
/// order mark skipped, whitespace trimmed, comments and blanks dropped.
fn lines(text: &str) -> (usize, Vec<Line>) {
    let mut out = Vec::new();
    let mut count = 0;
    let mut start = 0;
    for raw in text.split_inclusive('\n') {
        count += 1;
        let bom = if start == 0 && raw.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };
        let body = &raw[bom..];
        let body = body.strip_suffix('\n').unwrap_or(body);
        let trimmed = body.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            let from = start + bom + (body.len() - body.trim_start().len());
            let pattern = Pattern::new(trimmed);
            let key = pattern
                .as_ref()
                .map_or_else(|_| trimmed.to_string(), |p| p.key.clone());
            out.push(Line {
                line: count,
                range: from..from + trimmed.len(),
                pattern,
                key,
                matched: MatchedPaths::default(),
                effective: false,
                overridden_by: None,
            });
        }
        start += raw.len();
    }
    (count, out)
}

/// Checks the container ignore file `path` (holding `text`) against the build
/// context, taken as the directory holding the file. See
/// [`crate::check_ignore_file`].
pub(crate) fn check(
    path: &Path,
    text: &str,
    cancel: &AtomicBool,
    max_entries: usize,
) -> IgnoreReport {
    let base = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    let (line_count, mut rules) = lines(text);
    let end = if cancel.load(Ordering::Relaxed) {
        ScanEnd::Cancelled
    } else {
        scan(base, &mut rules, cancel, max_entries)
    };
    let walked_all = end == ScanEnd::Complete;

    let mut first_seen: HashMap<String, usize> = HashMap::new();
    let mut findings = Vec::new();
    let mut matches = vec![MatchedPaths::default(); line_count];
    for rule in rules {
        let duplicate_of = match first_seen.get(&rule.key) {
            Some(&first) => Some(first),
            None => {
                first_seen.insert(rule.key.clone(), rule.line);
                None
            }
        };
        let found = if let Err(message) = &rule.pattern {
            Some((
                IgnoreFindingKind::Invalid(message.clone()),
                IgnoreSeverity::Error,
            ))
        } else if let Some(first_line) = duplicate_of {
            Some((
                IgnoreFindingKind::Duplicate { first_line },
                IgnoreSeverity::Warning,
            ))
        } else if !walked_all {
            None
        } else if rule.matched.total == 0 {
            Some((IgnoreFindingKind::Unused, IgnoreSeverity::Hint))
        } else {
            match rule.overridden_by {
                Some(by_line) if !rule.effective => Some((
                    IgnoreFindingKind::Shadowed { by_line },
                    IgnoreSeverity::Warning,
                )),
                _ => None,
            }
        };
        if let Some((kind, severity)) = found {
            findings.push(IgnoreFinding {
                line: rule.line,
                range: rule.range.clone(),
                kind,
                severity,
            });
        }
        matches[rule.line - 1] = rule.matched;
    }
    IgnoreReport {
        findings,
        matches,
        truncated: end == ScanEnd::Truncated,
        cancelled: end == ScanEnd::Cancelled,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanEnd {
    Complete,
    Truncated,
    Cancelled,
}

/// Walks the context below `base`, recording in each line what it matched
/// directly and whether it decided a path or a later line did the other way.
fn scan(base: &Path, rules: &mut [Line], cancel: &AtomicBool, max_entries: usize) -> ScanEnd {
    let mut builder = WalkBuilder::new(base);
    builder
        .standard_filters(false)
        .sort_by_file_name(OsStr::cmp)
        .filter_entry(|entry| entry.depth() == 0 || !is_git_dir(entry.file_name()));

    let mut end = ScanEnd::Complete;
    let mut entries = 0;
    // For each directory on the way down, the lines matching it or a parent.
    let mut covers: Vec<Vec<usize>> = Vec::new();
    for entry in builder.build() {
        if cancel.load(Ordering::Relaxed) {
            return ScanEnd::Cancelled;
        }
        let Ok(entry) = entry else {
            end = ScanEnd::Truncated;
            continue;
        };
        let depth = entry.depth();
        if depth == 0 {
            continue;
        }
        entries += 1;
        if entries > max_entries {
            return ScanEnd::Truncated;
        }
        let path = entry.path();
        let relative = path.strip_prefix(base).unwrap_or(path);
        let slash_path = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());

        covers.truncate(depth - 1);
        let direct: Vec<usize> = rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.matches(&slash_path))
            .map(|(ix, _)| ix)
            .collect();
        let inherited = covers.last().map(Vec::as_slice).unwrap_or_default();
        let decider = direct.last().copied().max(inherited.last().copied());
        if is_dir {
            let mut cover = inherited.to_vec();
            cover.extend(&direct);
            cover.sort_unstable();
            cover.dedup();
            covers.push(cover);
        }
        let Some(decider) = decider else { continue };
        let (decider_line, decider_negated) = (rules[decider].line, rules[decider].negated());
        for hit in direct {
            let rule = &mut rules[hit];
            rule.matched.total += 1;
            if rule.matched.paths.len() < MAX_LISTED_PATHS {
                rule.matched.paths.push(PathBuf::from(relative));
            }
            if hit != decider && rule.negated() != decider_negated {
                rule.overridden_by = rule.overridden_by.max(Some(decider_line));
            } else {
                rule.effective = true;
            }
        }
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_follows_go() {
        assert_eq!(clean("/dist"), "dist");
        assert_eq!(clean("/"), "/");
        assert_eq!(clean("./dist"), "dist");
        assert_eq!(clean("dist/"), "dist");
        assert_eq!(clean("a//b/../c"), "a/c");
        assert_eq!(clean(""), ".");
    }

    #[test]
    fn matching_and_walk_order() {
        assert!(matches("*.log", "top.log"));
        assert!(!matches("*.log", "a/top.log"));
        assert!(matches("**/*.log", "a/b/top.log"));
        assert!(matches("**/*.log", "top.log"));
        assert!(matches("[^a-z]x", "1x"));
        assert!(!matches("[^a-z]x", "ax"));
        assert!(!matches("[abc", "a"));
        assert!(matches("a/**", "a/b/c"));
        assert!(matches("**.log", "top.log"));
        assert!(matches("**.log", "a/top.log"));
        assert!(matches("**foo", "x/y/barfoo"));
        assert!(!matches("**.log", "top.txt"));
        assert!(matches("a**b", "ab"));
        assert!(matches("a**b", "ax/b"));
        assert!(!matches("a**b", "axb"));
        assert!(!matches("**.l?g", "top.log"));
        assert!(matches("**.l?g", "x/.l-g"));
        let patterns: Vec<_> = ["docs", "!docs/README.md"]
            .iter()
            .map(|p| Pattern::new(p).unwrap())
            .collect();
        assert!(excluded(&patterns, "docs/a.md"));
        assert!(!excluded(&patterns, "docs/README.md"));
    }
}
