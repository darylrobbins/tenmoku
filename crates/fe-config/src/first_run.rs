//! Whether this launch is the machine's first, for the setup wizard.
//!
//! The marker is `<state_dir>/setup.toml`: state, not settings, because it
//! records this machine and settings may be synced between machines.
//! [`decide`] reads only; [`record`] writes the marker atomically.
//!
//! People who already use Tenmoku must never be asked, and the config and
//! state directories moved with no migration, so "no marker" alone is not
//! enough: any file a user makes by using the app counts as [`FirstRun::Existing`].

use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use kiln_core::KEYMAP_FILE;

use crate::paths::{SETTINGS_FILE, THEME_FILE, VIMRC_FILE, app_paths_from};

/// The marker's file name, in the state directory.
pub const MARKER_FILE: &str = "setup.toml";

/// What the setup wizard's marker records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The wizard was put up; nothing is known of how it ended.
    Shown,
    /// The person finished it.
    Completed,
    /// The person skipped it.
    Skipped,
    /// The person already used Tenmoku, so it was never shown.
    Existing,
    /// A launch that must not show it (`--wait`, `--perf`) found a new
    /// machine: no evidence of use, and the wizard still to offer. Its own
    /// writes are not evidence either, so [`decide`] shows it.
    Pending,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::Shown => "shown",
            Outcome::Completed => "completed",
            Outcome::Skipped => "skipped",
            Outcome::Existing => "existing",
            Outcome::Pending => "pending",
        }
    }
}

/// What to do about the wizard at launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstRun {
    /// A new machine: show the wizard.
    Show,
    /// No marker, but the person already uses Tenmoku: show nothing, and
    /// record [`Outcome::Existing`].
    Existing,
    /// Already decided, or nothing could be recorded: show nothing.
    Done,
}

/// Paths this process is about to write, by whether they were absent (or an
/// empty directory) before its first write there: what it made itself is not
/// evidence of an earlier use.
static WRITING: Mutex<Option<HashMap<PathBuf, bool>>> = Mutex::new(None);

/// Notes that this process is about to write `path`, a file or a directory:
/// call it before the process's first write there. Only the first call per
/// path counts. The state listed after it is then known: `path` that was
/// absent, or an empty directory, was made by this launch, and [`decide`]
/// does not take it for use.
pub fn note_writing(path: &Path) {
    let mut noted = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    noted
        .get_or_insert_with(HashMap::new)
        .entry(path.to_owned())
        .or_insert_with(|| match fs::read_dir(path) {
            Ok(mut entries) => entries.next().is_none(),
            Err(error) => error.kind() == io::ErrorKind::NotFound,
        });
}

/// Whether this launch made `path`: see [`note_writing`].
fn made_here(path: &Path) -> bool {
    let noted = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    noted
        .as_ref()
        .and_then(|noted| noted.get(path))
        .copied()
        .unwrap_or(false)
}

/// Whether `path` is there. A lookup that fails other than by absence is
/// logged and answered with `unknown`.
fn present(path: &Path, unknown: bool) -> bool {
    match fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => {
            log::warn!("{}: {error}", path.display());
            unknown
        }
    }
}

/// Decides what to do about the wizard. Reads only. Anything that can't be
/// read counts as the safe answer: no marker lookup that fails asks again,
/// and unreadable evidence of use counts as use.
pub fn decide(config_dir: Option<&Path>, state_dir: Option<&Path>) -> FirstRun {
    let Some(state) = state_dir else {
        return FirstRun::Done;
    };
    let marker = state.join(MARKER_FILE);
    if present(&marker, true) {
        match fs::read_to_string(&marker) {
            Ok(text) => match text.parse::<toml::Table>() {
                // A launch that couldn't show it left the wizard to offer,
                // and wrote layouts of its own meanwhile.
                Ok(table) if table.get("outcome").and_then(|o| o.as_str()) == Some("pending") => {
                    return FirstRun::Show;
                }
                Ok(_) => {}
                Err(_) => log::warn!("{} does not parse; not asking again", marker.display()),
            },
            Err(error) => log::warn!("{}: {error}; not asking again", marker.display()),
        }
        return FirstRun::Done;
    }
    if config_dir.is_some_and(config_has_user_files) || state_has_user_files(state) {
        FirstRun::Existing
    } else {
        FirstRun::Show
    }
}

fn config_has_user_files(config: &Path) -> bool {
    if settings_in_use(&config.join(SETTINGS_FILE)) {
        return true;
    }
    [KEYMAP_FILE, THEME_FILE, VIMRC_FILE]
        .iter()
        .any(|name| present(&config.join(name), true))
}

/// A `settings.toml` that parses to a non-empty table, or doesn't parse. The
/// template, every key commented out, parses to an empty table.
fn settings_in_use(path: &Path) -> bool {
    match fs::read_to_string(path) {
        Ok(text) => text
            .parse::<toml::Table>()
            .map_or(true, |table| has_keys(&table)),
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    }
}

/// Whether a table holds any key; tables of empty tables, which the template's
/// section headers make, do not.
fn has_keys(table: &toml::Table) -> bool {
    table.values().any(|value| match value {
        toml::Value::Table(inner) => has_keys(inner),
        _ => true,
    })
}

fn state_has_user_files(state: &Path) -> bool {
    let recent = crate::recent_path()
        .and_then(|path| path.file_name().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("recent.toml"));
    let recent = state.join(recent);
    let layouts =
        app_paths_from(|name| (name == "TENMOKU_STATE_DIR").then(|| state.as_os_str().to_owned()))
            .layouts_dir()
            .unwrap_or_else(|| state.join("layouts"));
    // Each is listed first and the registry read after: whatever this launch
    // wrote and is listed here was noted before it was written.
    let recent_there = present(&recent, true);
    if recent_there && !made_here(&recent) {
        return true;
    }
    let layouts_there = match fs::read_dir(&layouts) {
        Ok(mut entries) => entries.next().is_some(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => {
            log::warn!("{}: {error}", layouts.display());
            true
        }
    };
    layouts_there && !made_here(&layouts)
}

/// Writes the marker, creating the state directory; a later call replaces
/// the outcome (shown, then completed or skipped).
pub fn record(state_dir: &Path, outcome: Outcome) -> io::Result<()> {
    fs::create_dir_all(state_dir)?;
    let text = format!(
        "outcome = \"{}\"\nversion = \"{}\"\n",
        outcome.as_str(),
        env!("CARGO_PKG_VERSION")
    );
    // A unique name made exclusively (`create_new` does not follow a planted
    // symlink), so concurrent records never share a temp file.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (tmp, mut file) = loop {
        let tmp = state_dir.join(format!(
            "{MARKER_FILE}.{}.{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => break (tmp, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let written = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, state_dir.join(MARKER_FILE))
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}
