//! Checks an ignore file's patterns against the project below it, as git
//! would match them: lines that match nothing, repeat an earlier line, are
//! overridden by a later line, don't parse, or try to re-include a path below
//! an excluded directory.
//!
//! The walk has ignore rules off, because a pattern like `node_modules/` hides
//! the very paths that show it is in use. It starts at the ignore file's
//! directory, skips `.git`, and doesn't follow symlinks. Each pattern is
//! matched alone against each path it reaches directly, without counting the
//! contents of a directory it matches. Only this file's lines are considered:
//! rules from other ignore files, and git's stopping at a nested repository,
//! aren't modelled.
//!
//! Patterns go through `ignore`'s [`GitignoreBuilder`], rewritten first where
//! its glob syntax departs from git's: braces are literal in git, and bracket
//! expressions take backslash escapes and POSIX classes. Patterns compare by
//! case, as git's do unless `core.ignoreCase` is set.

use std::{
    collections::{HashMap, hash_map::Entry},
    ffi::OsStr,
    ops::Range,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use ignore::{
    Error, WalkBuilder,
    gitignore::{Gitignore, GitignoreBuilder},
};

use crate::listing::is_git_dir;

/// The most paths [`check_ignore_file`] walks before giving up on the checks
/// that need the whole project.
pub const MAX_SCAN_ENTRIES: usize = 200_000;

/// The most paths [`MatchedPaths`] lists for a line.
const MAX_LISTED_PATHS: usize = 50;

/// What [`check_ignore_file`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IgnoreReport {
    /// At most one finding per line, in line order.
    pub findings: Vec<IgnoreFinding>,
    /// What each line of the text matched, `matches[line - 1]`. Comments,
    /// blank lines and invalid patterns match nothing. Partial if the walk
    /// didn't finish.
    pub matches: Vec<MatchedPaths>,
    /// The walk stopped at the entry cap or couldn't read part of the project,
    /// so no line was reported as unused, shadowed, or as failing to re-include
    /// a path it matched.
    pub truncated: bool,
    /// As [`IgnoreReport::truncated`], because the cancel flag was set.
    pub cancelled: bool,
}

/// One problem with one line of an ignore file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IgnoreFinding {
    /// The 1-based line.
    pub line: usize,
    /// The pattern's bytes in the text, without its line ending, its trailing
    /// spaces or a byte order mark.
    pub range: Range<usize>,
    pub kind: IgnoreFindingKind,
    pub severity: IgnoreSeverity,
}

/// Why a line was reported. A line that fits several is reported as the first
/// of: [`Invalid`](Self::Invalid), [`Duplicate`](Self::Duplicate),
/// [`Unused`](Self::Unused), [`Shadowed`](Self::Shadowed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IgnoreFindingKind {
    /// The pattern matches no path.
    Unused,
    /// The same pattern is on `first_line` (1-based), earlier in the file.
    Duplicate { first_line: usize },
    /// Every path the pattern matches is decided the other way by a later
    /// line: `by_line` (1-based), the last of them.
    Shadowed { by_line: usize },
    /// The pattern doesn't parse, or it is a negation that can't take effect
    /// because a parent directory of what it names is excluded.
    Invalid(String),
}

/// How much a finding matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IgnoreSeverity {
    Hint,
    Warning,
    Error,
}

/// The paths one line matched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MatchedPaths {
    /// The first paths in walk order, at most 50, relative to the directory
    /// the patterns are based at.
    pub paths: Vec<PathBuf>,
    /// How many paths matched, listed or not.
    pub total: usize,
}

/// Checks `text`, the contents (perhaps unsaved) of the ignore file at `path`,
/// against the project. Walks the disk, so run it off the UI thread. Setting
/// `cancel` ends the walk at its next entry; the checks that need no walk are
/// still reported.
///
/// A `.dockerignore` returns an empty report: Docker bases its patterns at the
/// build context, matches with Go's `filepath.Match`, and lets a `!` re-include
/// inside an excluded directory, none of which git's rules model.
pub fn check_ignore_file(path: &Path, text: &str, cancel: &AtomicBool) -> IgnoreReport {
    check_ignore_file_capped(path, text, cancel, MAX_SCAN_ENTRIES)
}

