//! Task output as problems: text in, [`TaskProblem`]s out. Each [`Matcher`]
//! is a hand-written line parser for one tool's diagnostics; nothing here
//! runs a task or touches the Problems panel.

use std::path::{Component, Path, PathBuf};

use kiln_l10n::{tr, verbatim};

use crate::task_defs::Source;

/// The most problems one output yields.
pub const MAX_PROBLEMS: usize = 500;

/// A tool's output format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matcher {
    /// rustc and cargo: a `error[E0308]: msg` line, then ` --> path:l:c`;
    /// and a test's panic, `thread 'name' (id) panicked at path:l:c:` and its
    /// message on the next line.
    Rustc,
    /// `path(l,c): error TS2322: msg`.
    Tsc,
    /// gcc and clang: `path:l:c: warning: msg [-Wx]`.
    Gcc,
    /// `path.go:l:c: msg`, and a failing test's indented `path.go:l: msg`.
    Go,
    /// ESLint's unix format: `path:l:c: msg [Error/rule]`.
    Eslint,
    /// A Python traceback, at its innermost frame.
    Python,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskProblem {
    pub path: PathBuf,
    /// 1-based.
    pub line: u32,
    /// 1-based.
    pub column: u32,
    pub severity: Severity,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MatchResult {
    pub problems: Vec<TaskProblem>,
    /// More than [`MAX_PROBLEMS`] matched; the rest were dropped.
    pub truncated: bool,
}

impl Matcher {
    /// The matcher a task names.
    pub fn from_name(name: &str) -> Result<Self, String> {
        Ok(match name {
            "rustc" => Self::Rustc,
            "tsc" => Self::Tsc,
            "gcc" => Self::Gcc,
            "go" => Self::Go,
            "eslint" => Self::Eslint,
            "python" => Self::Python,
            other => {
                return Err(tr!("workbench-lsp-tasks-unknown-matcher", matcher = other).into());
            }
        })
    }

    /// The matcher a task without one uses: rustc for Cargo, none otherwise.
    pub fn default_for(source: Source) -> Option<Self> {
        (source == Source::Cargo).then_some(Self::Rustc)
    }
}

/// The problems in `text`. Paths resolve against `cwd`; one outside `root`
/// or for which `exists` is false is dropped.
pub fn match_output(
    matcher: Matcher,
    text: &str,
    cwd: &Path,
    root: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> MatchResult {
    let mut out = Collector {
        result: MatchResult::default(),
        cwd,
        root,
        exists,
    };
    let mut pending: Option<(Severity, String)> = None;
    let mut traceback: Option<(String, u32)> = None;
    let mut in_traceback = false;
    // A panic whose message is on the lines after it.
    let mut panic: Option<(String, u32, u32, String)> = None;
    for raw in text.lines() {
        if out.result.truncated {
            break;
        }
        let line = strip_ansi(raw);
        let line = line.trim_end_matches(['\r', '\n']);
        match matcher {
            Matcher::Rustc => {
                if let Some((path, l, c, thread)) = panic.take()
                    && !line.trim().is_empty()
                {
                    out.push(path, l, c, Severity::Error, panic_message(&thread, line));
                    continue;
                }
                if let Some(found) = rust_panic(line) {
                    match found {
                        RustPanic::Inline(path, l, c, message) => {
                            out.push(path, l, c, Severity::Error, message);
                        }
                        RustPanic::Next(path, l, c, thread) => panic = Some((path, l, c, thread)),
                    }
                    pending = None;
                } else if let Some(arrow) = line.trim_start().strip_prefix("--> ") {
                    if let (Some((severity, message)), Some((path, l, c, _))) =
                        (pending.take(), split_loc(arrow, true))
                    {
                        out.push(path, l, c, severity, message);
                    }
                } else {
                    pending = rustc_header(line);
                }
            }
            Matcher::Tsc => {
                if let Some((path, l, c, severity, message)) = tsc_line(line) {
                    out.push(path, l, c, severity, message);
                }
            }
            Matcher::Gcc => {
                if let Some((path, l, c, rest)) = split_loc(line, false)
                    && let Some((severity, message)) = gcc_message(rest)
                {
                    out.push(path, l, c, severity, message);
                }
            }
            Matcher::Go => {
                // `go test` indents a test's failures, which have no column.
                let indented = line.starts_with([' ', '\t']);
                if let Some((path, l, c, rest)) = split_loc(line.trim_start(), !indented)
                    && path.ends_with(".go")
                    && !path.contains(char::is_whitespace)
                    && !rest.is_empty()
                {
                    out.push(path, l, c, Severity::Error, rest.to_owned());
                }
            }
            Matcher::Eslint => {
                if let Some((path, l, c, rest)) = split_loc(line, true)
                    && let Some((severity, message)) = eslint_message(rest)
                {
                    out.push(path, l, c, severity, message);
                }
            }
            Matcher::Python => {
                if line.starts_with("Traceback (most recent call last):") {
                    in_traceback = true;
                    traceback = None;
                } else if in_traceback {
                    if let Some(frame) = python_frame(line) {
                        traceback = Some(frame);
                    } else if !line.starts_with([' ', '\t']) && !line.is_empty() {
                        in_traceback = false;
                        if let Some((path, l)) = traceback.take() {
                            out.push(path, l, 1, Severity::Error, line.to_owned());
                        }
                    }
                }
            }
        }
    }
    out.result
}

struct Collector<'a> {
    result: MatchResult,
    cwd: &'a Path,
    root: &'a Path,
    exists: &'a dyn Fn(&Path) -> bool,
}

impl Collector<'_> {
    fn push(&mut self, path: String, line: u32, column: u32, severity: Severity, message: String) {
        let Some(path) = self.resolve(&path) else {
            return;
        };
        if self.result.problems.len() >= MAX_PROBLEMS {
            self.result.truncated = true;
            return;
        }
        self.result.problems.push(TaskProblem {
            path,
            line,
            column,
            severity,
            message,
        });
    }

    fn resolve(&self, path: &str) -> Option<PathBuf> {
        let path = Path::new(path);
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        };
        let path = normalize(&joined);
        (path.starts_with(normalize(self.root)) && (self.exists)(&path)).then_some(path)
    }
}

