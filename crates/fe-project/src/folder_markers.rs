//! The build files in a folder, for the project tree's module and
//! source-root icons, found without scanning the project.
//!
//! A folder's [`Markers`] come from a [`MarkerProbe`], run off the UI
//! thread: the folder opened from `/` a component at a time without
//! following a symlink, its names read once through that handle (no stat per
//! entry; at most [`MAX_STATS`] metadata calls in all, for entries whose kind
//! the file system doesn't report and a `Cargo.toml`'s), and a `Cargo.toml`,
//! if there is one, read and parsed for a `workspace` table. Nothing below
//! the folder is looked at. [`MarkerCache`] holds each folder's result by
//! path, so a folder is probed once however many frames draw it, until a
//! watcher batch names the folder, or an entry of it whose name could change
//! its markers ([`MarkerCache::apply`]).
//!
//! Until its probe lands, a listed folder has the markers of the entries the
//! tree holds ([`Markers::of_listing`], no IO). The probe's win once it
//! lands: they count the entries the tree hides too (ignored ones), so a
//! folder's role doesn't change as it's expanded or ignored entries shown.

use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    io::{self, Read as _},
    path::{Path, PathBuf},
};

use crate::{
    EntryKind, FileTree, FsChange, NodeId,
    listing::{self, Access},
};

/// The most metadata calls a probe makes in a folder.
pub const MAX_STATS: usize = 10;

/// The most bytes of a `Cargo.toml` a probe reads: a bigger one isn't
/// taken for a workspace.
pub const MAX_READ: u64 = 1024 * 1024;

/// Cargo's manifest, a file name.
const CARGO_TOML: &str = kiln_l10n::verbatim!("Cargo.toml");

/// The marker files with a fixed name.
const MARKER_NAMES: [&str; 9] = [
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "pom.xml",
    CARGO_TOML,
    "package.json",
    "go.mod",
    "pyproject.toml",
];

/// A .NET project's extensions.
const DOTNET_PROJECTS: [&str; 3] = ["csproj", "fsproj", "vbproj"];

/// A .NET solution's extensions.
const DOTNET_SOLUTIONS: [&str; 2] = ["sln", "slnx"];

/// The folders of Maven's and Gradle's layout under `src`.
const LAYOUT_DIRS: [&str; 2] = ["main", "test"];

/// The build files in a folder, by the build tool they name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Markers {
    /// `build.gradle` or `build.gradle.kts`.
    pub gradle: bool,
    /// `settings.gradle` or `settings.gradle.kts`.
    pub gradle_settings: bool,
    /// `pom.xml`.
    pub maven: bool,
    /// A `*.csproj`, `*.fsproj` or `*.vbproj`.
    pub dotnet: bool,
    /// A `*.sln` or `*.slnx`.
    pub dotnet_solution: bool,
    /// `Cargo.toml`.
    pub cargo: bool,
    /// A `Cargo.toml` with a `workspace` table.
    pub cargo_workspace: bool,
    /// `package.json`.
    pub npm: bool,
    /// `go.mod`.
    pub go: bool,
    /// `pyproject.toml`.
    pub python: bool,
    /// A `main` or `test` folder.
    pub main_or_test: bool,
}

/// `name`'s extension, if it has a stem too, as one of `exts`, ASCII
/// case-insensitively.
fn has_ext(name: &str, exts: &[&str]) -> bool {
    name.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty() && exts.iter().any(|e| ext.eq_ignore_ascii_case(e))
    })
}

/// Whether an entry called `name` can count for its folder's markers:
/// ASCII case-insensitively, so as to err towards probing again on a volume
/// that ignores case.
fn matters(name: &str) -> bool {
    let named = |names: &[&str]| names.iter().any(|n| name.eq_ignore_ascii_case(n));
    named(&MARKER_NAMES)
        || named(&LAYOUT_DIRS)
        || has_ext(name, &DOTNET_PROJECTS)
        || has_ext(name, &DOTNET_SOLUTIONS)
}