/// [`check_ignore_file`] with its own limit on the paths walked.
pub fn check_ignore_file_capped(
    path: &Path,
    text: &str,
    cancel: &AtomicBool,
    max_entries: usize,
) -> IgnoreReport {
    if has_docker_syntax(path) {
        return IgnoreReport::default();
    }
    let base = base_dir(path);
    let (line_count, lines) = pattern_lines(text);
    let mut rules: Vec<Rule> = lines
        .into_iter()
        .map(|line| Rule::new(line, base))
        .collect();
    let parents_blocked: Vec<Option<Blocked>> = rules
        .iter()
        .map(|rule| {
            if cancel.load(Ordering::Relaxed) {
                None
            } else {
                blocked_parent(rule, &rules, base)
            }
        })
        .collect();
    let end = scan(base, &mut rules, cancel, max_entries);
    let walked_all = end == ScanEnd::Complete;

    let mut first_seen = HashMap::new();
    let mut findings = Vec::new();
    let mut matches = vec![MatchedPaths::default(); line_count];
    for (rule, parent_blocked) in rules.into_iter().zip(parents_blocked) {
        let duplicate_of = match first_seen.entry(rule.pattern) {
            Entry::Occupied(first) => Some(*first.get()),
            Entry::Vacant(slot) => {
                slot.insert(rule.line);
                None
            }
        };
        if let Some((kind, severity)) = rule.finding(parent_blocked, duplicate_of, walked_all) {
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

/// Whether `path` is one of the files Docker reads (`.dockerignore`, or
/// `<name>.dockerignore` beside a Dockerfile), or Podman's alike.
fn has_docker_syntax(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.ends_with(".dockerignore") || name.ends_with(".containerignore"))
}

/// The directory `path`'s patterns are based at: its own, but for a
/// repository's `info/exclude`, whose patterns are based at the worktree.
fn base_dir(path: &Path) -> &Path {
    let ancestor = if path.ends_with(".git/info/exclude") {
        3
    } else {
        1
    };
    match path.ancestors().nth(ancestor) {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    }
}

struct PatternLine<'a> {
    /// 1-based.
    line: usize,
    range: Range<usize>,
    pattern: &'a str,
}

/// How many lines `text` has, and those that hold a pattern, as git reads them:
/// a byte order mark is skipped, one `\r` before the `\n` is dropped, and a line
/// starting with `#` is a comment.
fn pattern_lines(text: &str) -> (usize, Vec<PatternLine<'_>>) {
    let mut patterns = Vec::new();
    let mut line_count = 0;
    let mut start = 0;
    for raw in text.split_inclusive('\n') {
        line_count += 1;
        let bom = if start == 0 && raw.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };
        let body = &raw[bom..];
        let body = body.strip_suffix('\n').unwrap_or(body);
        let body = body.strip_suffix('\r').unwrap_or(body);
        let pattern = trim_trailing_spaces(body);
        if !pattern.starts_with('#') && !pattern.trim_end().is_empty() {
            let from = start + bom;
            patterns.push(PatternLine {
                line: line_count,
                range: from..from + pattern.len(),
                pattern,
            });
        }
        start += raw.len();
    }
    (line_count, patterns)
}

/// `line` without its trailing spaces, keeping a backslash-escaped one, as git
/// trims a line. (`GitignoreBuilder::add_line` would make `a\  ` a dangling
/// escape.)
fn trim_trailing_spaces(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut end = None;
    let mut ix = 0;
    while ix < bytes.len() {
        match bytes[ix] {
            b' ' => {
                end.get_or_insert(ix);
            }
            b'\\' => {
                ix += 1;
                end = None;
            }
            _ => end = None,
        }
        ix += 1;
    }
    &line[..end.unwrap_or(line.len())]
}

/// A parent directory that keeps a negation from taking effect.
struct Blocked {
    dir: PathBuf,
    /// The line that excludes it.
    by_line: usize,
}

/// One pattern line, built alone, and what the walk found for it.
struct Rule<'a> {
    line: usize,
    range: Range<usize>,
    pattern: &'a str,
    /// The matcher, or why the pattern doesn't parse.
    matcher: Result<Gitignore, String>,
    negated: bool,
    matched: MatchedPaths,
    /// Decides some path, or leaves it to a later line with the same effect.
    effective: bool,
    /// The last later line of opposite effect that decided one of its paths.
    overridden_by: Option<usize>,
    /// A negation that matched a path with no excluded parent.
    reincludes: bool,
    /// A negation that matched a path with an excluded parent.
    path_blocked: Option<Blocked>,
}

impl<'a> Rule<'a> {
    fn new(line: PatternLine<'a>, base: &Path) -> Self {
        Rule {
            line: line.line,
            range: line.range,
            pattern: line.pattern,
            matcher: build_matcher(line.pattern, base),
            negated: line.pattern.starts_with('!'),
            matched: MatchedPaths::default(),
            effective: false,
            overridden_by: None,
            reincludes: false,
            path_blocked: None,
        }
    }

    /// Whether the rule's pattern matches `path` itself.
    fn matches(&self, path: &Path, is_dir: bool) -> bool {
        self.matcher
            .as_ref()
            .is_ok_and(|matcher| !matcher.matched(path, is_dir).is_none())
    }