/// `path` with `.` and `..` folded away, without touching the disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// `text` without its ANSI escape sequences.
fn strip_ansi(text: &str) -> String {
    if !text.contains('\x1b') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    break;
                }
            }
        } else {
            chars.next();
        }
    }
    out
}

/// `path:line:col: rest` (or `path:line: rest`, when `need_column` is false;
/// the column is then 1) as its parts.
fn split_loc(line: &str, need_column: bool) -> Option<(String, u32, u32, &str)> {
    for (at, _) in line.match_indices(':') {
        if at == 0 {
            continue;
        }
        let after = &line[at + 1..];
        let (l, rest) = take_number(after)?;
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        let (column, rest) = match take_number(rest) {
            Some((c, r)) if r.starts_with(':') || r.is_empty() => (c, r),
            _ if need_column => continue,
            _ => (1, rest),
        };
        let rest = rest.strip_prefix(':').unwrap_or(rest);
        return Some((line[..at].to_owned(), l, column.max(1), rest.trim()));
    }
    None
}

fn take_number(text: &str) -> Option<(u32, &str)> {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    if end == 0 || end > 9 {
        return None;
    }
    Some((text[..end].parse().ok()?, &text[end..]))
}

fn rustc_header(line: &str) -> Option<(Severity, String)> {
    let (severity, rest) = if let Some(rest) = line.strip_prefix("error") {
        (Severity::Error, rest)
    } else {
        (Severity::Warning, line.strip_prefix("warning")?)
    };
    let (code, message) = if let Some(rest) = rest.strip_prefix('[') {
        let (code, message) = rest.split_once("]: ")?;
        (Some(code), message)
    } else {
        (None, rest.strip_prefix(": ")?)
    };
    Some((
        severity,
        match code {
            Some(code) => tr!(
                "workbench-lsp-tasks-problem-code",
                message = message,
                code = code
            )
            .into(),
            None => message.to_owned(),
        },
    ))
}

