//! The user's vim remaps as a source of kiln's config service: the vimrc
//! (`~/.config/fastedit/vimrc`, the subset [`fe_vim::parse_vimrc`] reads)
//! and the settings' `[[vim.remap]]` table with `vim.leader`, built into
//! one set for every editor and reloaded when any of their files changes.
//! Their problems go to the config problems list, each at its line: see
//! `docs/decisions/vim-remaps.md` §1(c) and §7.
//!
//! Known limit: the settings store keeps each layer's last good table but
//! not its text, so this source reads the settings files itself to place
//! the table's and the leader's problems, taking a file's text only when
//! the store's diagnostics show no error for it. If a project settings file
//! changes between `SettingsStore::load` and [`VimRemaps::new`] (or the
//! first watcher batch rejects it), what this source took can differ from
//! what the store holds: the project's remap and leader warnings can then
//! fall back to the user's file, or sit at the wrong line. Which maps apply,
//! and the last good maps on a bad reload, are unaffected. Placing them
//! exactly needs the store to keep each layer's accepted text or spans and
//! hand that snapshot here, which T225 does.

use std::{
    fs::OpenOptions,
    io::{self, Read as _},
    path::{Path, PathBuf},
};

use fe_config::{Diagnostic, Settings, SettingsStore, Severity, VimRemap, VimRemapMode};
use fe_vim::{Remap, RemapMode, RemapSet, parse_keys, parse_vimrc};
use gpui::{App, Global};
use kiln::{GlobalSettings, platform::config::ConfigSource};
use toml::{
    Spanned,
    de::{DeTable, DeValue},
};

/// Files bigger than this aren't read, as kiln reads no config file
/// bigger.
const MAX_BYTES: u64 = 1 << 20;

/// The records every editor's set was last built from, and the leader.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RemapsInUse {
    pub(crate) remaps: Vec<Remap>,
    pub(crate) leader: String,
}

impl Global for RemapsInUse {}

/// The vimrc's maps first, then the settings' `vim.remap` entries, so that
/// for the same mode and keys the table wins.
pub(crate) struct VimRemaps {
    vimrc: Option<PathBuf>,
    /// The settings files, the user's first: a later one's `vim.remap` or
    /// `vim.leader` replaces an earlier one's, so its problems are in it.
    settings_files: Vec<PathBuf>,
    /// Each settings file's text as the settings store last took it: a
    /// file with errors keeps its last good table, empty until it has
    /// loaded once, so it keeps that text here.
    accepted: Vec<String>,
    /// The vimrc's maps: as last read, or as last read well while it
    /// can't be read.
    vimrc_remaps: Vec<Remap>,
    /// A warning for each of the vimrc's problems, or the one error
    /// reading it.
    vimrc_diagnostics: Vec<Diagnostic>,
    /// The settings' `vim.remap` and `vim.leader`, as last seen.
    table: Vec<VimRemap>,
    leader: String,
    /// A warning for each `vim.remap` entry that can't load, and for a
    /// `vim.leader` that can't be read, at its line.
    table_diagnostics: Vec<Diagnostic>,
}

