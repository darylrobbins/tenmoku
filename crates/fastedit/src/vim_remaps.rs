//! The user's vim remaps as a source of kiln's config service: the vimrc
//! (`~/.config/tenmoku/vimrc`, the subset [`fe_vim::parse_vimrc`] reads)
//! and the settings' `[[vim.remap]]` table with `vim.leader`, built into
//! one set for every editor and reloaded when any of their files changes.
//! Beneath them, a bundled preset (`vim.preset`) in the same vimrc subset.
//! The names which-key gives groups of maps (the preset's and the vimrc's
//! `:fegroup`s, then `[[vim.which_key_group]]`) load in the same order.
//! Their problems go to the config problems list, each at its line: see
//! `docs/decisions/vim-remaps.md` §1(c) and §7.
//!
//! The text a settings file's problems are placed in is the text the
//! settings store accepted ([`SettingsStore::accepted_sources`], and on a
//! reload `accepted_sources`), never a fresh read of the
//! files, so a file that changes after the store took it cannot misplace
//! them.

use kiln_l10n::tr;
use std::{
    fs::OpenOptions,
    io::{self, Read as _},
    path::{Path, PathBuf},
};

use fe_config::{
    Diagnostic, Settings, SettingsStore, Severity, VimPreset, VimRemap, VimRemapMode,
    VimWhichKeyGroup,
};
use fe_vim::{
    Remap, RemapMode, RemapSet, Vimrc, WhichKeyGroupName, parse_keys, parse_vimrc_with_leader,
};
use gpui::{App, Global};
use kiln::{GlobalSettings, platform::config::ConfigSource};
use std::sync::Arc;
use toml::{
    Spanned,
    de::{DeTable, DeValue},
};

/// Files bigger than this aren't read, as kiln reads no config file
/// bigger.
const MAX_BYTES: u64 = 1 << 20;

/// `vim.leader`'s default, which a preset's leader replaces.
const DEFAULT_LEADER: &str = "\\";

/// A bundled preset: a vimrc-subset file loaded beneath the user's maps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Preset {
    /// The file name its problems are listed against.
    pub(crate) name: &'static str,
    pub(crate) text: &'static str,
    /// What `<leader>` is while `vim.leader` is left at its default.
    pub(crate) leader: &'static str,
}

/// Every bundled preset.
pub(crate) const PRESETS: &[(VimPreset, Preset)] = &[(
    VimPreset::Lazyvim,
    Preset {
        name: "lazyvim.vimrc",
        text: include_str!("../presets/lazyvim.vimrc"),
        leader: "<Space>",
    },
)];

/// The bundled preset `preset` loads, none for plain vim.
fn bundled(preset: VimPreset) -> Option<Preset> {
    (PRESETS.iter())
        .find(|(name, _)| *name == preset)
        .map(|(_, preset)| *preset)
}

/// The records every editor's set was last built from, and the leader.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RemapsInUse {
    pub(crate) remaps: Vec<Remap>,
    pub(crate) leader: String,
}

impl Global for RemapsInUse {}

/// The preset's maps first, then the vimrc's, then the settings'
/// `vim.remap` entries, so that for the same mode and keys the later wins.
pub(crate) struct VimRemaps {
    vimrc: Option<PathBuf>,
    /// The settings files, the user's first: a later one's `vim.remap` or
    /// `vim.leader` replaces an earlier one's, so its problems are in it.
    settings_files: Vec<PathBuf>,
    /// Each settings file's text as the settings store last took it: a
    /// file with errors keeps its last good table, empty until it has
    /// loaded once, so it keeps that text here.
    accepted: Vec<String>,
    /// The vimrc's text: as last read, or as last read well while it
    /// can't be read. It's parsed as the set is built, since with a
    /// preset its `<leader>` starts as the leader in use.
    vimrc_text: String,
    /// The error reading the vimrc, while it can't be read.
    vimrc_error: Option<Diagnostic>,
    /// The vimrc's maps, as last built.
    vimrc_remaps: Vec<Remap>,
    /// The line each of `vimrc_remaps` was written on.
    vimrc_lines: Vec<usize>,
    /// The vimrc's `:fegroup` names, as last built.
    vimrc_groups: Vec<WhichKeyGroupName>,
    /// A warning for each of the vimrc's problems, or the one error
    /// reading it.
    vimrc_diagnostics: Vec<Diagnostic>,
    /// The settings' `vim.remap`, `vim.leader` and `vim.preset`, as last
    /// seen.
    table: Vec<VimRemap>,
    leader: String,
    preset: VimPreset,
    /// The settings' `vim.which_key_group`, as last seen.
    group_table: Vec<VimWhichKeyGroup>,
    /// The preset `vim.preset` names: [`bundled`], but for tests.
    presets: fn(VimPreset) -> Option<Preset>,
    /// The preset's maps, the line each was written on, and a warning for
    /// each of its problems, against its name; as last built.
    preset_remaps: Vec<Remap>,
    preset_lines: Vec<usize>,
    preset_diagnostics: Vec<Diagnostic>,
    /// The preset's `:fegroup` names, as last built.
    preset_groups: Vec<WhichKeyGroupName>,
    /// A warning for each `vim.remap` or `vim.which_key_group` entry that
    /// can't load, and for a `vim.leader` that can't be read, at its line.
    table_diagnostics: Vec<Diagnostic>,
    /// The user's keymap file, watched so a change to it is seen.
    keymap_file: Option<PathBuf>,
    /// The keymap the conflicts were last found against.
    keymap: Option<Arc<fe_gpui::fe_keymap::Keymap>>,
    /// A warning for each insert or cmdline map the keymap hides, at the
    /// vimrc (or preset) line that wrote it.
    vimrc_conflicts: Vec<Diagnostic>,
}

