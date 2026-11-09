//! The first-party grammars' index, `index.json`, and what it's made from.
//!
//! The download unit is one language: its artifact is the files
//! [`Language::files`] lists, each with its size and SHA-256. A language's
//! [`Language::tag`] is content-addressed, from [`Language::digest`] over
//! those, so a published release of it is never replaced and an unchanged
//! language republishes nothing. Packs are metadata only: named lists of
//! languages, each with an offline tarball. A language says which packs
//! it's in, in its `extension.toml`'s `[[contributes.grammars]]` `packs`;
//! `extensions/grammars/registry.toml` ([`Registry`]) defines them.
//!
//! [`generate`] reads `extensions/grammars/` and the built modules and
//! returns the index without tarballs; [`Generated::write`] writes it and
//! each artifact; [`finalize`] fills in the tarballs once they're packed.
//! The output is deterministic: maps sorted by key, and nothing that varies
//! but [`Index::serial`], which the caller passes.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The index's schema, [`Index::schema`].
pub const SCHEMA: u32 = 1;

/// The highlight capture names' version, [`Engine::captures`]. Bump it only
/// when an existing capture name's meaning changes in
/// `crates/fe-syntax/src/highlight.rs`, so that an app that colours it the
/// old way doesn't install a grammar that means the new one. Adding a name
/// doesn't bump it: an app that doesn't know a name just doesn't colour it.
pub const CAPTURES: u32 = 1;

/// The pack definitions' file in the grammars directory.
pub const REGISTRY_FILE: &str = "registry.toml";

/// The index's file name, in the output directory and the release.
pub const INDEX_FILE: &str = "index.json";

/// The directory under the output directory holding each language's
/// artifact, `languages/<language>/`.
pub const LANGUAGES_DIR: &str = "languages";

/// The offline tarball of `pack`, as `scripts/package-language-pack.sh
/// --pack` names it.
pub fn tarball_name(pack: &str) -> String {
    format!("Tenmoku-language-pack-{pack}.tar.gz")
}

/// What the generator refuses, or couldn't read or write.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("{}: {message}", path.display())]
    Invalid { path: PathBuf, message: String },
    #[error("{language} lists the pack {pack:?}, which {REGISTRY_FILE} doesn't define (it defines {})", defined.join(", "))]
    UnknownPack {
        language: String,
        pack: String,
        defined: Vec<String>,
    },
    #[error("the index is schema {found}; this reads schema {SCHEMA}")]
    Schema { found: String },
    #[error("the index: {0}")]
    Index(String),
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

fn invalid(path: &Path, message: impl Into<String>) -> Error {
    Error::Invalid {
        path: path.to_owned(),
        message: message.into(),
    }
}

/// `index.json`. [`Index::parse`] reads it, checking [`SCHEMA`] first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    /// [`SCHEMA`].
    pub schema: u32,
    /// Given by the caller (CI uses HEAD's committer time); an app refuses
    /// an index older than the one it has.
    pub serial: u64,
    /// A file's URL, with `{tag}` and `{file}` replaced: a language's file
    /// is under its [`Language::tag`].
    pub url_template: String,
    /// The packs with at least one language, by name.
    pub packs: BTreeMap<String, Pack>,
    /// Every language, by name.
    pub languages: BTreeMap<String, Language>,
}

/// A named list of languages, with its offline tarball.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub name: String,
    pub description: String,
    /// Sorted.
    pub languages: Vec<String>,
    /// `Tenmoku-language-pack-<pack>.tar.gz`; none until [`finalize`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tarball: Option<FileEntry>,
}

/// One language's artifact and what the app shows of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Language {
    /// The manifest's `name` without a trailing " syntax".
    pub name: String,
    pub version: String,
    /// `<language>-<first 16 hex of digest>`: the release the files are in.
    pub tag: String,
    /// SHA-256, in hex, over `<name>\t<size>\t<sha256>\n` for each of
    /// [`Language::files`], in order ([`digest`]).
    pub digest: String,
    pub extensions: Vec<String>,
    pub file_names: Vec<String>,
    pub shebangs: Vec<String>,
    /// As the manifest lists them: the first is the language's main pack.
    pub packs: Vec<String>,
    /// The extension's SPDX licence.
    pub license: String,
    pub engine: Engine,
    /// Every file of the artifact, sorted by name.
    pub files: Vec<FileEntry>,
}

