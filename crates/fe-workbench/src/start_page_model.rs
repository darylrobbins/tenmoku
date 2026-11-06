//! What the Start page lists ([`crate::start_page`]), worked out with no
//! window: its sections and their rows, each with the command invocation it
//! runs, and, while vim is on, the letter keys of LazyVim's dashboard.
//!
//! - **Start:** New File, Open File, Open Folder, and Clone Repository if a
//!   clone command is registered. With vim on, also Find Text, Recent Files
//!   and Quit, which carry the dashboard's `g`, `r` and `q`.
//! - **Recent folders** and **Recent files:** the five most recent of each
//!   that still exist ([`RecentList::existing`]), as
//!   [`OPEN_RECENT_FOLDER`] and [`OPEN_RECENT_FILE`] with their path.
//! - **Settings:** Open Settings and Keyboard Shortcuts.
//!
//! The dashboard's keys are `n` New File, `f` Open File (LazyVim's Find
//! File), `p` Open Folder (its Projects), `g` Find Text, `r` Recent Files,
//! `s` Open Settings and `q` Quit.

use std::path::{Path, PathBuf};

use fe_command::{CommandId, Invocation, Value};
use fe_config::recent::RecentList;

/// The most recent folders, and the most recent files, listed.
pub(crate) const MAX_RECENT: usize = 5;

/// Opens the folder its `path` arg names in the window, as Open Folder
/// does: a recent folder's row.
pub(crate) const OPEN_RECENT_FOLDER: &str = "start_page.open_recent_folder";

/// Opens the file its `path` arg names in the active pane: a recent file's
/// row.
pub(crate) const OPEN_RECENT_FILE: &str = "start_page.open_recent_file";

/// The clone command, listed under Start only where it's registered.
pub(crate) const CLONE: &str = "git.clone";

/// Which list a section is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SectionKind {
    Start,
    RecentFolders,
    RecentFiles,
    Settings,
}

impl SectionKind {
    pub(crate) fn title(self) -> &'static str {
        match self {
            SectionKind::Start => "Start",
            SectionKind::RecentFolders => "Recent Folders",
            SectionKind::RecentFiles => "Recent Files",
            SectionKind::Settings => "Settings",
        }
    }
}

/// The picture beside a row, for the view to draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RowIcon {
    NewFile,
    OpenFile,
    OpenFolder,
    Clone,
    Search,
    Recent,
    Quit,
    /// A recent folder.
    Folder,
    /// A recent file, by its name.
    File(String),
    Settings,
    Shortcuts,
}

/// One row: what it says, and what it runs.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub(crate) label: String,
    /// A recent entry's folder, shortened by `~`: quieter, after the label.
    pub(crate) detail: Option<String>,
    pub(crate) icon: RowIcon,
    /// The dashboard key, while vim is on.
    pub(crate) key: Option<char>,
    pub(crate) invocation: Invocation,
}

impl Row {
    /// Where in [`Row::label`] the key's letter first shows, ignoring case,
    /// for the view to underline: a byte range.
    pub(crate) fn key_in_label(&self) -> Option<std::ops::Range<usize>> {
        let key = self.key?;
        let (start, ch) =
            (self.label.char_indices()).find(|(_, ch)| ch.to_lowercase().eq(key.to_lowercase()))?;
        Some(start..start + ch.len_utf8())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Section {
    pub(crate) kind: SectionKind,
    pub(crate) rows: Vec<Row>,
}

/// The page's sections, in order; an empty recent list has no section.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StartPage {
    pub(crate) sections: Vec<Section>,
    /// Whether it was made with vim on, so has the dashboard's keys.
    pub(crate) vim: bool,
}

/// What a [`StartPage`] is made with besides the recent list.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Options<'a> {
    /// Whether vim is on: the dashboard's keys and rows.
    pub(crate) vim: bool,
    /// Whether [`CLONE`] is a registered command.
    pub(crate) clone: bool,
    /// The home directory, which details shorten to `~`.
    pub(crate) home: Option<&'a Path>,
}