/// A problem with the settings' remaps, before it's found in a file.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Problem {
    Leader(String),
    /// The entry of `vim.remap`, and which of its keys the problem is in.
    Entry(usize, &'static str, String),
    /// The entry of `vim.which_key_group`, and which of its keys can't be
    /// read: `keys` (the entry is left out) or `icon` (it loads without).
    Group(usize, &'static str, String),
}

impl VimRemaps {
    /// The remaps of the vimrc at `vimrc` and of `settings`, the store the
    /// config service is given: read now, so their diagnostics are there
    /// before the service starts.
    pub(crate) fn new(vimrc: Option<PathBuf>, settings: &SettingsStore) -> Self {
        let settings_files = settings.paths();
        let vim = &settings.settings().vim;
        let mut remaps = Self {
            vimrc,
            accepted: (settings.accepted_sources().into_iter())
                .map(str::to_owned)
                .collect(),
            settings_files,
            vimrc_text: String::new(),
            vimrc_error: None,
            vimrc_remaps: Vec::new(),
            vimrc_lines: Vec::new(),
            vimrc_groups: Vec::new(),
            vimrc_diagnostics: Vec::new(),
            table: vim.remap.clone(),
            leader: vim.leader.clone(),
            preset: vim.preset,
            group_table: vim.which_key_group.clone(),
            presets: bundled,
            preset_remaps: Vec::new(),
            preset_lines: Vec::new(),
            preset_diagnostics: Vec::new(),
            preset_groups: Vec::new(),
            table_diagnostics: Vec::new(),
            keymap_file: fe_gpui::fe_keymap::user_keymap_path(),
            keymap: None,
            vimrc_conflicts: Vec::new(),
        };
        remaps.read_vimrc();
        remaps.build(None);
        remaps
    }

    /// Reads the vimrc, a missing one being empty. One that can't be read
    /// keeps the text last read, with one error at its first line.
    fn read_vimrc(&mut self) {
        let Some(path) = &self.vimrc else {
            return;
        };
        match read(path) {
            Ok(text) => {
                self.vimrc_text = text;
                self.vimrc_error = None;
            }
            Err(message) => {
                self.vimrc_error = Some(at_line(path, Severity::Error, 1, message));
            }
        }
    }

    /// Parses the preset's text and the vimrc's with `<leader>` starting
    /// as `leader`, readable vim notation.
    fn parse_files(&mut self, preset: Option<Preset>, leader: &str) {
        let issues = |path: &Path, vimrc: &Vimrc| {
            (vimrc.issues.iter())
                .map(|issue| at_line(path, Severity::Warning, issue.line, issue.message.clone()))
                .collect()
        };
        match preset {
            Some(preset) => {
                let parsed = parse_with_leader(preset.text, leader);
                self.preset_diagnostics = issues(Path::new(preset.name), &parsed);
                self.preset_remaps = parsed.remaps;
                self.preset_lines = parsed.lines;
                self.preset_groups = parsed.groups;
                // Only the bundled preset's names are keys: a test's own
                // preset, like a user's groups, stays as written.
                if std::ptr::fn_addr_eq(self.presets, bundled as fn(VimPreset) -> Option<Preset>) {
                    for group in &mut self.preset_groups {
                        group.name = fe_vim::preset_group_name(&group.name);
                    }
                }
            }
            None => {
                self.preset_remaps.clear();
                self.preset_lines.clear();
                self.preset_diagnostics.clear();
                self.preset_groups.clear();
            }
        }
        let Some(path) = &self.vimrc else {
            return;
        };
        let parsed = parse_with_leader(&self.vimrc_text, leader);
        self.vimrc_diagnostics = match &self.vimrc_error {
            Some(error) => vec![error.clone()],
            None => issues(path, &parsed),
        };
        self.vimrc_remaps = parsed.remaps;
        self.vimrc_lines = parsed.lines;
        self.vimrc_groups = parsed.groups;
    }

    /// Takes `vim.remap`, `vim.leader`, `vim.preset` and
    /// `vim.which_key_group` from the settings
    /// in use, returning whether they changed, and the text the store
    /// accepted of each file.
    fn take_settings(&mut self, cx: &App) -> bool {
        let Some(global) = cx.try_global::<GlobalSettings<Settings>>() else {
            return false;
        };
        self.accepted = kiln::platform::config::accepted_sources::<Settings>(cx);
        let vim = &global.settings().vim;
        if vim.remap == self.table
            && vim.leader == self.leader
            && vim.preset == self.preset
            && vim.which_key_group == self.group_table
        {
            return false;
        }
        self.table = vim.remap.clone();
        self.leader = vim.leader.clone();
        self.preset = vim.preset;
        self.group_table = vim.which_key_group.clone();
        true
    }

    /// Builds the set: the preset's records, the vimrc's, then the
    /// table's. With `cx`, gives it to every editor unless it's the one
    /// they have. Then finds the settings' problems in their files.
    ///
    /// The leader is `vim.leader`, or the preset's while `vim.leader` is
    /// left at its default. The preset's and the vimrc's records are
    /// checked as they are parsed, their `<leader>`s put in: the preset's
    /// as the leader, and, with a preset, the vimrc's starting as it, as
    /// LazyVim sets `mapleader` before the user's config; with none, the
    /// vimrc's own (`\` until it sets one), whatever `vim.leader` is. A
    /// `vim.leader` that can't be read is one problem, and the entries
    /// naming `<leader>` are left out, each with its own.
    fn build(&mut self, cx: Option<&mut App>) {
        let mut problems = Vec::new();
        let preset = (self.presets)(self.preset);
        let leader = match preset {
            Some(preset) if self.leader == DEFAULT_LEADER => preset.leader,
            _ => &self.leader,
        };
        let readable = match parse_keys(leader) {
            Ok(_) => true,
            Err(error) => {
                problems.push(Problem::Leader(error.to_string()));
                false
            }
        };
        // Any leader will do for records that name none.
        let leader = if readable {
            leader.to_owned()
        } else {
            preset
                .map_or(DEFAULT_LEADER, |preset| preset.leader)
                .to_owned()
        };
        let vimrc_leader = if preset.is_some() {
            leader.as_str()
        } else {
            DEFAULT_LEADER
        };
        self.parse_files(preset, vimrc_leader);
        let mut remaps = self.preset_remaps.clone();
        remaps.extend_from_slice(&self.vimrc_remaps);
        // The entry each of the table's records is, in order.
        let mut entries = Vec::new();
        for (entry, remap) in self.table.iter().enumerate() {
            if !readable && let Some(side) = names_leader(remap) {
                let message = tr!("fastedit-vim-leader-unreadable").to_string();
                problems.push(Problem::Entry(entry, side, message));
                continue;
            }
            entries.push(entry);
            remaps.push(Remap {
                modes: remap_modes(remap.mode.modes()),
                from: remap.from.clone(),
                to: remap.to.clone(),
                recursive: remap.recursive,
            });
        }
        let groups = self.groups(readable, &leader, &mut problems);
        let in_use = RemapsInUse { remaps, leader };
        let in_use_keymap = cx.as_deref().map(fe_gpui::current_keymap);
        let issues = match cx {
            Some(cx) => {
                fe_gpui::set_vim_which_key_groups(cx, groups);
                if cx.try_global::<RemapsInUse>() != Some(&in_use) {
                    let issues = fe_gpui::set_vim_remaps(cx, &in_use.remaps, &in_use.leader);
                    cx.set_global(in_use.clone());
                    issues
                } else {
                    RemapSet::new(&in_use.remaps, &in_use.leader).1
                }
            }
            None => RemapSet::new(&in_use.remaps, &in_use.leader).1,
        };
        // The preset's records, then the vimrc's, then the table's.
        let in_vimrc = self.preset_remaps.len();
        let first = in_vimrc + self.vimrc_remaps.len();
        self.vimrc_conflicts.clear();
        if let Some(keymap) = in_use_keymap {
            let conflicts =
                fe_gpui::remap_keymap_conflicts(&keymap, &in_use.remaps, &in_use.leader);
            for conflict in conflicts {
                let message = conflict_message(&conflict);
                if let (Some(&line), Some(preset)) = (self.preset_lines.get(conflict.index), preset)
                {
                    let path = Path::new(preset.name);
                    self.vimrc_conflicts
                        .push(at_line(path, Severity::Warning, line, message));
                } else if let Some(&line) =
                    (conflict.index.checked_sub(in_vimrc)).and_then(|at| self.vimrc_lines.get(at))
                    && conflict.index < first
                {
                    let path = self.vimrc.clone().unwrap_or_default();
                    self.vimrc_conflicts
                        .push(at_line(&path, Severity::Warning, line, message));
                } else if let Some(&entry) =
                    (conflict.index.checked_sub(first)).and_then(|at| entries.get(at))
                {
                    problems.push(Problem::Entry(entry, "from", message));
                }
            }
            self.keymap = Some(keymap);
        }
        for issue in issues {
            let Some(&entry) = (issue.index.checked_sub(first)).and_then(|at| entries.get(at))
            else {
                continue;
            };
            let side = failing_side(&in_use.remaps[issue.index], &in_use.leader);
            problems.push(Problem::Entry(entry, side, issue.message));
        }
        problems.sort();
        self.table_diagnostics = self.place(problems);
    }

    /// The group names: the preset's, the vimrc's, then the table's, each
    /// of whose `keys` is read with `leader`; one naming `<leader>` while
    /// `vim.leader` can't be read, or whose keys can't be read, is left
    /// out with a problem.
    fn groups(
        &self,
        readable: bool,
        leader: &str,
        problems: &mut Vec<Problem>,
    ) -> Vec<WhichKeyGroupName> {
        let mut groups = self.preset_groups.clone();
        groups.extend_from_slice(&self.vimrc_groups);
        for (entry, group) in self.group_table.iter().enumerate() {
            if !readable && group.keys.to_ascii_lowercase().contains("<leader>") {
                let message = tr!("fastedit-vim-leader-unreadable").to_string();
                problems.push(Problem::Group(entry, "keys", message));
                continue;
            }
            let modes = remap_modes(group.mode.modes());
            let (keys, name, icon) = (&group.keys, &group.name, group.icon.as_deref());
            match WhichKeyGroupName::parse(&modes, keys, name, leader) {
                Ok(named) => {
                    match WhichKeyGroupName::parse_with_icon(&modes, keys, name, icon, leader) {
                        Ok(with_icon) => groups.extend(with_icon),
                        Err(message) => {
                            problems.push(Problem::Group(entry, "icon", message));
                            groups.extend(named);
                        }
                    }
                }
                Err(message) => problems.push(Problem::Group(entry, "keys", message)),
            }
        }
        groups
    }

    /// A warning for each of `problems`, at its key in the last settings
    /// file whose accepted text sets that key.
    fn place(&self, problems: Vec<Problem>) -> Vec<Diagnostic> {
        if problems.is_empty() {
            return Vec::new();
        }
        let sources = &self.accepted;
        let documents: Vec<Option<Spanned<DeTable<'_>>>> = (sources.iter())
            .map(|source| DeTable::parse(source).ok())
            .collect();
        // The last file setting `vim.<key>`, or the user's.
        let origin = |key: &str| {
            (documents.iter())
                .rposition(|document| {
                    document
                        .as_ref()
                        .is_some_and(|document| vim_key(document.get_ref(), key).is_some())
                })
                .or((!documents.is_empty()).then_some(0))
        };
        // Where `at` finds the key in file `ix`, or its start.
        let offset = |ix: usize, at: &dyn Fn(&DeTable<'_>) -> usize| {
            documents[ix]
                .as_ref()
                .map_or(0, |document| at(document.get_ref()))
        };
        let mut diagnostics = Vec::new();
        for problem in problems {
            let (key, ix, offset, message) = match problem {
                Problem::Leader(message) => {
                    let Some(ix) = origin("leader") else { continue };
                    let at = offset(ix, &|table| vim_key(table, "leader").unwrap_or(0));
                    ("vim.leader".to_owned(), ix, at, message)
                }
                Problem::Entry(entry, side, message) => {
                    let Some(ix) = origin("remap") else { continue };
                    let at = offset(ix, &|table| entry_offset(table, "remap", entry, side));
                    (format!("vim.remap[{entry}].{side}"), ix, at, message)
                }
                Problem::Group(entry, side, message) => {
                    let Some(ix) = origin("which_key_group") else {
                        continue;
                    };
                    let at = offset(ix, &|table| {
                        entry_offset(table, "which_key_group", entry, side)
                    });
                    (
                        format!("vim.which_key_group[{entry}].{side}"),
                        ix,
                        at,
                        message,
                    )
                }
            };
            let (line, column) = line_column(&sources[ix], offset);
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                file: self.settings_files[ix].clone(),
                key: Some(key),
                line,
                column,
                message,
            });
        }
        diagnostics
    }
}