/// What an app must load to install the language.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Engine {
    /// The manifest's `[engine] tenmoku`, the kiln extension API version.
    pub tenmoku: String,
    /// The module's tree-sitter ABI.
    pub abi: u32,
    /// [`CAPTURES`].
    pub captures: u32,
}

/// A file: a plain name, its size and its SHA-256 in lowercase hex.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

impl FileEntry {
    pub fn of(name: &str, bytes: &[u8]) -> FileEntry {
        FileEntry {
            name: name.to_owned(),
            size: bytes.len() as u64,
            sha256: sha256_hex(bytes),
        }
    }
}

impl Index {
    /// Parses `index.json`: its schema must be [`SCHEMA`], it may have no
    /// key the types don't, and each entry must hold together (see
    /// [`Index::check`]).
    pub fn parse(text: &str) -> Result<Index, Error> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|error| Error::Index(error.to_string()))?;
        match value.get("schema") {
            Some(schema) if schema.as_u64() == Some(SCHEMA.into()) => {}
            Some(schema) => {
                return Err(Error::Schema {
                    found: schema.to_string(),
                });
            }
            None => return Err(Error::Index("it has no schema".to_owned())),
        }
        let index: Index =
            serde_json::from_value(value).map_err(|error| Error::Index(error.to_string()))?;
        index.check()?;
        Ok(index)
    }

    /// The index as `index.json` holds it: pretty-printed, with a trailing
    /// newline.
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self).expect("the index is plain data");
        json.push('\n');
        json
    }

    /// Checks what the types can't: each file's name is plain and its hash
    /// is SHA-256 hex, each language's digest and tag are its files', and
    /// its packs and theirs agree.
    pub fn check(&self) -> Result<(), Error> {
        let bad = |message: String| Err(Error::Index(message));
        for (key, language) in &self.languages {
            for file in &language.files {
                if !is_plain_name(&file.name) || !is_sha256(&file.sha256) {
                    return bad(format!("{key}'s file {:?} is malformed", file.name));
                }
            }
            if !(language.files.windows(2)).all(|pair| pair[0].name < pair[1].name) {
                return bad(format!("{key}'s files aren't sorted by name, once each"));
            }
            if language.digest != digest(&language.files)
                || language.tag != tag(key, &language.digest)
            {
                return bad(format!("{key}'s digest or tag isn't its files'"));
            }
            for pack in &language.packs {
                let listed = (self.packs.get(pack)).is_some_and(|p| p.languages.contains(key));
                if !listed {
                    return bad(format!(
                        "{key} lists the pack {pack}, which doesn't list it"
                    ));
                }
            }
        }
        for (key, pack) in &self.packs {
            if !is_pack_name(key) {
                return bad(format!("the pack name {key:?} is malformed"));
            }
            for language in &pack.languages {
                let listed = (self.languages.get(language)).is_some_and(|l| l.packs.contains(key));
                if !listed {
                    return bad(format!(
                        "the pack {key} lists {language}, which doesn't list it"
                    ));
                }
            }
            if let Some(tarball) = &pack.tarball
                && (tarball.name != tarball_name(key) || !is_sha256(&tarball.sha256))
            {
                return bad(format!("the pack {key}'s tarball is malformed"));
            }
        }
        Ok(())
    }
}

/// `extensions/grammars/registry.toml`: where the registry is, and the packs.
/// Which languages a pack has is in each language's `extension.toml`, never
/// here.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub schema: u32,
    pub url_template: String,
    pub index_url: String,
    pub packs: BTreeMap<String, PackDefinition>,
}

/// One of [`Registry::packs`].
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackDefinition {
    pub name: String,
    pub description: String,
}