impl StartPage {
    /// The page for `recent`, leaving out its entries whose paths are gone.
    /// Reads the file system: make it off the UI thread.
    pub(crate) fn from(recent: &RecentList, options: Options<'_>) -> Self {
        let Options { vim, clone, home } = options;
        let key = |key: char| vim.then_some(key);
        let mut start = vec![
            command_row("New File", RowIcon::NewFile, key('n'), "workbench.new_file"),
            command_row(
                "Open File…",
                RowIcon::OpenFile,
                key('f'),
                "workbench.open_file",
            ),
            command_row(
                "Open Folder…",
                RowIcon::OpenFolder,
                key('p'),
                "workbench.open_folder",
            ),
        ];
        if clone {
            start.push(command_row(
                "Clone Repository…",
                RowIcon::Clone,
                None,
                CLONE,
            ));
        }
        if vim {
            start.extend([
                command_row("Find Text", RowIcon::Search, key('g'), "search.show"),
                command_row(
                    "Recent Files…",
                    RowIcon::Recent,
                    key('r'),
                    "workbench.open_recent",
                ),
            ]);
        }
        let existing = recent.existing();
        let folders = (existing.folders.iter().take(MAX_RECENT))
            .map(|path| recent_row(path, true, home))
            .collect();
        let files = (existing.files.iter().take(MAX_RECENT))
            .map(|path| recent_row(path, false, home))
            .collect();
        let mut settings = vec![
            command_row(
                "Open Settings",
                RowIcon::Settings,
                key('s'),
                "workbench.open_settings_ui",
            ),
            command_row(
                "Keyboard Shortcuts",
                RowIcon::Shortcuts,
                None,
                "workbench.open_keyboard_shortcuts",
            ),
        ];
        if vim {
            settings.push(command_row("Quit", RowIcon::Quit, key('q'), "app.quit"));
        }
        let sections = [
            (SectionKind::Start, start),
            (SectionKind::RecentFolders, folders),
            (SectionKind::RecentFiles, files),
            (SectionKind::Settings, settings),
        ];
        let sections = (sections.into_iter())
            .filter(|(_, rows)| !rows.is_empty())
            .map(|(kind, rows)| Section { kind, rows })
            .collect();
        Self { sections, vim }
    }

    /// Every row, top to bottom: what up and down move through.
    pub(crate) fn rows(&self) -> impl Iterator<Item = &Row> {
        self.sections.iter().flat_map(|section| &section.rows)
    }

    pub(crate) fn row(&self, ix: usize) -> Option<&Row> {
        self.rows().nth(ix)
    }

    pub(crate) fn len(&self) -> usize {
        self.sections.iter().map(|section| section.rows.len()).sum()
    }

    /// The row whose dashboard key is `key`.
    pub(crate) fn row_for_key(&self, key: char) -> Option<usize> {
        self.rows().position(|row| row.key == Some(key))
    }

    #[cfg(test)]
    pub(crate) fn section(&self, kind: SectionKind) -> Option<&Section> {
        self.sections.iter().find(|section| section.kind == kind)
    }
}

fn invocation(command: &str) -> Invocation {
    Invocation::new(CommandId::parse(command).expect("the Start page's command IDs are valid"))
}

fn command_row(label: &str, icon: RowIcon, key: Option<char>, command: &str) -> Row {
    Row {
        label: label.to_owned(),
        detail: None,
        icon,
        key,
        invocation: invocation(command),
    }
}

/// A recent folder's or file's row: its name, then the folder it's in.
fn recent_row(path: &Path, folder: bool, home: Option<&Path>) -> Row {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let label = name.clone().unwrap_or_else(|| tilde(path, home));
    let detail = path
        .parent()
        .filter(|_| name.is_some())
        .map(|parent| tilde(parent, home));
    let (icon, command) = match folder {
        true => (RowIcon::Folder, OPEN_RECENT_FOLDER),
        false => (RowIcon::File(label.clone()), OPEN_RECENT_FILE),
    };
    let args = Value::from_iter([("path", path_value(path))]);
    Row {
        label,
        detail,
        icon,
        key: None,
        invocation: invocation(command).args(args),
    }
}

fn path_value(path: &Path) -> Value {
    Value::String(path.to_string_lossy().into_owned())
}

/// The path a recent row's invocation opens.
pub(crate) fn invocation_path(invocation: &Invocation) -> Option<PathBuf> {
    invocation.args.get("path")?.as_str().map(PathBuf::from)
}

/// `path` as shown: under the home directory, from `~`.
fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    }
}
