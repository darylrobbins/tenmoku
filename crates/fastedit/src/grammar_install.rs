//! Installs a first-party WebAssembly grammar as an ordinary user extension
//! (the decision record: `docs/decisions/c1pt00h80c-grammar-fetch.md`).
//!
//! `extensions/grammars/pack.toml`, compiled in, says where each grammar's wasm
//! is and what it hashes to. Only the wasm travels: its extension's other files
//! are compiled in too. The result is `<extensions>/tenmoku.grammar_<language>/`,
//! which the extension loader ([`crate::config::with_extensions`]) registers as
//! it would any other.
//!
//! Nothing here runs on its own: a caller asks for one language, after the
//! user's consent. The wasm comes from a fetcher the caller passes
//! ([`install_fetched`]) or a language pack ([`install_from_pack_dir`]), and is
//! checked against the manifest before anything is written.

use std::{
    collections::BTreeMap,
    fs,
    future::Future,
    io::{self, Read as _, Write as _},
    path::{Path, PathBuf},
};

use fe_net::{Client, GrammarFetchError, grammar_fetch, sha256_hex};
use fe_syntax::MAX_GRAMMAR_BYTES;
use kiln::command::{Capability, Grants};
use serde::Deserialize;

/// The hosts a download may reach: the releases, and where GitHub's redirects
/// send them. `release-assets` is the host it uses now, and `objects` the one
/// it used before.
pub(crate) const GRANTED_HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

const PACK_TOML: &str = include_str!("../../../extensions/grammars/pack.toml");

/// `extensions/grammars/pack.toml`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pack {
    /// The release's URL, which the file names are relative to.
    base_url: String,
    languages: BTreeMap<String, PackEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackEntry {
    file: String,
    size: u64,
    sha256: String,
}

impl Pack {
    pub(crate) fn embedded() -> Result<Pack, InstallError> {
        Pack::parse(PACK_TOML)
    }

    pub(crate) fn parse(text: &str) -> Result<Pack, InstallError> {
        let pack: Pack =
            toml::from_str(text).map_err(|error| InstallError::Manifest(error.to_string()))?;
        for (language, entry) in &pack.languages {
            if !is_wasm_file_name(&entry.file) {
                return Err(InstallError::Manifest(format!(
                    "{language}'s file {:?} isn't a .wasm file's name",
                    entry.file
                )));
            }
        }
        Ok(pack)
    }

    /// The languages the manifest lists, by name.
    pub(crate) fn languages(&self) -> impl Iterator<Item = &str> {
        self.languages.keys().map(String::as_str)
    }

    /// What installing `language` takes: its manifest entry and its files.
    pub(crate) fn grammar(&self, language: &str) -> Result<Grammar, InstallError> {
        let unknown = || InstallError::UnknownLanguage(language.to_owned());
        let entry = self.languages.get(language).ok_or_else(unknown)?;
        let files = first_party_files(language).ok_or_else(unknown)?;
        Ok(Grammar {
            language: language.to_owned(),
            url: format!("{}/{}", self.base_url.trim_end_matches('/'), entry.file),
            file: entry.file.clone(),
            size: entry.size,
            sha256: entry.sha256.clone(),
            files,
        })
    }
}

/// A plain file name, so that it stays in the extension's directory, and not
/// one of the extension's other files.
fn is_wasm_file_name(name: &str) -> bool {
    let stem = name.strip_suffix(".wasm").unwrap_or_default();
    !stem.is_empty() && !stem.starts_with('.') && !name.contains(['/', '\\'])
}

/// An extension's files other than its wasm.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ExtensionFiles {
    pub(crate) extension_toml: &'static str,
    pub(crate) highlights: &'static str,
    pub(crate) injections: &'static str,
}

macro_rules! first_party {
    ($($language:literal),* $(,)?) => {
        [$((
            $language,
            ExtensionFiles {
                extension_toml: include_str!(concat!(
                    "../../../extensions/grammars/", $language, "/extension.toml"
                )),
                highlights: include_str!(concat!(
                    "../../../extensions/grammars/", $language, "/highlights.scm"
                )),
                injections: include_str!(concat!(
                    "../../../extensions/grammars/", $language, "/injections.scm"
                )),
            },
        )),*]
    };
}

/// The files of each first-party grammar's extension, those the wasm parity
/// tests check.
const FIRST_PARTY: [(&str, ExtensionFiles); 8] = first_party!(
    "kotlin", "csharp", "scala", "swift", "cpp", "sql", "php", "ruby"
);