impl Markers {
    /// Notes the entry `name`, a folder if `dir`; a file or a link counts
    /// as a marker file.
    fn note(&mut self, name: &str, dir: bool) {
        if dir {
            self.main_or_test |= LAYOUT_DIRS.contains(&name);
            return;
        }
        match name {
            "build.gradle" | "build.gradle.kts" => self.gradle = true,
            "settings.gradle" | "settings.gradle.kts" => self.gradle_settings = true,
            "pom.xml" => self.maven = true,
            CARGO_TOML => self.cargo = true,
            "package.json" => self.npm = true,
            "go.mod" => self.go = true,
            "pyproject.toml" => self.python = true,
            _ => {
                self.dotnet |= has_ext(name, &DOTNET_PROJECTS);
                self.dotnet_solution |= has_ext(name, &DOTNET_SOLUTIONS);
            }
        }
    }

    /// A folder's markers from its entries, each a name and whether it's a
    /// folder. Nothing says whether a `Cargo.toml` has a workspace.
    pub fn of_names<'a>(entries: impl IntoIterator<Item = (&'a OsStr, bool)>) -> Markers {
        let mut markers = Markers::default();
        for (name, dir) in entries {
            if let Some(name) = name.to_str() {
                markers.note(name, dir);
            }
        }
        markers
    }

    /// Folder `dir`'s markers from the entries the tree has listed, with no
    /// IO; `None` if it isn't listed. Entries the tree hides (ignored ones,
    /// unless it shows them) don't count, and nothing says whether a
    /// `Cargo.toml` has a workspace: a stand-in until a probe lands.
    pub fn of_listing(tree: &FileTree, dir: NodeId) -> Option<Markers> {
        let children = tree.children(dir)?;
        let entries = (children.iter())
            .filter_map(|&id| tree.row(id))
            .map(|row| (row.name, row.kind == EntryKind::Dir));
        Some(Markers::of_names(entries))
    }
}

/// The IO that finds a folder's [`Markers`], to run on any thread.
#[derive(Debug)]
pub struct MarkerProbe {
    dir: PathBuf,
    serial: u64,
}

/// A [`MarkerProbe`]'s result, for [`MarkerCache::store`].
#[derive(Debug)]
pub struct Probed {
    dir: PathBuf,
    serial: u64,
    markers: Markers,
}

impl MarkerProbe {
    /// The folder it probes.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Opens the folder from `/` without following a symlink and reads its
    /// names through the handle. An entry whose kind the file system doesn't
    /// report, and whose name matters, is looked up by the handle, while
    /// the budget lasts (one metadata call is kept for a `Cargo.toml`); past
    /// it, it counts as a file. A `Cargo.toml` is opened without following a
    /// symlink or waiting on a FIFO, and read, at most [`MAX_READ`] bytes of
    /// a regular file, for a `workspace` table. A folder that can't be
    /// reached so (gone, or a symlink now) has no markers.
    pub fn run(self) -> Probed {
        let markers = probe(&self.dir).unwrap_or_default();
        Probed {
            dir: self.dir,
            serial: self.serial,
            markers,
        }
    }
}

fn probe(dir: &Path) -> io::Result<Markers> {
    record(Call::Open, dir);
    let handle = listing::open_dirs(dir, Access::Read)
        .map_err(|unreached| unreached.err)?
        .pop()
        .expect("`/` is always opened");
    record(Call::ReadDir, dir);
    let mut markers = Markers::default();
    let mut stats = 0;
    for (name, kind) in listing::read_dir(&handle)? {
        let Some(text) = name.to_str().filter(|text| matters(text)) else {
            continue;
        };
        let kind = match kind {
            Some(kind) => kind,
            None if stats < MAX_STATS - 1 => {
                stats += 1;
                record(Call::Stat, &dir.join(&name));
                match listing::stat_at(&handle, &name) {
                    Ok(stat) => stat.kind,
                    Err(_) => continue,
                }
            }
            None => EntryKind::File,
        };
        markers.note(text, kind == EntryKind::Dir);
    }
    if markers.cargo {
        markers.cargo_workspace = has_workspace(dir).unwrap_or(false);
    }
    Ok(markers)
}