/// A problem with the settings' remaps, before it's found in a file.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Problem {
    Leader(String),
    /// The entry of `vim.remap`, and which of its keys the problem is in.
    Entry(usize, &'static str, String),
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
            accepted: vec![String::new(); settings_files.len()],
            settings_files,
            vimrc_remaps: Vec::new(),
            vimrc_diagnostics: Vec::new(),
            table: vim.remap.clone(),
            leader: vim.leader.clone(),
            table_diagnostics: Vec::new(),
        };
        remaps.accept(None, settings.diagnostics());
        remaps.read_vimrc();
        remaps.build(None);
        remaps
    }

    /// Keeps the text of each settings file in `changed` (every one if
    /// `None`) that the store took, as `diagnostics` (the store's) say:
    /// those with no error.
    fn accept(&mut self, changed: Option<&[PathBuf]>, diagnostics: &[Diagnostic]) {
        for (path, accepted) in self.settings_files.iter().zip(&mut self.accepted) {
            let named = changed.is_none_or(|changed| changed.contains(path));
            let rejected = (diagnostics.iter()).any(|d| d.is_error() && d.file == *path);
            if named && !rejected {
                *accepted = read(path).unwrap_or_default();
            }
        }
    }

    /// Reads the vimrc, a missing one being empty. One that can't be read
    /// keeps the maps last read, with one error at its first line.
    fn read_vimrc(&mut self) {
        let Some(path) = &self.vimrc else {
            return;
        };
        match read(path) {
            Ok(text) => {
                let vimrc = parse_vimrc(&text);
                self.vimrc_remaps = vimrc.remaps;
                self.vimrc_diagnostics = (vimrc.issues.into_iter())
                    .map(|issue| at_line(path, Severity::Warning, issue.line, issue.message))
                    .collect();
            }
            Err(message) => {
                self.vimrc_diagnostics = vec![at_line(path, Severity::Error, 1, message)];
            }
        }
    }

    /// Takes `vim.remap` and `vim.leader` from the settings in use,
    /// returning whether they changed, and the text of each file in
    /// `changed` the store took.
    fn take_settings(&mut self, changed: &[PathBuf], cx: &App) -> bool {
        let Some(global) = cx.try_global::<GlobalSettings<Settings>>() else {
            return false;
        };
        self.accept(Some(changed), global.diagnostics());
        let vim = &global.settings().vim;
        if vim.remap == self.table && vim.leader == self.leader {
            return false;
        }
        self.table = vim.remap.clone();
        self.leader = vim.leader.clone();
        true
    }

    /// Builds the set: the vimrc's records, then the table's. With `cx`,
    /// gives it to every editor unless it's the one they have. Then finds
    /// the settings' problems in their files.
    ///
    /// The vimrc's records were checked as it was read, its `<leader>`s
    /// put in, so they load whatever `vim.leader` is. A `vim.leader` that
    /// can't be read is one problem, and the entries naming `<leader>` are
    /// left out, each with its own.
    fn build(&mut self, cx: Option<&mut App>) {
        let mut problems = Vec::new();
        let readable = match parse_keys(&self.leader) {
            Ok(_) => true,
            Err(error) => {
                problems.push(Problem::Leader(error.to_string()));
                false
            }
        };
        // Any leader will do for records that name none.
        let leader = if readable {
            self.leader.clone()
        } else {
            "\\".to_owned()
        };
        let mut remaps = self.vimrc_remaps.clone();
        // The entry each of the table's records is, in order.
        let mut entries = Vec::new();
        for (entry, remap) in self.table.iter().enumerate() {
            if !readable && let Some(side) = names_leader(remap) {
                let message = "vim.leader can't be read".to_owned();
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
        let in_use = RemapsInUse { remaps, leader };
        let issues = match cx {
            Some(cx) if cx.try_global::<RemapsInUse>() != Some(&in_use) => {
                let issues = fe_gpui::set_vim_remaps(cx, &in_use.remaps, &in_use.leader);
                cx.set_global(in_use.clone());
                issues
            }
            _ => RemapSet::new(&in_use.remaps, &in_use.leader).1,
        };
        let first = self.vimrc_remaps.len();
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
                    let at = offset(ix, &|table| entry_offset(table, entry, side));
                    (format!("vim.remap[{entry}].{side}"), ix, at, message)
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
            .cloned()
            .collect()
    }

    /// The vimrc's directory is created, so a vimrc written later is seen.
    fn user_files(&self) -> Vec<PathBuf> {
        self.vimrc.iter().cloned().collect()
    }

    fn install(&mut self, cx: &mut App) {
        self.read_vimrc();
        // The store hasn't read its files since this source was made.
        self.take_settings(&[], cx);
        self.build(Some(cx));
    }

    fn reload(&mut self, changed: &[PathBuf], cx: &mut App) {
        let named = self.paths().iter().any(|path| changed.contains(path));
        // The settings' source reloads first, so these are the new ones.
        let table = self.take_settings(changed, cx);
        if !(named || table) {
            return;
        }
        if named {
            self.read_vimrc();
        }
        self.build(Some(cx));
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        (self.vimrc_diagnostics.iter())
            .chain(&self.table_diagnostics)
            .cloned()
            .collect()
    }
}

/// The text of the file at `path`, empty if there's none. Only a regular
/// file of at most [`MAX_BYTES`] is read, opened without blocking (a FIFO
/// would block the open) and checked once open.
fn read(path: &Path) -> Result<String, String> {
    let cannot = |error: io::Error| format!("cannot read the file: {error}");
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
        return Err("cannot read the file: not a regular file".to_owned());
    }
    let mut bytes = Vec::new();
    (file.take(MAX_BYTES + 1).read_to_end(&mut bytes)).map_err(cannot)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!(
            "cannot read the file: bigger than {} KiB",
            MAX_BYTES / 1024
        ));
    }
    String::from_utf8(bytes).map_err(|_| "cannot read the file: it isn't valid UTF-8".to_owned())
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