impl Registry {
    /// Parses `registry.toml`'s text, `path` naming it in errors.
    pub fn parse(path: &Path, text: &str) -> Result<Registry, Error> {
        let registry: Registry =
            toml::from_str(text).map_err(|error| invalid(path, error.to_string()))?;
        if registry.schema != SCHEMA {
            return Err(invalid(
                path,
                format!("schema {}, not {SCHEMA}", registry.schema),
            ));
        }
        if let Some(name) = registry.packs.keys().find(|name| !is_pack_name(name)) {
            return Err(invalid(
                path,
                format!("the pack name {name:?} isn't lowercase letters, digits and -"),
            ));
        }
        Ok(registry)
    }

    /// Reads `<grammars>/registry.toml`.
    pub fn read(grammars: &Path) -> Result<Registry, Error> {
        let path = grammars.join(REGISTRY_FILE);
        let text = fs::read_to_string(&path).map_err(io_error(&path))?;
        Registry::parse(&path, &text)
    }
}

/// What the index takes from an extension's manifest; the extension loader
/// checks the rest.
#[derive(Deserialize)]
struct Manifest {
    name: String,
    version: String,
    license: String,
    engine: ManifestEngine,
    contributes: ManifestContributes,
}

#[derive(Deserialize)]
struct ManifestEngine {
    tenmoku: String,
}

#[derive(Deserialize)]
struct ManifestContributes {
    #[serde(default)]
    grammars: Vec<ManifestGrammar>,
}

#[derive(Deserialize)]
struct ManifestGrammar {
    language: String,
    wasm: String,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    file_names: Vec<String>,
    #[serde(default)]
    shebangs: Vec<String>,
    #[serde(default)]
    packs: Vec<String>,
    highlights: Option<String>,
    injections: Option<String>,
}

/// The packs `<grammars>/<language>/extension.toml` lists, in its order, for
/// each language directory under `grammars`.
pub fn manifest_packs(grammars: &Path) -> Result<BTreeMap<String, Vec<String>>, Error> {
    let mut packs = BTreeMap::new();
    for language in language_dirs(grammars)? {
        let path = grammars.join(&language).join("extension.toml");
        let (_, grammar) = read_manifest(&path, &language)?;
        packs.insert(language, grammar.packs);
    }
    Ok(packs)
}

/// The index, without tarballs, and each language's artifact.
#[derive(Clone, Debug)]
pub struct Generated {
    pub index: Index,
    /// Each language's files, as [`Language::files`] lists them, with their
    /// bytes.
    pub artifacts: BTreeMap<String, Vec<(String, Vec<u8>)>>,
    /// What isn't wrong but should be said: a language in no pack.
    pub warnings: Vec<String>,
}