fn first_party_files(language: &str) -> Option<ExtensionFiles> {
    FIRST_PARTY
        .iter()
        .find(|(name, _)| *name == language)
        .map(|(_, files)| *files)
}

/// One grammar to install.
#[derive(Clone, Debug)]
pub(crate) struct Grammar {
    pub(crate) language: String,
    /// Where the wasm is.
    pub(crate) url: String,
    /// The wasm's file name, as the extension's manifest names it.
    pub(crate) file: String,
    /// The wasm's size in bytes: 0 until it's released.
    pub(crate) size: u64,
    /// The wasm's SHA-256 in lowercase hex: empty until it's released, which
    /// refuses every install.
    pub(crate) sha256: String,
    pub(crate) files: ExtensionFiles,
}

impl Grammar {
    /// The name of its extension's directory, and its ID.
    pub(crate) fn extension_id(&self) -> String {
        format!("tenmoku.grammar_{}", self.language)
    }

    /// The most the wasm may be: its size once recorded, and never more than
    /// fe-syntax would load.
    fn max_bytes(&self) -> u64 {
        match self.size {
            0 => MAX_GRAMMAR_BYTES,
            size => size.min(MAX_GRAMMAR_BYTES),
        }
    }

    /// `Ok` if `bytes` are what the manifest recorded.
    fn verify(&self, bytes: &[u8]) -> Result<(), GrammarFetchError> {
        let actual = sha256_hex(bytes);
        if actual == self.sha256 {
            return Ok(());
        }
        Err(GrammarFetchError::HashMismatch {
            expected: self.sha256.clone(),
            actual,
        })
    }
}

/// Why a grammar wasn't installed. The extensions directory is as it was, unless
/// it's [`InstallError::NotRestored`].
#[derive(Debug, thiserror::Error)]
pub(crate) enum InstallError {
    #[error("the grammar manifest: {0}")]
    Manifest(String),
    #[error("there's no first-party grammar for {0}")]
    UnknownLanguage(String),
    /// The wasm couldn't be had, or isn't what the manifest recorded.
    #[error(transparent)]
    Fetch(#[from] GrammarFetchError),
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    /// The new extension couldn't be put in place, nor the old one put back:
    /// `previous` is the only copy of what was at `target`.
    #[error(
        "{}: {source}; the previous version couldn't be put back ({restoring}) and is at {}",
        target.display(),
        previous.display()
    )]
    NotRestored {
        target: PathBuf,
        previous: PathBuf,
        source: io::Error,
        restoring: io::Error,
    },
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> InstallError {
    let path = path.to_owned();
    move |source| InstallError::Io { path, source }
}

/// The capabilities a download needs: [`GRANTED_HOSTS`].
pub(crate) fn grants() -> Grants {
    GRANTED_HOSTS
        .iter()
        .flat_map(|host| Capability::parse(&format!("net:{host}")).ok())
        .collect()
}

/// Downloads `grammar`'s wasm from its release through `client`.
pub(crate) async fn fetch_from_release(
    client: &Client,
    grammar: &Grammar,
) -> Result<Vec<u8>, GrammarFetchError> {
    grammar_fetch(client, &grammar.url, &grammar.sha256, grammar.max_bytes()).await
}

/// Installs the first-party grammar for `language` under `extensions`, from its
/// release. The directory it wrote.
pub(crate) async fn install_first_party(
    language: &str,
    extensions: &Path,
) -> Result<PathBuf, InstallError> {
    let grammar = Pack::embedded()?.grammar(language)?;
    let client = Client::new(&grants());
    install_fetched(&grammar, extensions, |grammar| {
        fetch_from_release(&client, grammar)
    })
    .await
}