    /// What to report for this line. `walked_all` says the walk saw the whole
    /// project: without that, a path it didn't reach could change any answer
    /// that depends on it.
    fn finding(
        &self,
        parent_blocked: Option<Blocked>,
        duplicate_of: Option<usize>,
        walked_all: bool,
    ) -> Option<(IgnoreFindingKind, IgnoreSeverity)> {
        if let Err(error) = &self.matcher {
            return Some((
                IgnoreFindingKind::Invalid(error.clone()),
                IgnoreSeverity::Error,
            ));
        }
        let path_blocked = self.path_blocked.as_ref().filter(|_| !self.reincludes);
        let blocked = parent_blocked
            .as_ref()
            .or(path_blocked.filter(|_| walked_all));
        if let Some(blocked) = blocked {
            let message = format!(
                "cannot re-include `{}`: its parent directory `{}` is excluded by line {}",
                self.pattern.strip_prefix('!').unwrap_or(self.pattern),
                blocked.dir.display(),
                blocked.by_line,
            );
            return Some((IgnoreFindingKind::Invalid(message), IgnoreSeverity::Warning));
        }
        if let Some(first_line) = duplicate_of {
            return Some((
                IgnoreFindingKind::Duplicate { first_line },
                IgnoreSeverity::Warning,
            ));
        }
        if !walked_all {
            return None;
        }
        if self.matched.total == 0 {
            return Some((IgnoreFindingKind::Unused, IgnoreSeverity::Hint));
        }
        match self.overridden_by {
            Some(by_line) if !self.effective => Some((
                IgnoreFindingKind::Shadowed { by_line },
                IgnoreSeverity::Warning,
            )),
            _ => None,
        }
    }
}

fn build_matcher(pattern: &str, base: &Path) -> Result<Gitignore, String> {
    let describe = |error: Error| match error {
        Error::Glob { err, .. } => err,
        other => other.to_string(),
    };
    // Nothing but `!` and slashes names no path to git, where the builder's
    // `!` would match every path.
    let name = pattern.strip_prefix('!').unwrap_or(pattern);
    if name.trim_matches('/').is_empty() {
        return Ok(Gitignore::empty());
    }
    let glob = glob_syntax(pattern)?;
    let mut builder = GitignoreBuilder::new(base);
    builder.allow_unclosed_class(false);
    builder.add_line(None, &glob).map_err(describe)?;
    builder.build().map_err(describe)
}

/// `pattern` in the syntax `ignore` parses, with the meaning it has in git:
/// braces are escaped, and bracket expressions are spelled out. An unclosed
/// bracket stays, for the builder to reject.
fn glob_syntax(pattern: &str) -> Result<String, String> {
    let mut glob = String::with_capacity(pattern.len());
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                glob.push('\\');
                glob.extend(chars.next());
            }
            '{' | '}' => {
                glob.push('\\');
                glob.push(c);
            }
            '[' => match bracket_expression(chars.as_str())? {
                Some((class, rest)) => {
                    glob.push_str(&class);
                    chars = rest.chars();
                }
                None => {
                    glob.push('[');
                    glob.push_str(chars.as_str());
                    break;
                }
            },
            c => glob.push(c),
        }
    }
    Ok(glob)
}

/// The bracket expression whose `[` comes just before `rest`, rewritten for
/// `ignore`, whose classes have no escapes (so `]` must come first and `-` last)
/// and no POSIX classes, and the text after it. `None` if it isn't closed.
fn bracket_expression(rest: &str) -> Result<Option<(String, &str)>, String> {
    let mut class = String::from("[");
    let mut chars = rest.chars();
    let mut after_negation = chars.clone();
    let negated = matches!(after_negation.next(), Some('!' | '^'));
    if negated {
        class.push_str(&rest[..1]);
        chars = after_negation;
    }
    let (mut body, mut close, mut dash) = (String::new(), false, false);
    let mut first = true;
    loop {
        let Some(c) = chars.next() else {
            return Ok(None);
        };
        match c {
            ']' if first => close = true,
            ']' => break,
            '\\' => match chars.next() {
                None => return Ok(None),
                Some(']') => close = true,
                Some('-') => dash = true,
                Some(escaped) => body.push(escaped),
            },
            '[' => {
                let named = chars
                    .as_str()
                    .strip_prefix(':')
                    .and_then(|after| Some(after.split_at(after.find(":]")?)));
                match named {
                    Some((name, after)) => {
                        let members = posix_class(name)
                            .ok_or_else(|| format!("unknown character class `[:{name}:]`"))?;
                        body.push_str(members);
                        chars = after[":]".len()..].chars();
                    }
                    None => body.push('['),
                }
            }
            c => body.push(c),
        }
        first = false;
    }
    // An escaped `!` or `^` mustn't land where it would negate the class.
    let negates_by_accident = !negated && !close && body.starts_with(['!', '^']);
    if negates_by_accident && !dash {
        if body.len() == 1 {
            return Ok(Some((format!("\\{body}"), chars.as_str())));
        }
        let member = body.remove(0);
        body.push(member);
    }
    if close {
        class.push(']');
    }
    if negates_by_accident && dash {
        class.push('-');
    }
    class.push_str(&body);
    if dash && !negates_by_accident {
        class.push('-');
    }
    class.push(']');
    Ok(Some((class, chars.as_str())))
}