/// Reads each language directory under `grammars` (one holding an
/// `extension.toml`), `grammars/registry.toml`, and each language's module
/// and its `<wasm>.abi` in `wasm`, which `scripts/build-grammar-wasm.sh`
/// writes, and makes the index at `serial`.
///
/// A language's artifact is its module, `extension.toml`, the queries the
/// manifest names, `LICENSE` (which it must have), any `LICENSE-*` and any
/// `NOTICE`: never `grammar-source.toml` or `test/`. It refuses a pack
/// `registry.toml` doesn't define, a missing file, a module whose `.abi`
/// is missing or isn't one line holding a number, and a file name that isn't
/// plain.
pub fn generate(grammars: &Path, wasm: &Path, serial: u64) -> Result<Generated, Error> {
    let registry = Registry::read(grammars)?;
    let mut languages = BTreeMap::new();
    let mut artifacts = BTreeMap::new();
    let mut members: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut warnings = Vec::new();
    for key in language_dirs(grammars)? {
        let dir = grammars.join(&key);
        let manifest_path = dir.join("extension.toml");
        let (manifest, grammar) = read_manifest(&manifest_path, &key)?;

        let mut seen = BTreeSet::new();
        for pack in &grammar.packs {
            if !registry.packs.contains_key(pack) {
                return Err(Error::UnknownPack {
                    language: key.clone(),
                    pack: pack.clone(),
                    defined: registry.packs.keys().cloned().collect(),
                });
            }
            if !seen.insert(pack) {
                return Err(invalid(
                    &manifest_path,
                    format!("lists the pack {pack} twice"),
                ));
            }
            members.entry(pack.clone()).or_default().push(key.clone());
        }
        if grammar.packs.is_empty() {
            warnings.push(format!(
                "{key} lists no packs: it's in no pack's tarball, only the one with every language"
            ));
        }

        let files = artifact_files(&dir, &manifest_path, &grammar, wasm)?;
        let entries: Vec<FileEntry> = (files.iter())
            .map(|(name, bytes)| FileEntry::of(name, bytes))
            .collect();
        let digest = digest(&entries);
        let abi = read_abi(&wasm.join(format!("{}.abi", grammar.wasm)))?;
        languages.insert(
            key.clone(),
            Language {
                name: (manifest.name.strip_suffix(" syntax"))
                    .unwrap_or(&manifest.name)
                    .to_owned(),
                version: manifest.version,
                tag: tag(&key, &digest),
                digest,
                extensions: grammar.extensions,
                file_names: grammar.file_names,
                shebangs: grammar.shebangs,
                packs: grammar.packs,
                license: manifest.license,
                engine: Engine {
                    tenmoku: manifest.engine.tenmoku,
                    abi,
                    captures: CAPTURES,
                },
                files: entries,
            },
        );
        artifacts.insert(key, files);
    }

    let packs = (registry.packs.into_iter())
        .filter_map(|(key, definition)| {
            let mut languages = members.remove(&key)?;
            languages.sort();
            let pack = Pack {
                name: definition.name,
                description: definition.description,
                languages,
                tarball: None,
            };
            Some((key, pack))
        })
        .collect();
    let index = Index {
        schema: SCHEMA,
        serial,
        url_template: registry.url_template,
        packs,
        languages,
    };
    Ok(Generated {
        index,
        artifacts,
        warnings,
    })
}

impl Generated {
    /// Writes `<out>/languages/<language>/`, each holding exactly its
    /// artifact's files (replacing what was there), and `<out>/index.json`.
    pub fn write(&self, out: &Path) -> Result<(), Error> {
        let languages = out.join(LANGUAGES_DIR);
        match fs::remove_dir_all(&languages) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                return Err(io_error(&languages)(error));
            }
            _ => {}
        }
        for (language, files) in &self.artifacts {
            let dir = languages.join(language);
            fs::create_dir_all(&dir).map_err(io_error(&dir))?;
            for (name, bytes) in files {
                let path = dir.join(name);
                fs::write(&path, bytes).map_err(io_error(&path))?;
            }
        }
        write_index(&out.join(INDEX_FILE), &self.index)
    }
}

/// Writes `index` to `path` whole: to a temporary file beside it, then
/// renamed over it.
pub fn write_index(path: &Path, index: &Index) -> Result<(), Error> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(io_error(dir))?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, index.to_json()).map_err(io_error(&temp))?;
    fs::rename(&temp, path).map_err(io_error(path))
}

/// Fills in each pack's tarball from `<tarballs>/Tenmoku-language-pack-<pack>.tar.gz`,
/// which must exist.
pub fn finalize(index: &mut Index, tarballs: &Path) -> Result<(), Error> {
    for (key, pack) in &mut index.packs {
        let name = tarball_name(key);
        let path = tarballs.join(&name);
        let bytes = fs::read(&path).map_err(io_error(&path))?;
        pack.tarball = Some(FileEntry::of(&name, &bytes));
    }
    Ok(())
}

/// SHA-256 in hex over `<name>\t<size>\t<sha256>\n` for each file, in order.
pub fn digest(files: &[FileEntry]) -> String {
    let mut lines = String::new();
    for file in files {
        let _ = writeln!(lines, "{}\t{}\t{}", file.name, file.size, file.sha256);
    }
    sha256_hex(lines.as_bytes())
}

