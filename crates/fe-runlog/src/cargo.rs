//! `cargo build --message-format=json-render-diagnostics`: a JSON object per
//! stdout line, plus the human lines on stderr, read into a per-crate
//! timeline for the build waterfall.
//!
//! ```text
//! {"reason":"compiler-artifact","package_id":"registry+…#serde_json@1.0.151","fresh":false,…}
//! {"reason":"compiler-message","message":{"level":"warning","rendered":"…","spans":[…]},…}
//! {"reason":"build-finished","success":true}
//! ```
//!
//! Stderr's `Compiling name vX.Y.Z (…)` line says when a crate started; cargo
//! doesn't say so on stdout. A crate that was fresh has no such line and its
//! artifact is reported at once, so its span is zero-length. With cargo's
//! `timing-info` records in the stream (`--timings=json`, unstable) their
//! durations are used for the ends instead.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

use crate::clean_line;

/// One crate's part of a build.
#[derive(Clone, Debug, PartialEq)]
pub struct CrateSpan {
    /// The package's name (`serde_json`).
    pub name: String,
    /// The package's version, empty when cargo didn't say.
    pub version: String,
    /// When compiling it started, from the start of the build.
    pub start: Duration,
    /// When its artifact was done (`start` for a fresh crate).
    pub end: Duration,
    /// It was up to date: nothing was compiled.
    pub fresh: bool,
    /// It is a path dependency (a `path+file://` package id): the crate the
    /// user is editing, rather than one from a registry or git.
    pub local: bool,
}

impl CrateSpan {
    /// How long the crate took.
    pub fn duration(&self) -> Duration {
        self.end.saturating_sub(self.start)
    }
}

/// A warning or error the compiler printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildMessage {
    /// The package it came from.
    pub package: String,
    /// `warning`, `error`, …, as rustc says.
    pub level: String,
    /// The primary span's file, line and column (1-based), if it has one.
    pub file: Option<String>,
    /// See `file`.
    pub line: Option<usize>,
    /// See `file`.
    pub column: Option<usize>,
    /// The diagnostic as rustc rendered it.
    pub rendered: String,
}

/// A build, read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildTimeline {
    /// The crates, in the order cargo first mentioned them.
    pub spans: Vec<CrateSpan>,
    /// The warnings and errors, in order.
    pub messages: Vec<BuildMessage>,
    /// How many warnings there were (a "N warnings emitted" summary isn't one).
    pub warnings: usize,
    /// How many errors there were ("aborting due to" isn't one).
    pub errors: usize,
    /// `build-finished` said it succeeded; false when it failed or never came.
    pub success: bool,
}

#[derive(Debug)]
struct Entry {
    span: CrateSpan,
    /// Summed `timing-info` durations.
    timed: Option<Duration>,
    /// A `compiler-artifact` has said whether the crate was fresh.
    artifact: bool,
    /// The package id, once an artifact, message or timing record named it.
    id: Option<String>,
    /// A `Compiling` line has said when the crate started.
    compiling: bool,
    /// For an unbound entry from a `Compiling` line: the path in its
    /// parentheses, which a path package id is matched on.
    path: Option<String>,
}

/// Reads a build's stdout and stderr lines, each with when it arrived, into a
/// [`BuildTimeline`]. Lines that aren't what the parser knows are skipped.
#[derive(Debug, Default)]
pub struct CargoParser {
    entries: Vec<Entry>,
    /// Index into `entries` by package id.
    by_id: HashMap<String, usize>,
    timeline: BuildTimeline,
    /// The index in `timeline.messages` of the stderr diagnostic being read:
    /// its lines go on until the next diagnostic or cargo's next status line.
    open: Option<usize>,
}

impl CargoParser {
    /// A parser at the start of a build.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads one stdout line, which arrived `at` after the build started.
    pub fn feed_stdout(&mut self, line: &str, at: Duration) {
        let line = clean_line(line);
        let Ok(Value::Object(message)) = serde_json::from_str::<Value>(line.trim()) else {
            return;
        };
        let Some(reason) = message.get("reason").and_then(Value::as_str) else {
            return;
        };
        match reason {
            "compiler-artifact" | "build-script-executed" => {
                let Some(id) = package_id(&message) else {
                    return;
                };
                let index = self.entry(id, at);
                let entry = &mut self.entries[index];
                entry.span.end = entry.span.end.max(at);
                if reason == "compiler-artifact" {
                    let fresh = message.get("fresh").and_then(Value::as_bool) == Some(true);
                    entry.span.fresh = (!entry.artifact || entry.span.fresh) && fresh;
                    entry.artifact = true;
                }
            }
            "timing-info" => {
                let Some(id) = package_id(&message) else {
                    return;
                };
                // A duration that doesn't fit, or whose sum doesn't, is skipped.
                let Some(secs) = message
                    .get("duration")
                    .and_then(Value::as_f64)
                    .and_then(|secs| Duration::try_from_secs_f64(secs).ok())
                else {
                    return;
                };
                let index = self.entry(id, at);
                let timed = &mut self.entries[index].timed;
                if let Some(sum) = timed.unwrap_or_default().checked_add(secs) {
                    *timed = Some(sum);
                }
            }
            "compiler-message" => self.message(&message, at),
            "build-finished" => {
                self.timeline.success =
                    message.get("success").and_then(Value::as_bool) == Some(true);
            }
            _ => {}
        }
    }