/// A POSIX character class's members, for git's ASCII reading of it. None starts
/// with `!`, which would negate a class it opens.
fn posix_class(name: &str) -> Option<&'static str> {
    Some(match name {
        "alnum" => "0-9A-Za-z",
        "alpha" => "A-Za-z",
        "blank" => " \t",
        "cntrl" => "\x01-\x1f\x7f",
        "digit" => "0-9",
        "graph" => "\"-~!",
        "lower" => "a-z",
        "print" => " -~",
        "punct" => ":-@!-/[-`{-~",
        "space" => " \t\n\r\x0b\x0c",
        "upper" => "A-Z",
        "xdigit" => "0-9A-Fa-f",
        _ => return None,
    })
}

/// The directory named literally in negation `rule` that the file as a whole
/// excludes, if any. Git decides a directory before looking inside it, so
/// this holds whether or not the path exists, and whichever line comes first.
fn blocked_parent(rule: &Rule, rules: &[Rule], base: &Path) -> Option<Blocked> {
    let body = rule.pattern.strip_prefix('!')?;
    let body = body.strip_prefix('/').unwrap_or(body);
    let body = body.strip_suffix('/').unwrap_or(body);
    body.match_indices('/')
        .map(|(ix, _)| &body[..ix])
        .take_while(|parent| !parent.contains(['*', '?', '[', '\\']))
        .find_map(|parent| {
            let path = base.join(parent);
            let decider = rules
                .iter()
                .rev()
                .find(|other| other.matches(&path, true))?;
            (!decider.negated).then(|| Blocked {
                dir: PathBuf::from(parent),
                by_line: decider.line,
            })
        })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanEnd {
    Complete,
    Truncated,
    Cancelled,
}

/// Walks the project below `base`, recording in each rule what it matched.
fn scan(base: &Path, rules: &mut [Rule], cancel: &AtomicBool, max_entries: usize) -> ScanEnd {
    let mut builder = WalkBuilder::new(base);
    builder
        .standard_filters(false)
        .sort_by_file_name(OsStr::cmp)
        .filter_entry(|entry| entry.depth() == 0 || !is_git_dir(entry.file_name()));

    let mut end = ScanEnd::Complete;
    let mut entries = 0;
    let mut hits = Vec::new();
    // The directories the file as a whole excludes, by relative path. A walk
    // reaches a directory before its contents.
    let mut excluded: HashMap<PathBuf, usize> = HashMap::new();
    for entry in builder.build() {
        if cancel.load(Ordering::Relaxed) {
            return ScanEnd::Cancelled;
        }
        let Ok(entry) = entry else {
            // A directory it can't read may hold what a pattern matches.
            end = ScanEnd::Truncated;
            continue;
        };
        if entry.depth() == 0 {
            continue;
        }
        entries += 1;
        if entries > max_entries {
            return ScanEnd::Truncated;
        }
        let path = entry.path();
        let relative = path.strip_prefix(base).unwrap_or(path);
        let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());

        hits.clear();
        hits.extend(
            rules
                .iter()
                .enumerate()
                .filter(|(_, rule)| rule.matches(path, is_dir))
                .map(|(ix, _)| ix),
        );
        let Some(&last) = hits.last() else {
            continue;
        };
        let (decider_line, decider_negated) = (rules[last].line, rules[last].negated);
        if is_dir && !decider_negated {
            excluded.insert(relative.to_path_buf(), decider_line);
        }
        for &hit in &hits {
            let rule = &mut rules[hit];
            rule.matched.total += 1;
            if rule.matched.paths.len() < MAX_LISTED_PATHS {
                rule.matched.paths.push(relative.to_path_buf());
            }
            if hit != last && rule.negated != decider_negated {
                rule.overridden_by = rule.overridden_by.max(Some(decider_line));
            } else {
                rule.effective = true;
            }
            if rule.negated {
                let parent = relative
                    .ancestors()
                    .skip(1)
                    .find_map(|dir| Some((dir, *excluded.get(dir)?)));
                match parent {
                    Some((dir, by_line)) => {
                        rule.path_blocked.get_or_insert_with(|| Blocked {
                            dir: dir.to_path_buf(),
                            by_line,
                        });
                    }
                    None => rule.reincludes = true,
                }
            }
        }
    }
    end
}