/// `<language>-<first 16 hex of digest>`.
pub fn tag(language: &str, digest: &str) -> String {
    format!("{language}-{}", &digest[..digest.len().min(16)])
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn is_sha256(hex: &str) -> bool {
    hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A file name that stays in its directory: no `/`, `\` or leading `.`.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\'])
}

/// Lowercase letters, digits and `-`, not starting with `-`: it goes in a
/// file name.
fn is_pack_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && (name.bytes()).all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The directories under `grammars` that hold an `extension.toml`, sorted.
fn language_dirs(grammars: &Path) -> Result<Vec<String>, Error> {
    let mut languages = Vec::new();
    for entry in fs::read_dir(grammars).map_err(io_error(grammars))? {
        let entry = entry.map_err(io_error(grammars))?;
        let path = entry.path();
        if !path.join("extension.toml").is_file() {
            continue;
        }
        let name = (entry.file_name().into_string())
            .map_err(|name| invalid(&path, format!("{name:?} isn't UTF-8")))?;
        if !is_plain_name(&name) {
            return Err(invalid(&path, "a language's directory name must be plain"));
        }
        languages.push(name);
    }
    languages.sort();
    Ok(languages)
}

/// The manifest at `path` and its grammar for `language`.
fn read_manifest(path: &Path, language: &str) -> Result<(Manifest, ManifestGrammar), Error> {
    let text = fs::read_to_string(path).map_err(io_error(path))?;
    let mut manifest: Manifest =
        toml::from_str(&text).map_err(|error| invalid(path, error.to_string()))?;
    let at = (manifest.contributes.grammars.iter())
        .position(|grammar| grammar.language == language)
        .ok_or_else(|| invalid(path, format!("it has no grammar for {language}")))?;
    let grammar = manifest.contributes.grammars.swap_remove(at);
    Ok((manifest, grammar))
}

/// The artifact's files, sorted by name, with their bytes.
fn artifact_files(
    dir: &Path,
    manifest_path: &Path,
    grammar: &ManifestGrammar,
    wasm: &Path,
) -> Result<Vec<(String, Vec<u8>)>, Error> {
    let wasm_name = &grammar.wasm;
    if !is_plain_name(wasm_name) || !wasm_name.ends_with(".wasm") || wasm_name == ".wasm" {
        return Err(invalid(
            manifest_path,
            format!("wasm {wasm_name:?} isn't a .wasm file's name"),
        ));
    }
    let mut names = vec!["extension.toml".to_owned(), "LICENSE".to_owned()];
    for (key, query) in [
        ("highlights", &grammar.highlights),
        ("injections", &grammar.injections),
    ] {
        let query = query
            .as_ref()
            .ok_or_else(|| invalid(manifest_path, format!("it names no {key} query")))?;
        if !is_plain_name(query) {
            return Err(invalid(
                manifest_path,
                format!("{key} {query:?} isn't a plain file name"),
            ));
        }
        names.push(query.clone());
    }
    for entry in fs::read_dir(dir).map_err(io_error(dir))? {
        let entry = entry.map_err(io_error(dir))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let licence = name.starts_with("LICENSE-") || name == "NOTICE";
        if licence && is_plain_name(&name) && entry.path().is_file() {
            names.push(name);
        }
    }
    let mut files = Vec::new();
    for name in &names {
        let path = dir.join(name);
        let bytes = fs::read(&path).map_err(io_error(&path))?;
        files.push((name.clone(), bytes));
    }
    let module = wasm.join(wasm_name);
    let bytes = fs::read(&module).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => invalid(
            &module,
            "missing: build it with scripts/build-grammar-wasm.sh",
        ),
        _ => io_error(&module)(error),
    })?;
    files.push((wasm_name.clone(), bytes));
    files.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(pair) = files.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return Err(invalid(
            manifest_path,
            format!("{:?} is two of the artifact's files", pair[0].0),
        ));
    }
    Ok(files)
}

/// The ABI in a module's `.abi` file: one line holding a number.
fn read_abi(path: &Path) -> Result<u32, Error> {
    let text = fs::read_to_string(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => invalid(
            path,
            "missing: scripts/build-grammar-wasm.sh writes it beside the module",
        ),
        _ => io_error(path)(error),
    })?;
    let line = text.strip_suffix('\n').unwrap_or(&text);
    if line.is_empty() || !line.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid(
            path,
            format!("{text:?} isn't one line holding the ABI number"),
        ));
    }
    line.parse()
        .map_err(|_| invalid(path, format!("{line} isn't an ABI number")))
}