impl ConfigSource for VimRemaps {
    fn paths(&self) -> Vec<PathBuf> {
        (self.vimrc.iter().chain(&self.settings_files))
            .chain(&self.keymap_file)
            .cloned()
            .collect()
    }

    /// The vimrc's directory is created, so a vimrc written later is seen.
    fn user_files(&self) -> Vec<PathBuf> {
        self.vimrc.iter().cloned().collect()
    }

    fn install(&mut self, cx: &mut App) {
        self.read_vimrc();
        // The store's text is what it accepted, whatever the files hold.
        self.take_settings(cx);
        self.build(Some(cx));
    }

    fn reload(&mut self, changed: &[PathBuf], cx: &mut App) {
        let named = self.paths().iter().any(|path| changed.contains(path));
        // The settings' source reloads first, so these are the new ones.
        let table = self.take_settings(cx);
        // The keymap source reloads before this one, so this is the new one.
        let keymap = self
            .keymap
            .as_ref()
            .is_some_and(|last| *last != fe_gpui::current_keymap(cx));
        if !(named || table || keymap) {
            return;
        }
        if named {
            self.read_vimrc();
        }
        self.build(Some(cx));
    }

    /// A keymap swapped by `SharedKeymap::update` (extension bindings)
    /// recomputes the keymap conflict warnings, which are then published.
    fn keymap_changed(&mut self, cx: &mut App) {
        self.build(Some(cx));
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        (self.preset_diagnostics.iter())
            .chain(&self.vimrc_diagnostics)
            .chain(&self.table_diagnostics)
            .chain(&self.vimrc_conflicts)
            .cloned()
            .collect()
    }
}