/// Whether `dir`'s `Cargo.toml`, a regular file of at most [`MAX_READ`]
/// bytes, is TOML with a top-level `workspace` table (however it's spelled:
/// `[workspace]`, `[ "workspace" ]`, `workspace.members = …`).
fn has_workspace(dir: &Path) -> io::Result<bool> {
    let path = dir.join(CARGO_TOML);
    record(Call::Open, &path);
    let file = crate::open_file_under(dir, Path::new(CARGO_TOML))?;
    record(Call::Stat, &path);
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > MAX_READ {
        return Ok(false);
    }
    let mut bytes = Vec::new();
    file.take(MAX_READ + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_READ {
        return Ok(false);
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Ok(false);
    };
    Ok(text
        .parse::<toml::Table>()
        .is_ok_and(|table| table.contains_key("workspace")))
}

/// A probe's file system call, recorded in tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Call {
    /// Opening a folder (from `/`, without following a symlink) or a file.
    Open,
    /// Reading an open folder's names.
    ReadDir,
    /// A metadata call: an entry looked up in its folder, or an open
    /// file's.
    Stat,
}

#[cfg(test)]
thread_local! {
    /// Every probe's IO on this thread.
    static CALLS: std::cell::RefCell<Vec<(Call, PathBuf)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn record(_call: Call, _path: &Path) {
    #[cfg(test)]
    CALLS.with(|calls| calls.borrow_mut().push((_call, _path.to_owned())));
}

/// The probe IO made on this thread since the last call.
#[cfg(test)]
pub(crate) fn take_calls() -> Vec<(Call, PathBuf)> {
    CALLS.with(|calls| calls.take())
}

/// A folder's probe: running, or its result.
#[derive(Debug)]
struct Slot {
    serial: u64,
    markers: Option<Markers>,
}

/// Each folder's probed [`Markers`], by absolute path, as the tree's
/// [`FileTree::path`] spells it (watcher batches spell paths so too).
#[derive(Debug, Default)]
pub struct MarkerCache {
    slots: HashMap<PathBuf, Slot>,
    serial: u64,
}

impl MarkerCache {
    /// Folder `dir`'s markers as far as they're known: what its probe
    /// found, else `listed`, its markers from the tree's listing if it has
    /// one, else none.
    pub fn markers(&self, dir: &Path, listed: Option<Markers>) -> Markers {
        (self.slots.get(dir))
            .and_then(|slot| slot.markers)
            .or(listed)
            .unwrap_or_default()
    }

    /// Whether [`MarkerCache::request`] would give `dir` a probe.
    pub fn wants(&self, dir: &Path) -> bool {
        !self.slots.contains_key(dir)
    }

    /// The probe folder `dir` needs; `None` once one has run or is running
    /// for it, until a watcher batch forgets it.
    pub fn request(&mut self, dir: &Path) -> Option<MarkerProbe> {
        if !self.wants(dir) {
            return None;
        }
        self.serial += 1;
        let serial = self.serial;
        let slot = Slot {
            serial,
            markers: None,
        };
        self.slots.insert(dir.to_owned(), slot);
        Some(MarkerProbe {
            dir: dir.to_owned(),
            serial,
        })
    }

    /// Keeps a probe's result, unless its folder was forgotten after it was
    /// requested. Returns whether it was kept.
    pub fn store(&mut self, probed: Probed) -> bool {
        match self.slots.get_mut(&probed.dir) {
            Some(slot) if slot.serial == probed.serial => {
                slot.markers = Some(probed.markers);
                true
            }
            _ => false,
        }
    }

    /// Forgets what a watcher batch may have changed: each folder it names,
    /// with everything below it, and each folder holding an entry it names
    /// whose name can count for its markers (a build file, a `main` or
    /// `test`). A rescan forgets everything. Returns whether it forgot any
    /// folder, probed or being probed, so what's drawn can be redrawn.
    pub fn apply(&mut self, changes: &[FsChange]) -> bool {
        let before = self.slots.len();
        let mut named = HashSet::new();
        let mut holding = HashSet::new();
        for change in changes {
            match change {
                FsChange::Rescan => {
                    self.slots.clear();
                    return before > 0;
                }
                FsChange::Path(path) => {
                    named.insert(path.as_path());
                    let name = path.file_name().and_then(OsStr::to_str);
                    if name.is_none_or(matters) {
                        holding.extend(path.parent());
                    }
                }
            }
        }
        self.slots.retain(|dir, _| {
            !holding.contains(dir.as_path()) && !dir.ancestors().any(|dir| named.contains(dir))
        });
        self.slots.len() != before
    }

    /// Forgets every folder, as for a tree opened afresh.
    pub fn clear(&mut self) {
        self.slots.clear();
    }
}