    /// Reads one stderr line, which arrived `at` after the build started.
    pub fn feed_stderr(&mut self, line: &str, at: Duration) {
        let line = clean_line(line);
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Compiling ") {
            self.open = None;
            let mut words = rest.split_whitespace();
            let (Some(name), Some(version)) = (words.next(), words.next()) else {
                return;
            };
            let version = version.strip_prefix('v').unwrap_or(version);
            let path = rest
                .split_once('(')
                .and_then(|(_, path)| path.strip_suffix(')'))
                .map(str::to_owned);
            // The package may already have an entry: a build script's artifact
            // comes before its library is compiled.
            let existing = self.entries.iter().position(|entry| {
                !entry.compiling
                    && entry.span.name == name
                    && entry.span.version == version
                    && match &entry.id {
                        Some(id) => source_matches(path.as_deref(), id),
                        None => false,
                    }
            });
            if let Some(index) = existing {
                let entry = &mut self.entries[index];
                entry.compiling = true;
                entry.span.start = at;
                entry.span.end = entry.span.end.max(at);
                entry.span.fresh = false;
                return;
            }
            self.entries.push(Entry {
                span: CrateSpan {
                    name: name.to_owned(),
                    version: version.to_owned(),
                    start: at,
                    end: at,
                    fresh: false,
                    local: false,
                },
                timed: None,
                artifact: false,
                id: None,
                compiling: true,
                path,
            });
            return;
        }
        if let Some((is_error, text)) = diagnostic_header(&line) {
            self.open = None;
            if is_summary(text) {
                return;
            }
            if is_error {
                self.timeline.errors += 1;
            } else {
                self.timeline.warnings += 1;
            }
            self.timeline.messages.push(BuildMessage {
                package: String::new(),
                level: if is_error { "error" } else { "warning" }.to_owned(),
                file: None,
                line: None,
                column: None,
                rendered: line.into_owned(),
            });
            self.open = Some(self.timeline.messages.len() - 1);
            return;
        }
        if is_status(trimmed) {
            self.open = None;
            return;
        }
        let Some(open) = self.open else {
            return;
        };
        let message = &mut self.timeline.messages[open];
        if message.file.is_none()
            && let Some(location) = trimmed.strip_prefix("--> ")
        {
            let mut parts = location.rsplitn(3, ':');
            let column = parts.next().and_then(|n| n.parse().ok());
            let line_no = parts.next().and_then(|n| n.parse().ok());
            if let Some(file) = parts.next() {
                message.file = Some(file.to_owned());
                message.line = line_no;
                message.column = column;
            }
        }
        message.rendered.push('\n');
        message.rendered.push_str(&line);
    }

    /// The build has ended: returns the timeline.
    pub fn finish(mut self) -> BuildTimeline {
        let mut timeline = std::mem::take(&mut self.timeline);
        for message in &mut timeline.messages {
            message.rendered.truncate(message.rendered.trim_end().len());
        }
        timeline.spans = self
            .entries
            .into_iter()
            .map(
                |Entry {
                     mut span,
                     timed,
                     artifact,
                     ..
                 }| {
                    // A crate that never got an artifact failed, or was cut short.
                    if !artifact {
                        span.fresh = false;
                    }
                    if let Some(end) = timed.and_then(|timed| span.start.checked_add(timed)) {
                        span.end = end;
                    }
                    if span.fresh {
                        span.end = span.start;
                    }
                    span
                },
            )
            .collect();
        timeline
    }

    /// The entry of package `id`: the one already bound to it, else the
    /// first stderr `Compiling` line's that fits it, else a new one made at `at`.
    fn entry(&mut self, id: &str, at: Duration) -> usize {
        if let Some(&index) = self.by_id.get(id) {
            return index;
        }
        let (name, version) = split_package_id(id);
        let local = id.starts_with("path+file://");
        let placeholder = self.entries.iter().position(|entry| {
            entry.id.is_none()
                && entry.span.name == name
                && entry.span.version == version
                && source_matches(entry.path.as_deref(), id)
        });
        let index = placeholder.unwrap_or_else(|| {
            self.entries.push(Entry {
                span: CrateSpan {
                    name,
                    version,
                    start: at,
                    end: at,
                    fresh: true,
                    local,
                },
                timed: None,
                artifact: false,
                id: None,
                compiling: false,
                path: None,
            });
            self.entries.len() - 1
        });
        let entry = &mut self.entries[index];
        entry.id = Some(id.to_owned());
        entry.span.local = local;
        self.by_id.insert(id.to_owned(), index);
        index
    }

    fn message(&mut self, message: &serde_json::Map<String, Value>, at: Duration) {
        let Some(diagnostic) = message.get("message") else {
            return;
        };
        let field = |key: &str| diagnostic.get(key).and_then(Value::as_str);
        let (Some(level), Some(rendered)) = (field("level"), field("rendered")) else {
            return;
        };
        let spans = diagnostic.get("spans").and_then(Value::as_array);
        let primary = spans.and_then(|spans| {
            spans
                .iter()
                .find(|span| span.get("is_primary").and_then(Value::as_bool) == Some(true))
        });
        let package = package_id(message).map(split_package_id);
        if let Some(id) = package_id(message) {
            // Only so the crate starts no later than its first message: cargo
            // replays a cached crate's warnings, so a message says nothing
            // about freshness (its artifact does).
            self.entry(id, at);
        }
        if primary.is_none() && is_summary(field("message").unwrap_or_default()) {
            return;
        }
        if level.starts_with("error") {
            self.timeline.errors += 1;
        } else if level == "warning" {
            self.timeline.warnings += 1;
        } else {
            return;
        }
        let number = |key: &str| {
            primary
                .and_then(|span| span.get(key))
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
        };
        self.timeline.messages.push(BuildMessage {
            package: package.map(|(name, _)| name).unwrap_or_default(),
            level: level.to_owned(),
            file: primary
                .and_then(|span| span.get("file_name"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            line: number("line_start"),
            column: number("column_start"),
            rendered: rendered.trim_end().to_owned(),
        });
    }
}

fn package_id(message: &serde_json::Map<String, Value>) -> Option<&str> {
    message.get("package_id").and_then(Value::as_str)
}

/// Whether a `Compiling` line's parenthesised source (a path, a git url, or
/// nothing for a registry crate) is that of package `id`.
fn source_matches(source: Option<&str>, id: &str) -> bool {
    let url = id.split('#').next().unwrap_or(id);
    match source {
        Some(path) => match url.strip_prefix("path+file://") {
            Some(url_path) => percent_decode(url_path) == path,
            None => url.contains(path.split('#').next().unwrap_or(path)),
        },
        None => id.starts_with("registry+") || id.starts_with("sparse+"),
    }
}

/// `text` with its `%20`-style escapes decoded, as in a `file://` url's path.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A cargo status line (`   Finished dev profile …`): its verb, then text.
/// A diagnostic's lines never start with one of these.
fn is_status(trimmed: &str) -> bool {
    const VERBS: &[&str] = &[
        "Adding",
        "Blocking",
        "Building",
        "Checking",
        "Compiled",
        "Compiling",
        "Dirty",
        "Documenting",
        "Downloaded",
        "Downloading",
        "Finished",
        "Fixed",
        "Fresh",
        "Generated",
        "Installing",
        "Locking",
        "Packaging",
        "Removing",
        "Running",
        "Updating",
        "Uploading",
        "Verifying",
    ];
    trimmed
        .split_once(' ')
        .is_some_and(|(verb, _)| VERBS.contains(&verb))
}

/// A diagnostic's first line, `warning: text` or `error[E0308]: text`: whether
/// it is an error, and the text.
fn diagnostic_header(line: &str) -> Option<(bool, &str)> {
    if line.starts_with(' ') {
        return None;
    }
    let (level, text) = line.split_once(':')?;
    let is_error = level == "error" || (level.starts_with("error[") && level.ends_with(']'));
    (is_error || level == "warning").then(|| (is_error, text.trim()))
}

/// The lines rustc and cargo print after a crate's diagnostics, which count
/// nothing: "aborting due to 1 previous error", "2 warnings emitted", "`a`
/// (lib) generated 2 warnings (run `cargo fix …` to apply 1 suggestion)" and
/// "could not compile `a` (lib) due to 1 previous error".
fn is_summary(text: &str) -> bool {
    let words: Vec<_> = text.split_whitespace().collect();
    let counted = |word: &str| matches!(word, "warning" | "warnings" | "error" | "errors");
    text.starts_with("aborting due to ")
        || text.starts_with("could not compile `")
        || matches!(words[..], [n, what, "emitted"] if n.parse::<u32>().is_ok() && counted(what))
        || (text.starts_with('`')
            && matches!(words.iter().position(|&w| w == "generated"),
                Some(at) if words.get(at + 1).is_some_and(|n| n.parse::<u32>().is_ok())
                    && words.get(at + 2).is_some_and(|&w| counted(w))))
}

/// A package id's name and version. Cargo has written them as
/// `registry+https://…#serde_json@1.0.151`, `path+file:///dir/fe-runlog#0.1.5`
/// (the name is the directory's) and `path+file:///dir/x#name@0.1.5`.
fn split_package_id(id: &str) -> (String, String) {
    let (url, fragment) = id.split_once('#').unwrap_or((id, ""));
    match fragment.split_once('@') {
        Some((name, version)) => (name.to_owned(), version.to_owned()),
        None => {
            let from_url = url.rsplit('/').next().unwrap_or_default();
            (from_url.to_owned(), fragment.to_owned())
        }
    }
}