/// The text of the file at `path`, empty if there's none. Only a regular
/// file of at most [`MAX_BYTES`] is read, opened without blocking (a FIFO
/// would block the open) and checked once open.
fn read(path: &Path) -> Result<String, String> {
    let cannot =
        |error: io::Error| tr!("fastedit-vim-read-failed", error = error.to_string()).to_string();
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => return Err(cannot(error)),
    };
    if !file.metadata().map_err(cannot)?.is_file() {
        return Err(tr!("fastedit-vim-read-not-regular").to_string());
    }
    let mut bytes = Vec::new();
    (file.take(MAX_BYTES + 1).read_to_end(&mut bytes)).map_err(cannot)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(tr!("fastedit-vim-read-too-big", kib = MAX_BYTES / 1024).to_string());
    }
    String::from_utf8(bytes).map_err(|_| tr!("fastedit-vim-read-not-utf8").to_string())
}

/// What `text`, in the vimrc subset, loads as when `<leader>` starts as
/// `leader`, readable vim notation: its keys, as `[[vim.remap]]` reads
/// them.
fn parse_with_leader(text: &str, leader: &str) -> Vimrc {
    parse_vimrc_with_leader(text, &parse_keys(leader).unwrap_or_default())
}

/// What a map the keymap hides says in the config problems list.
fn conflict_message(conflict: &fe_gpui::RemapConflict) -> String {
    let (from, keys, command) = (
        conflict.from.to_string(),
        conflict.keys.to_string(),
        conflict.command.to_string(),
    );
    match conflict.mode {
        RemapMode::CmdLine => tr!(
            "fastedit-vim-conflict-cmdline",
            from = from,
            keys = keys,
            command = command
        ),
        _ => tr!(
            "fastedit-vim-conflict-insert",
            from = from,
            keys = keys,
            command = command
        ),
    }
    .to_string()
}

/// A diagnostic for the file at `path`, at the start of `line`.
fn at_line(path: &Path, severity: Severity, line: usize, message: String) -> Diagnostic {
    Diagnostic {
        severity,
        file: path.to_owned(),
        key: None,
        line,
        column: 1,
        message,
    }
}

/// `v` means `x`; a mode listed twice counts once.
fn remap_modes(modes: &[VimRemapMode]) -> Vec<RemapMode> {
    let mut out = Vec::new();
    for mode in modes {
        let mode = match mode {
            VimRemapMode::Normal => RemapMode::Normal,
            VimRemapMode::Visual | VimRemapMode::VisualSelect => RemapMode::Visual,
            VimRemapMode::OperatorPending => RemapMode::OperatorPending,
            VimRemapMode::Insert => RemapMode::Insert,
            VimRemapMode::CmdLine => RemapMode::CmdLine,
        };
        if !out.contains(&mode) {
            out.push(mode);
        }
    }
    out
}

/// Which of `remap`'s keys names `<leader>` (any case), `from` first.
fn names_leader(remap: &VimRemap) -> Option<&'static str> {
    let names = |text: &str| text.to_ascii_lowercase().contains("<leader>");
    match (names(&remap.from), names(&remap.to)) {
        (true, _) => Some("from"),
        (false, true) => Some("to"),
        (false, false) => None,
    }
}

/// `from` if `record`'s left side alone can't load with `leader`, else
/// `to`.
fn failing_side(record: &Remap, leader: &str) -> &'static str {
    let left = Remap {
        to: String::new(),
        ..record.clone()
    };
    let (_, issues) = RemapSet::new(std::slice::from_ref(&left), leader);
    if issues.is_empty() { "to" } else { "from" }
}

/// Where the `vim.{array}` array's entry `entry` (`vim.remap`'s, or
/// `vim.which_key_group`'s) has its `side` key in the source, or where the
/// deepest of the entry, the array and `vim` found is.
fn entry_offset(table: &DeTable<'_>, array: &str, entry: usize, side: &str) -> usize {
    let Some((vim_at, vim)) = find(table, "vim") else {
        return 0;
    };
    let DeValue::Table(vim) = vim.get_ref() else {
        return vim_at;
    };
    let Some((remap_at, remap)) = find(vim, array) else {
        return vim_at;
    };
    let DeValue::Array(entries) = remap.get_ref() else {
        return remap_at;
    };
    let Some(found) = entries.get(entry) else {
        return remap_at;
    };
    let DeValue::Table(found_table) = found.get_ref() else {
        return found.span().start;
    };
    find(found_table, side).map_or(found.span().start, |(at, _)| at)
}

