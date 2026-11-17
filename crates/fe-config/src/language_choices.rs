//! Languages picked by hand for files (Change Language Mode), remembered so
//! a file opens in the language picked for it last, kept per project in
//! `<state_dir>/languages/<key>.toml`, `<key>` the project's
//! [`project_key`]. A window with no root keeps its own under the key of
//! the empty path, as its saved layouts are.
//!
//! A [`LanguageChoices`] maps files, by absolute path (canonical where the
//! file exists), to a language's name, or the empty name for Plain Text.
//! [`remember`] and [`forget`] change the list of the project a file is
//! under, under an advisory lock, so windows and processes changing one
//! list at once all land; [`remembered`] finds a file's choice in the list
//! of the nearest project around it that names it, else the rootless one.
//! A file that can't be parsed reads as an empty list and is left as it is
//! until the next change replaces it.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

pub use crate::harpoon::project_key;

/// One project's files with a language picked for them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LanguageChoices {
    /// The language for each file, by its absolute path; `""` for Plain
    /// Text.
    files: BTreeMap<String, String>,
}

impl LanguageChoices {
    /// The language picked for `file`, an absolute path as [`key`] spells
    /// it: `Some(None)` for Plain Text.
    pub fn get(&self, file: &Path) -> Option<Option<&str>> {
        let name = self.files.get(file.to_str()?)?;
        Some(Some(name.as_str()).filter(|name| !name.is_empty()))
    }

    /// Picks `language` for `file`, `None` for Plain Text; whether that
    /// changed anything. A path that isn't UTF-8 can't be kept.
    pub fn set(&mut self, file: &Path, language: Option<&str>) -> bool {
        let (Some(file), language) = (file.to_str(), language.unwrap_or_default()) else {
            return false;
        };
        let old = self.files.insert(file.to_owned(), language.to_owned());
        old.as_deref() != Some(language)
    }

    /// Forgets `file`'s language; whether it had one.
    pub fn remove(&mut self, file: &Path) -> bool {
        file.to_str()
            .is_some_and(|file| self.files.remove(file).is_some())
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Reads the list at `path`. A missing file is an empty list; an
    /// unreadable or invalid one is an empty list with the error beside it,
    /// for the caller to log, the file left as it is.
    pub fn load(path: &Path) -> (LanguageChoices, Option<io::Error>) {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return (LanguageChoices::default(), None);
            }
            Err(e) => return (LanguageChoices::default(), Some(e)),
        };
        match toml::from_str::<LanguageChoices>(&text) {
            Ok(list) => (list, None),
            Err(e) => (
                LanguageChoices::default(),
                Some(io::Error::new(io::ErrorKind::InvalidData, e)),
            ),
        }
    }

    /// Writes the list to `path` through a temp file and a rename, so a
    /// reader never sees half of it.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = toml::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir)?;
        }
        // A unique name made exclusively (`create_new` does not follow a
        // planted symlink), so concurrent saves never share a temp file.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let (tmp, mut file) = loop {
            let mut name = path.as_os_str().to_owned();
            name.push(format!(
                ".{}.{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let tmp = PathBuf::from(name);
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
            fs::rename(&tmp, path)
        })();
        if written.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        written
    }
}

/// The folder in `state_dir` the lists are kept in.
fn choices_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("languages")
}

/// `<state_dir>/languages/<key>.toml`: the list of the project at `root`,
/// or with none, of the windows without a root, under the key of the empty
/// path, which no canonical root has.
pub fn choices_file(state_dir: &Path, root: Option<&Path>) -> PathBuf {
    let key = project_key(root.unwrap_or(Path::new("")));
    choices_dir(state_dir).join(format!("{key}.toml"))
}

/// [`choices_file`] in the user's state directory, if there is one.
pub fn choices_path(root: Option<&Path>) -> Option<PathBuf> {
    crate::paths::state_dir().map(|dir| choices_file(&dir, root))
}

/// How a list spells `file`: canonical where it exists, else as given.
pub fn key(file: &Path) -> PathBuf {
    fs::canonicalize(file).unwrap_or_else(|_| file.to_owned())
}