/// Installs `grammar` under `extensions`, its wasm from `fetch`, replacing the
/// extension's directory whole if it's there. The directory it wrote.
///
/// A grammar not released yet is refused without calling `fetch`. The wasm is
/// checked against the manifest whatever `fetch` checked, and nothing is
/// written until it passes. Dropping the future while `fetch` runs stops it:
/// nothing has been written by then. Writing blocks, briefly.
pub(crate) async fn install_fetched<'a, Fetch, Fetching>(
    grammar: &'a Grammar,
    extensions: &Path,
    fetch: Fetch,
) -> Result<PathBuf, InstallError>
where
    Fetch: FnOnce(&'a Grammar) -> Fetching,
    Fetching: Future<Output = Result<Vec<u8>, GrammarFetchError>>,
{
    if grammar.sha256.is_empty() {
        return Err(GrammarFetchError::NotReleased.into());
    }
    let wasm = fetch(grammar).await?;
    grammar.verify(&wasm)?;
    write_extension(grammar, &wasm, extensions)
}

/// Installs `grammar` under `extensions`, its wasm from the language pack
/// unpacked at `pack`, which has a `tenmoku.grammar_<language>/` for each
/// language, without any network. The wasm is checked as [`install_fetched`]
/// checks it. The directory it wrote.
pub(crate) fn install_from_pack_dir(
    grammar: &Grammar,
    pack: &Path,
    extensions: &Path,
) -> Result<PathBuf, InstallError> {
    if grammar.sha256.is_empty() {
        return Err(GrammarFetchError::NotReleased.into());
    }
    let path = pack.join(grammar.extension_id()).join(&grammar.file);
    let max_bytes = grammar.max_bytes();
    let mut wasm = Vec::new();
    fs::File::open(&path)
        .and_then(|file| file.take(max_bytes + 1).read_to_end(&mut wasm))
        .map_err(io_error(&path))?;
    if wasm.len() as u64 > max_bytes {
        return Err(GrammarFetchError::TooLarge { max_bytes }.into());
    }
    grammar.verify(&wasm)?;
    write_extension(grammar, &wasm, extensions)
}

/// Writes `grammar`'s extension, whole or not at all.
///
/// It's staged in a temporary directory under `extensions` and renamed into
/// place, so it's on the same filesystem however `extensions` is linked. The
/// staged copy is a level down: the loader looks for a manifest directly in
/// each directory under `extensions`, so it never sees one half written. The
/// temporary directory holds nothing it would load, and goes on any return or
/// unwind but [`InstallError::NotRestored`]; a crash can leave it behind, and it
/// stays ignored.
fn write_extension(
    grammar: &Grammar,
    wasm: &[u8],
    extensions: &Path,
) -> Result<PathBuf, InstallError> {
    fs::create_dir_all(extensions).map_err(io_error(extensions))?;
    let staging = tempfile::Builder::new()
        .prefix(".tenmoku-grammar-install-")
        .tempdir_in(extensions)
        .map_err(io_error(extensions))?;
    let staged = stage_extension(grammar, wasm, staging.path())?;

    let target = extensions.join(grammar.extension_id());
    publish(staging, &staged, &target, &|from, to| fs::rename(from, to))?;
    Ok(target)
}

/// Renames `staged`, under `staging`, to `target`. What's at `target` moves into
/// `staging` first and goes with it; if `staged` can't follow, it moves back. If
/// it can't, `staging` stays, as it's the only copy left.
pub(crate) fn publish(
    staging: tempfile::TempDir,
    staged: &Path,
    target: &Path,
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> Result<(), InstallError> {
    let previous = staging.path().join("previous");
    let replacing = match rename(target, &previous) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(io_error(target)(error)),
    };
    if let Err(source) = rename(staged, target) {
        if replacing && let Err(restoring) = rename(&previous, target) {
            return Err(InstallError::NotRestored {
                target: target.to_owned(),
                previous: staging.keep().join("previous"),
                source,
                restoring,
            });
        }
        return Err(io_error(target)(source));
    }
    // A failure to clear away the old copy doesn't undo the install.
    let dir = staging.path().to_owned();
    if let Err(error) = staging.close() {
        log::warn!("couldn't remove {}: {error}", dir.display());
    }
    Ok(())
}

/// Writes `grammar`'s extension in a new directory under `staging`, which it
/// returns.
pub(crate) fn stage_extension(
    grammar: &Grammar,
    wasm: &[u8],
    staging: &Path,
) -> Result<PathBuf, InstallError> {
    let staged = staging.join(grammar.extension_id());
    fs::create_dir(&staged).map_err(io_error(&staged))?;
    let files = grammar.files;
    for (name, contents) in [
        (grammar.file.as_str(), wasm),
        ("highlights.scm", files.highlights.as_bytes()),
        ("injections.scm", files.injections.as_bytes()),
        ("extension.toml", files.extension_toml.as_bytes()),
    ] {
        write_synced(&staged.join(name), contents)?;
    }
    Ok(staged)
}

/// Writes `path`, on disk before the rename that publishes it.
fn write_synced(path: &Path, contents: &[u8]) -> Result<(), InstallError> {
    fs::File::create(path)
        .and_then(|mut file| {
            file.write_all(contents)?;
            file.sync_all()
        })
        .map_err(io_error(path))
}