/// Where the source's `vim.<name>` key is, if it has one.
fn vim_key(table: &DeTable<'_>, name: &str) -> Option<usize> {
    let (_, vim) = find(table, "vim")?;
    let DeValue::Table(vim) = vim.get_ref() else {
        return None;
    };
    find(vim, name).map(|(at, _)| at)
}

/// Where `table`'s key `name` is, and its value.
fn find<'t, 'a>(table: &'t DeTable<'a>, name: &str) -> Option<(usize, &'t Spanned<DeValue<'a>>)> {
    (table.iter())
        .find(|(key, _)| key.get_ref() == name)
        .map(|(key, value)| (key.span().start, value))
}

/// The 1-based line and column (in chars) of byte `offset` in `source`.
fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &source[..offset];
    let line_start = before.rfind('\n').map_or(0, |at| at + 1);
    (
        before.matches('\n').count() + 1,
        before[line_start..].chars().count() + 1,
    )
}

#[cfg(test)]
#[path = "vim_lazyvim_groups_tests.rs"]
mod vim_lazyvim_groups_tests;
#[cfg(test)]
#[path = "vim_lazyvim_preset_tests.rs"]
mod vim_lazyvim_preset_tests;
#[cfg(test)]
#[path = "vim_preset_tests.rs"]
mod vim_preset_tests;

#[cfg(test)]
mod tests {
    use std::fs;

    use fe_vim::{Lookup, VimKey, parse_vimrc};
    use gpui::TestAppContext;
    use kiln::{
        Config,
        core::{KeymapLayer, KeymapStore},
        platform::config::{BatchSender, ConfigService, batch_channel},
    };
    use tempfile::TempDir;

    use super::*;

    /// A config directory holding `settings` and, if given, `vimrc`, and
    /// a project's settings file beside it.
    pub(super) struct Files {
        pub(super) dir: TempDir,
    }

    impl Files {
        pub(super) fn new(settings: &str, vimrc: Option<&str>) -> Self {
            let files = Self {
                dir: tempfile::tempdir().unwrap(),
            };
            fs::write(files.settings(), settings).unwrap();
            if let Some(vimrc) = vimrc {
                fs::write(files.vimrc(), vimrc).unwrap();
            }
            files
        }

        pub(super) fn settings(&self) -> PathBuf {
            self.dir.path().join("settings.toml")
        }

        pub(super) fn vimrc(&self) -> PathBuf {
            self.dir.path().join("vimrc")
        }

        pub(super) fn project(&self) -> PathBuf {
            self.dir.path().join("project.toml")
        }
    }

    /// `kiln::init` with the user's settings and the project's, and a
    /// [`VimRemaps`] source on `files`, calling `between` after the source
    /// is made and before init; the batches are sent by hand. Returns what
    /// init logged.
    fn start_with(
        files: &Files,
        between: impl FnOnce(),
        cx: &mut TestAppContext,
    ) -> (BatchSender, Vec<Diagnostic>) {
        let store = fe_config::SettingsStore::load(Some(files.settings()), Some(files.project()));
        let remaps = VimRemaps::new(Some(files.vimrc()), &store);
        between();
        let (sender, batches) = batch_channel();
        let config = Config::new("dev.fastedit.test", "fastedit")
            .settings_store(store.into_inner())
            .source(remaps)
            .watch_with(batches);
        let diagnostics = cx.update(|cx| kiln::init(cx, config));
        cx.run_until_parked();
        (sender, diagnostics)
    }

    fn start(files: &Files, cx: &mut TestAppContext) -> (BatchSender, Vec<Diagnostic>) {
        start_with(files, || {}, cx)
    }

    /// Writes `text` to `path` and sends a batch naming `batch`.
    pub(super) fn edit(
        path: &Path,
        text: &str,
        batch: &[PathBuf],
        sender: &BatchSender,
        cx: &mut TestAppContext,
    ) {
        fs::write(path, text).unwrap();
        sender.send(batch.to_vec());
        cx.run_until_parked();
    }

    /// What `from` maps to in normal mode in the set every editor has.
    pub(super) fn normal(from: &str, cx: &mut TestAppContext) -> Option<Vec<VimKey>> {
        cx.update(|cx| {
            let in_use = cx.global::<RemapsInUse>();
            let (set, _) = RemapSet::new(&in_use.remaps, &in_use.leader);
            match set.lookup(RemapMode::Normal, &parse_keys(from).unwrap()) {
                Lookup::Exact { to, .. } | Lookup::ExactAndPrefix { to, .. } => Some(to.to_vec()),
                Lookup::None | Lookup::Prefix => None,
            }
        })
    }

    pub(super) fn keys(text: &str) -> Option<Vec<VimKey>> {
        Some(parse_keys(text).unwrap())
    }

    /// The config problems list's diagnostics for `path`.
    pub(super) fn problems(path: &Path, cx: &mut TestAppContext) -> Vec<Diagnostic> {
        cx.update(|cx| {
            (kiln::platform::config::diagnostics(cx).iter())
                .filter(|diagnostic| diagnostic.file == path)
                .cloned()
                .collect()
        })
    }

    /// The warnings for `path`: this source's, not the settings store's
    /// errors.
    pub(super) fn warnings(path: &Path, cx: &mut TestAppContext) -> Vec<Diagnostic> {
        let mut found = problems(path, cx);
        found.retain(|diagnostic| diagnostic.severity == Severity::Warning);
        found
    }

    /// Each diagnostic's severity, key, line and column.
    fn places(diagnostics: &[Diagnostic]) -> Vec<(Severity, &str, usize, usize)> {
        (diagnostics.iter())
            .map(|d| (d.severity, d.key.as_deref().unwrap_or(""), d.line, d.column))
            .collect()
    }