/// The list a choice for `file` picked in a window on `root` goes in: the
/// project's, if `file` is under it, else the rootless windows'.
fn list_for(state_dir: &Path, root: Option<&Path>, file: &Path) -> PathBuf {
    let root = root
        .map(|root| fs::canonicalize(root).unwrap_or_else(|_| root.to_owned()))
        .filter(|root| file.starts_with(root));
    choices_file(state_dir, root.as_deref())
}

/// Remembers `language` (`None` for Plain Text) for `file`, picked in a
/// window on `root`, in the list [`remembered`] finds it in: the project's,
/// if `file` is under `root`, else the rootless windows'.
pub fn remember(
    state_dir: &Path,
    root: Option<&Path>,
    file: &Path,
    language: Option<&str>,
) -> io::Result<()> {
    let file = key(file);
    let list = list_for(state_dir, root, &file);
    update(&list, |list| (list.set(&file, language), ())).map(drop)
}

/// Forgets the language picked for `file` in a window on `root`, in the
/// list [`remember`] keeps it in, and in whichever list [`remembered`]
/// would find it in instead, so detection decides again.
pub fn forget(state_dir: &Path, root: Option<&Path>, file: &Path) -> io::Result<()> {
    let file = key(file);
    let own = list_for(state_dir, root, &file);
    update(&own, |list| (list.remove(&file), ()))?;
    while let Some((list, _)) = list_naming(state_dir, &file) {
        let (_, removed) = update(&list, |list| {
            let removed = list.remove(&file);
            (removed, removed)
        })?;
        if !removed {
            break;
        }
    }
    Ok(())
}

/// The language remembered for `file`, `Some(None)` for Plain Text: from
/// the list of the nearest folder around it that has one naming it, else
/// the rootless windows'. Lists that can't be read are passed over.
pub fn remembered(state_dir: &Path, file: &Path) -> Option<Option<String>> {
    list_naming(state_dir, &key(file)).map(|(_, language)| language)
}

/// The list of the nearest folder around `file`, canonical, that names
/// it, else the rootless windows' if it does, with what it names.
fn list_naming(state_dir: &Path, file: &Path) -> Option<(PathBuf, Option<String>)> {
    let dir = choices_dir(state_dir);
    // The lists there, by key: most folders have none, and need no read.
    let kept: HashSet<String> = fs::read_dir(&dir)
        .ok()?
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            Some(name.strip_suffix(".toml")?.to_owned())
        })
        .collect();
    let roots = file.ancestors().skip(1).map(Some).chain([None]);
    roots
        .map(|root| choices_file(state_dir, root))
        .filter(|list| {
            let key = list.file_stem().and_then(|stem| stem.to_str());
            key.is_some_and(|key| kept.contains(key))
        })
        .find_map(|list| {
            let language = LanguageChoices::load(&list).0.get(file)?.map(str::to_owned);
            Some((list, language))
        })
}

/// Runs `change` on the list at `path` under the advisory lock on
/// `<path>.lock`, writing the result back if `change` says it changed it.
/// An invalid file counts as an empty list, replaced only if `change`
/// changes that; an unreadable one is left alone and its error returned.
/// Returns the list, as written or as read, and what `change` gave.
pub fn update<R>(
    path: &Path,
    change: impl FnOnce(&mut LanguageChoices) -> (bool, R),
) -> io::Result<(LanguageChoices, R)> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        // Before the first write there: a launch's own isn't earlier use.
        crate::first_run::note_writing(dir);
        fs::create_dir_all(dir)?;
    }
    let mut lock_name = path.as_os_str().to_owned();
    lock_name.push(".lock");
    let lock: File = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(PathBuf::from(lock_name))?;
    lock.lock()?;
    let mut list = match LanguageChoices::load(path) {
        (list, None) => list,
        (_, Some(e)) if e.kind() == io::ErrorKind::InvalidData => {
            log::warn!("invalid language choices {}: {e}", path.display());
            LanguageChoices::default()
        }
        (_, Some(e)) => return Err(e),
    };
    let (changed, result) = change(&mut list);
    if changed {
        list.save(path)?;
    }
    Ok((list, result))
}