fn tsc_line(line: &str) -> Option<(String, u32, u32, Severity, String)> {
    let open = line.find('(')?;
    let close = open + line[open..].find("): ")?;
    let (l, c) = line[open + 1..close].split_once(',')?;
    let (l, c) = (l.trim().parse().ok()?, c.trim().parse().ok()?);
    let rest = &line[close + 3..];
    let (severity, rest) = if let Some(rest) = rest.strip_prefix("error ") {
        (Severity::Error, rest)
    } else {
        (Severity::Warning, rest.strip_prefix("warning ")?)
    };
    let (code, message) = rest.split_once(": ")?;
    code.starts_with("TS").then_some(())?;
    Some((line[..open].to_owned(), l, c, severity, message.to_owned()))
}

fn gcc_message(rest: &str) -> Option<(Severity, String)> {
    for (prefix, severity) in [
        (verbatim!("fatal error: "), Severity::Error),
        (verbatim!("error: "), Severity::Error),
        (verbatim!("warning: "), Severity::Warning),
    ] {
        if let Some(message) = rest.strip_prefix(prefix) {
            return Some((severity, message.to_owned()));
        }
    }
    None
}

fn eslint_message(rest: &str) -> Option<(Severity, String)> {
    let rest = rest.trim_end();
    let open = rest.rfind(" [")?;
    let tag = rest[open + 2..].strip_suffix(']')?;
    let severity = match tag.split('/').next()? {
        "Error" => Severity::Error,
        "Warning" => Severity::Warning,
        _ => return None,
    };
    Some((severity, rest[..open].to_owned()))
}

/// A panic's line.
enum RustPanic {
    /// Before Rust 1.73: `thread 'name' panicked at 'message', path:l:c`.
    Inline(String, u32, u32, String),
    /// `thread 'name' panicked at path:l:c:`, the message on the next line
    /// (and on newer toolchains the thread's id after its name, `'name' (12345)`),
    /// with the thread's name, a test's.
    Next(String, u32, u32, String),
}

fn rust_panic(line: &str) -> Option<RustPanic> {
    let rest = line.strip_prefix("thread '")?;
    let (thread, rest) = rest.split_once('\'')?;
    // Newer toolchains print the thread's id after its name: `'name' (12345)`.
    let rest = match rest.strip_prefix(" (") {
        Some(id) => {
            let (id, rest) = id.split_once(')')?;
            id.chars().all(|c| c.is_ascii_digit()).then_some(rest)?
        }
        None => rest,
    };
    let rest = rest.strip_prefix(" panicked at ")?;
    if let Some(quoted) = rest.strip_prefix('\'') {
        let (message, location) = quoted.rsplit_once("', ")?;
        let (path, l, c, _) = split_loc(location, true)?;
        let message = panic_message(thread, message);
        return Some(RustPanic::Inline(path, l, c, message));
    }
    let (path, l, c, after) = split_loc(rest.trim_end(), true)?;
    after
        .is_empty()
        .then(|| RustPanic::Next(path, l, c, thread.to_owned()))
}

/// A panic's message, naming the test whose thread it was.
fn panic_message(thread: &str, message: &str) -> String {
    let message = message.trim();
    match thread {
        "main" | "<unnamed>" => message.to_owned(),
        test => tr!("workbench-lsp-tasks-panic", test = test, message = message).into(),
    }
}

/// `  File "x.py", line 3, in f`.
fn python_frame(line: &str) -> Option<(String, u32)> {
    let rest = line.trim_start().strip_prefix("File \"")?;
    let (path, rest) = rest.split_once(verbatim!("\", line "))?;
    let (l, _) = take_number(rest)?;
    Some((path.to_owned(), l))
}