    #[gpui::test]
    fn vimrc_and_table_merge(cx: &mut TestAppContext) {
        let settings = "[[vim.remap]]\nmode = \"n\"\nfrom = \"L\"\nto = \"g_\"\n\n\
                        [[vim.remap]]\nmode = \"n\"\nfrom = \"Q\"\nto = \"x\"\n";
        let files = Files::new(settings, Some("nnoremap H ^\nnnoremap L $\n"));
        let (sender, diagnostics) = start(&files, cx);
        assert_eq!(diagnostics, []);
        assert_eq!(normal("H", cx), keys("^"), "the vimrc's");
        assert_eq!(normal("L", cx), keys("g_"), "the table wins");
        assert_eq!(normal("Q", cx), keys("x"), "the table's");
        assert_eq!(problems(&files.settings(), cx), []);
        assert_eq!(problems(&files.vimrc(), cx), []);

        // A missing vimrc is no maps, and no problem.
        fs::remove_file(files.vimrc()).unwrap();
        sender.send(vec![files.vimrc()]);
        cx.run_until_parked();
        assert_eq!(normal("H", cx), None);
        assert_eq!(normal("L", cx), keys("g_"));
        assert_eq!(problems(&files.vimrc(), cx), []);
        assert_eq!(problems(&files.settings(), cx), []);
    }

    #[gpui::test]
    fn insert_and_cmdline_modes_load(cx: &mut TestAppContext) {
        let settings = "[[vim.remap]]\nmode = \"i\"\nfrom = \"jk\"\nto = \"<Esc>\"\n\n\
                        [[vim.remap]]\nmode = \"c\"\nfrom = \"kj\"\nto = \"<CR>\"\n";
        let files = Files::new(settings, Some("inoremap ab c\n"));
        let (_sender, diagnostics) = start(&files, cx);
        assert_eq!(diagnostics, []);
        let lookup = |mode: RemapMode, from: &str, cx: &mut TestAppContext| {
            cx.update(|cx| {
                let in_use = cx.global::<RemapsInUse>();
                let (set, _) = RemapSet::new(&in_use.remaps, &in_use.leader);
                match set.lookup(mode, &parse_keys(from).unwrap()) {
                    Lookup::Exact { to, .. } | Lookup::ExactAndPrefix { to, .. } => {
                        Some(to.to_vec())
                    }
                    Lookup::None | Lookup::Prefix => None,
                }
            })
        };
        assert_eq!(lookup(RemapMode::Insert, "jk", cx), keys("<Esc>"));
        assert_eq!(lookup(RemapMode::Insert, "ab", cx), keys("c"));
        assert_eq!(lookup(RemapMode::CmdLine, "kj", cx), keys("<CR>"));
        assert_eq!(lookup(RemapMode::Normal, "jk", cx), None);
        assert_eq!(lookup(RemapMode::Insert, "kj", cx), None);
    }

    #[gpui::test]
    fn problems_at_their_lines(cx: &mut TestAppContext) {
        let vimrc = "\" maps\nnnoremap H ^\ntmap jk <Esc>\nset number\nnnoremap <leader>v x\n";
        let settings = "[vim]\nleader = \",\"\n\n\
                        [[vim.remap]]\nmode = \"n\"\nfrom = \"a\"\nto = \"<Bogus>\"\n\n\
                        [[vim.remap]]\nmode = \"n\"\nfrom = \"<Nop>\"\nto = \"x\"\n\n\
                        [[vim.remap]]\nmode = \"n\"\nfrom = \"<leader>w\"\nto = \"x\"\n";
        let files = Files::new(settings, Some(vimrc));
        let (sender, logged) = start(&files, cx);

        let issues = parse_vimrc(vimrc).issues;
        assert_eq!(
            issues.iter().map(|issue| issue.line).collect::<Vec<_>>(),
            [3, 4]
        );
        let expected_vimrc: Vec<Diagnostic> = (issues.into_iter())
            .map(|issue| Diagnostic {
                severity: Severity::Warning,
                file: files.vimrc(),
                key: None,
                line: issue.line,
                column: 1,
                message: issue.message,
            })
            .collect();
        assert_eq!(problems(&files.vimrc(), cx), expected_vimrc);

        let table = problems(&files.settings(), cx);
        assert_eq!(
            places(&table),
            [
                (Severity::Warning, "vim.remap[0].to", 7, 1),
                (Severity::Warning, "vim.remap[1].from", 11, 1),
            ]
        );
        assert_eq!(normal(",w", cx), keys("x"), "the good entry loads");
        assert_eq!(normal("H", cx), keys("^"), "the good line loads");
        assert_eq!(normal("\\v", cx), keys("x"), "the vimrc's own leader");

        // init logged them all, as the service does later ones.
        let mut all = expected_vimrc;
        all.extend(table);
        assert_eq!(logged, all);

        // A leader that can't be read is a problem of its own, and leaves
        // out the entries naming it, but none of the vimrc's maps.
        let settings = "[vim]\nleader = \"<Bogus>\"\n\n\
                        [[vim.remap]]\nmode = \"n\"\nfrom = \"<leader>w\"\nto = \"x\"\n\n\
                        [[vim.remap]]\nmode = \"n\"\nfrom = \"Q\"\nto = \"x\"\n";
        edit(
            &files.settings(),
            settings,
            &[files.settings()],
            &sender,
            cx,
        );
        assert_eq!(
            places(&problems(&files.settings(), cx)),
            [
                (Severity::Warning, "vim.leader", 2, 1),
                (Severity::Warning, "vim.remap[0].from", 6, 1),
            ]
        );
        assert_eq!(normal("H", cx), keys("^"));
        assert_eq!(normal("\\v", cx), keys("x"));
        assert_eq!(normal("Q", cx), keys("x"));
    }

    #[gpui::test]
    fn a_tables_problems_are_in_its_file(cx: &mut TestAppContext) {
        // A project file's remaps are dropped, so the user's file holds them.
        let table = "[[vim.remap]]\nmode = \"n\"\nfrom = \"Q\"\nto = \"x\"\n\n\
                     [[vim.remap]]\nmode = \"n\"\nfrom = \"b\"\nto = \"<Bogus>\"\n";
        let files = Files::new(table, None);
        let (sender, _) = start(&files, cx);
        assert_eq!(normal("Q", cx), keys("x"));
        assert_eq!(problems(&files.project(), cx), []);
        assert_eq!(
            places(&problems(&files.settings(), cx)),
            [(Severity::Warning, "vim.remap[1].to", 9, 1)]
        );

        // An edit that only moves the entry moves its problem.
        let moved = format!("# remaps\n\n{table}");
        edit(&files.settings(), &moved, &[files.settings()], &sender, cx);
        assert_eq!(
            places(&problems(&files.settings(), cx)),
            [(Severity::Warning, "vim.remap[1].to", 11, 1)]
        );
    }