/// Where `vim.remap`'s entry `entry` has its `side` key in the source, or
/// where the deepest of the entry, the array and `vim` found is.
fn entry_offset(table: &DeTable<'_>, entry: usize, side: &str) -> usize {
    let Some((vim_at, vim)) = find(table, "vim") else {
        return 0;
    };
    let DeValue::Table(vim) = vim.get_ref() else {
        return vim_at;
    };
    let Some((remap_at, remap)) = find(vim, "remap") else {
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
mod tests {
    use std::fs;

    use fe_vim::{Lookup, VimKey};
    use gpui::TestAppContext;
    use kiln::{
        Config,
        platform::config::{BatchSender, batch_channel},
    };
    use tempfile::TempDir;

    use super::*;

    /// A config directory holding `settings` and, if given, `vimrc`, and
    /// a project's settings file beside it.
    struct Files {
        dir: TempDir,
    }

    impl Files {
        fn new(settings: &str, vimrc: Option<&str>) -> Self {
            let files = Self {
                dir: tempfile::tempdir().unwrap(),
            };
            fs::write(files.settings(), settings).unwrap();
            if let Some(vimrc) = vimrc {
                fs::write(files.vimrc(), vimrc).unwrap();
            }
            files
        }

        fn settings(&self) -> PathBuf {
            self.dir.path().join("settings.toml")
        }

        fn vimrc(&self) -> PathBuf {
            self.dir.path().join("vimrc")
        }

        fn project(&self) -> PathBuf {
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
    fn edit(
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
    fn normal(from: &str, cx: &mut TestAppContext) -> Option<Vec<VimKey>> {
        cx.update(|cx| {
            let in_use = cx.global::<RemapsInUse>();
            let (set, _) = RemapSet::new(&in_use.remaps, &in_use.leader);
            match set.lookup(RemapMode::Normal, &parse_keys(from).unwrap()) {
                Lookup::Exact { to, .. } | Lookup::ExactAndPrefix { to, .. } => Some(to.to_vec()),
                Lookup::None | Lookup::Prefix => None,
            }
        })
    }

    fn keys(text: &str) -> Option<Vec<VimKey>> {
        Some(parse_keys(text).unwrap())
    }

    /// The config problems list's diagnostics for `path`.
    fn problems(path: &Path, cx: &mut TestAppContext) -> Vec<Diagnostic> {
        cx.update(|cx| {
            (kiln::platform::config::diagnostics(cx).iter())
                .filter(|diagnostic| diagnostic.file == path)
                .cloned()
                .collect()
        })
    }

    /// The warnings for `path`: this source's, not the settings store's
    /// errors.
    fn warnings(path: &Path, cx: &mut TestAppContext) -> Vec<Diagnostic> {
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
    fn problems_at_their_lines(cx: &mut TestAppContext) {
        let vimrc = "\" maps\nnnoremap H ^\ninoremap jk <Esc>\nset number\nnnoremap <leader>v x\n";
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
    fn a_project_tables_problems_are_in_its_file(cx: &mut TestAppContext) {
        let files = Files::new(
            "[[vim.remap]]\nmode = \"n\"\nfrom = \"a\"\nto = \"<Bogus>\"\n",
            None,
        );
        let project = "[[vim.remap]]\nmode = \"n\"\nfrom = \"Q\"\nto = \"x\"\n\n\
                       [[vim.remap]]\nmode = \"n\"\nfrom = \"b\"\nto = \"<Bogus>\"\n";
        fs::write(files.project(), project).unwrap();
        let (sender, _) = start(&files, cx);
        assert_eq!(
            normal("Q", cx),
            keys("x"),
            "the project's table replaces the user's"
        );
        assert_eq!(problems(&files.settings(), cx), []);
        assert_eq!(
            places(&problems(&files.project(), cx)),
            [(Severity::Warning, "vim.remap[1].to", 9, 1)]
        );

        // An edit that only moves the entry moves its problem.
        let moved = format!("# remaps\n\n{project}");
        edit(&files.project(), &moved, &[files.project()], &sender, cx);
        assert_eq!(
            places(&problems(&files.project(), cx)),
            [(Severity::Warning, "vim.remap[1].to", 11, 1)]
        );
    }

    #[gpui::test]
    fn rejected_layers_keep_their_places(cx: &mut TestAppContext) {
        let user = "[vim]\nleader = \"<Bogus>\"\n\n\
                    [[vim.remap]]\nmode = \"n\"\nfrom = \"a\"\nto = \"<Bogus>\"\n";
        let files = Files::new(user, None);
        // `i` isn't a mode a remap may have, so the store rejects the file.
        let project = |mode: &str| {
            format!(
                "[vim]\nleader = \"<Bogus>\"\n\n\
                 [[vim.remap]]\nmode = \"{mode}\"\nfrom = \"b\"\nto = \"<Bogus>\"\n"
            )
        };
        fs::write(files.project(), project("i")).unwrap();
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
            &project("n"),
            &[files.project()],
            &sender,
            cx,
        );
        assert_eq!(warnings(&files.settings(), cx), []);
        let in_project = in_user;
        assert_eq!(places(&warnings(&files.project(), cx)), in_project);

        // Rejected again, it keeps them, at their lines in the text taken.
        let broken = format!("not toml\n\n{}", project("n"));
        edit(&files.project(), &broken, &[files.project()], &sender, cx);
        assert_eq!(warnings(&files.settings(), cx), []);
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
}