    #[gpui::test]
    fn rejected_layers_keep_their_places(cx: &mut TestAppContext) {
        let user = "[vim]\nleader = \"<Bogus>\"\n\n\
                    [[vim.remap]]\nmode = \"n\"\nfrom = \"a\"\nto = \"<Bogus>\"\n";
        let files = Files::new(user, None);
        // A string isn't a bool, so the store rejects the file.
        let project = |jump: &str| format!("[vim]\nleader = \"<Bogus>\"\njump_on_s = {jump}\n");
        fs::write(files.project(), project("\"i\"")).unwrap();
        let (sender, _) = start(&files, cx);
        assert!(
            problems(&files.project(), cx)
                .iter()
                .any(Diagnostic::is_error),
            "the store rejects the project's file"
        );
        let in_user = [
            (Severity::Warning, "vim.leader", 2, 1),
            (Severity::Warning, "vim.remap[0].to", 7, 1),
        ];
        assert_eq!(places(&warnings(&files.settings(), cx)), in_user);
        assert_eq!(warnings(&files.project(), cx), []);

        // Taken once good, its leader and table are the ones in use.
        edit(
            &files.project(),
            &project("true"),
            &[files.project()],
            &sender,
            cx,
        );
        // The user's table stays, so only its leader's warning moves.
        let user_table = [(Severity::Warning, "vim.remap[0].to", 7, 1)];
        assert_eq!(places(&warnings(&files.settings(), cx)), user_table);
        let in_project = [(Severity::Warning, "vim.leader", 2, 1)];
        assert_eq!(places(&warnings(&files.project(), cx)), in_project);

        // Rejected again, it keeps them, at their lines in the text taken.
        let broken = format!("not toml\n\n{}", project("true"));
        edit(&files.project(), &broken, &[files.project()], &sender, cx);
        assert_eq!(places(&warnings(&files.settings(), cx)), user_table);
        assert_eq!(places(&warnings(&files.project(), cx)), in_project);

        // A rejected user file keeps its place too, under the project's.
        edit(
            &files.settings(),
            "[vim\n",
            &[files.settings()],
            &sender,
            cx,
        );
        edit(&files.project(), "", &[files.project()], &sender, cx);
        assert_eq!(places(&warnings(&files.settings(), cx)), in_user);
        assert_eq!(warnings(&files.project(), cx), []);
    }

    /// A keymap layer binding `keys` to a command in the editor's insert mode.
    pub(super) fn insert_layer(keys: &str) -> Vec<kiln::core::KeymapLayer> {
        let source = format!(
            "[[bindings]]\nkeys = \"{keys}\"\ncommand = \"editor.undo\"\n\
             context = \"editor && vim.mode == insert\"\n"
        );
        let (layer, issues) = KeymapLayer::parse(Path::new("ext.toml"), &source, None);
        assert_eq!(issues, []);
        vec![layer.unwrap()]
    }

    #[gpui::test]
    fn keymap_update_refreshes_conflict_warnings(cx: &mut TestAppContext) {
        let files = Files::new("", Some("inoremap jk <Esc>\n"));
        let store = fe_config::SettingsStore::load(Some(files.settings()), Some(files.project()));
        let remaps = VimRemaps::new(Some(files.vimrc()), &store);
        let keymap = KeymapStore::with_defaults(Vec::new(), Vec::new(), None, None);
        let (_sender, batches) = batch_channel();
        let service = ConfigService::new().keymap(keymap).source(remaps);
        let shared = service.keymap_store().unwrap();
        cx.update(|cx| drop(service.start_with(batches, cx)));
        cx.run_until_parked();
        assert_eq!(warnings(&files.vimrc(), cx), []);

        // An update that binds the left side adds the warning, unreloaded.
        cx.update(|cx| {
            shared.update(cx, |store| {
                store.set_defaults(insert_layer("j k"), Vec::new(), None)
            })
        });
        let found = warnings(&files.vimrc(), cx);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].line, 1);

        // And one that removes it clears the warning.
        cx.update(|cx| shared.update(cx, |store| store.set_defaults(Vec::new(), Vec::new(), None)));
        assert_eq!(warnings(&files.vimrc(), cx), []);
    }

    #[gpui::test]
    fn keymap_update_in_a_batch_refreshes_earlier_sources(cx: &mut TestAppContext) {
        let files = Files::new("", Some("inoremap jk <Esc>\n"));
        let store = fe_config::SettingsStore::load(Some(files.settings()), Some(files.project()));
        let remaps = VimRemaps::new(Some(files.vimrc()), &store);
        let keymap = KeymapStore::with_defaults(Vec::new(), Vec::new(), None, None);
        let (sender, batches) = batch_channel();
        let service = ConfigService::new().keymap(keymap).source(remaps);
        let shared = service.keymap_store().unwrap();
        // A source after the remaps, whose batch changes the keymap.
        let trigger = files.dir.path().join("trigger");
        let bound = std::rc::Rc::new(std::cell::Cell::new(false));
        let service = service.on_change(vec![trigger.clone()], move |_, cx| {
            bound.set(!bound.get());
            let layers = if bound.get() {
                insert_layer("j k")
            } else {
                Vec::new()
            };
            shared.update(cx, |store| store.set_defaults(layers, Vec::new(), None));
        });
        let _task = cx.update(|cx| service.start_with(batches, cx));
        cx.run_until_parked();
        assert_eq!(warnings(&files.vimrc(), cx), []);

        sender.send(vec![trigger.clone()]);
        cx.run_until_parked();
        assert_eq!(warnings(&files.vimrc(), cx).len(), 1);

        sender.send(vec![trigger]);
        cx.run_until_parked();
        assert_eq!(warnings(&files.vimrc(), cx), []);
    }

    #[gpui::test]
    fn reload_keeps_last_good(cx: &mut TestAppContext) {
        let files = Files::new("", Some("nnoremap H ^\n"));
        let (sender, _) = start(&files, cx);
        assert_eq!(normal("H", cx), keys("^"));

        edit(
            &files.vimrc(),
            "nnoremap H $\nbogus\n",
            &[files.vimrc()],
            &sender,
            cx,
        );
        assert_eq!(normal("H", cx), keys("$"));
        let found = problems(&files.vimrc(), cx);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!((found[0].severity, found[0].line), (Severity::Warning, 2));

        // The table reloads with the settings, over the vimrc.
        let settings = "[[vim.remap]]\nmode = \"n\"\nfrom = \"H\"\nto = \"0\"\n";
        edit(
            &files.settings(),
            settings,
            &[files.settings()],
            &sender,
            cx,
        );
        assert_eq!(normal("H", cx), keys("0"));

        // A vimrc that can't be read keeps the maps last read.
        fs::write(files.settings(), "").unwrap();
        fs::remove_file(files.vimrc()).unwrap();
        fs::create_dir(files.vimrc()).unwrap();
        sender.send(vec![files.vimrc(), files.settings()]);
        cx.run_until_parked();
        assert_eq!(normal("H", cx), keys("$"));
        let found = problems(&files.vimrc(), cx);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            (found[0].severity, found[0].line, found[0].column),
            (Severity::Error, 1, 1)
        );

        // So does one too big to read.
        fs::remove_dir(files.vimrc()).unwrap();
        let big = "\" padding\n".repeat(MAX_BYTES as usize / 10 + 1);
        edit(&files.vimrc(), &big, &[files.vimrc()], &sender, cx);
        assert_eq!(normal("H", cx), keys("$"));
        let found = problems(&files.vimrc(), cx);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!((found[0].severity, found[0].line), (Severity::Error, 1));

        // Deleted, it's empty.
        fs::remove_file(files.vimrc()).unwrap();
        sender.send(vec![files.vimrc()]);
        cx.run_until_parked();
        assert_eq!(normal("H", cx), None);
        assert_eq!(problems(&files.vimrc(), cx), []);
    }

    #[gpui::test]
    fn install_and_settings_batches_read_the_vimrc(cx: &mut TestAppContext) {
        let files = Files::new("", Some("nnoremap H ^\n"));
        // Changed after the source was made, before the service started.
        let (sender, _) = start_with(
            &files,
            || fs::write(files.vimrc(), "nnoremap H $\n").unwrap(),
            cx,
        );
        assert_eq!(normal("H", cx), keys("$"));

        // A batch naming only the settings reads it again too.
        fs::write(files.vimrc(), "nnoremap H 0\n").unwrap();
        edit(&files.settings(), "", &[files.settings()], &sender, cx);
        assert_eq!(normal("H", cx), keys("0"));
    }

    const ACCEPTED_PROJECT: &str = "# project\n\n[vim]\nleader = \"<Bogus>\"\n";
    const ACCEPTED_USER: &str = "# user\n\n\
                           [[vim.remap]]\nmode = \"n\"\nfrom = \"b\"\nto = \"<Bogus>\"\n\n\
                           [[vim.remap]]\nmode = \"n\"\nfrom = \"<leader>w\"\nto = \"y\"\n";

    struct AcceptedFiles {
        dir: TempDir,
    }

    impl AcceptedFiles {
        fn new() -> Self {
            let files = Self {
                dir: tempfile::tempdir().unwrap(),
            };
            fs::write(files.user(), ACCEPTED_USER).unwrap();
            fs::write(files.project(), ACCEPTED_PROJECT).unwrap();
            files
        }

        fn user(&self) -> PathBuf {
            self.dir.path().join("settings.toml")
        }

        fn project(&self) -> PathBuf {
            self.dir.path().join("project.toml")
        }

        fn store(&self) -> fe_config::SettingsStore {
            fe_config::SettingsStore::load(Some(self.user()), Some(self.project()))
        }
    }

    fn accepted_places(diagnostics: &[Diagnostic]) -> Vec<(PathBuf, &str, usize)> {
        (diagnostics.iter())
            .map(|d| {
                assert_eq!(d.severity, Severity::Warning);
                (d.file.clone(), d.key.as_deref().unwrap_or(""), d.line)
            })
            .collect()
    }

    fn accepted_expected(files: &AcceptedFiles) -> Vec<(PathBuf, &'static str, usize)> {
        vec![
            (files.project(), "vim.leader", 4),
            (files.user(), "vim.remap[0].to", 6),
            (files.user(), "vim.remap[1].from", 10),
        ]
    }

    #[test]
    fn a_project_file_changing_between_load_and_construction_keeps_its_warning_lines() {
        let files = AcceptedFiles::new();
        let store = files.store();
        fs::write(files.user(), "not toml\n").unwrap();
        let remaps = VimRemaps::new(None, &store);
        assert_eq!(
            accepted_places(&remaps.table_diagnostics),
            accepted_expected(&files)
        );
    }

    #[test]
    fn construction_after_a_rejected_reload_keeps_the_accepted_lines() {
        let files = AcceptedFiles::new();
        let mut store = files.store();
        fs::write(files.user(), "not toml\n").unwrap();
        assert!(store.reload(&[files.user()]).is_some());
        let remaps = VimRemaps::new(None, &store);
        assert_eq!(
            accepted_places(&remaps.table_diagnostics),
            accepted_expected(&files)
        );
    }

    #[test]
    fn the_remaps_and_the_leader_keep_their_lines_when_the_file_moves() {
        let files = AcceptedFiles::new();
        let store = files.store();
        fs::write(files.user(), format!("# moved\n\n{ACCEPTED_USER}")).unwrap();
        let remaps = VimRemaps::new(None, &store);
        let lines: Vec<_> = (remaps.table_diagnostics.iter())
            .map(|d| (d.key.clone().unwrap(), d.line))
            .collect();
        assert_eq!(
            lines,
            [
                ("vim.leader".to_owned(), 4),
                ("vim.remap[0].to".to_owned(), 6),
                ("vim.remap[1].from".to_owned(), 10),
            ]
        );
    }
}
