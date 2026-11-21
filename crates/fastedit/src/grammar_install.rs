//! Installs a language's WebAssembly grammar as an ordinary user extension,
//! from the verified registry index (`src/grammar_registry.rs`; the decision
//! records: `docs/decisions/c1pt00h80c-grammar-fetch.md` and
//! `docs/decisions/xb65tqark9-grammar-registry-index.md`).
//!
//! The result is `<extensions>/tenmoku.grammar_<language>/`, which the
//! extension loader ([`crate::config::with_extensions`]) registers as it would
//! any other. Nothing here runs on its own: a caller asks for one language,
//! after the user's consent. Every file is checked against the index's entry
//! before anything is written.
//!
//! A language claiming no extension or file name is injection-only: another
//! language's `injections.scm` names it (Blade's PHP expressions are
//! `php_only`). It's never offered on its own; installing a language that
//! injects it installs it too, under the same consent, if it's not installed
//! yet. The two are written together or not at all.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    future::Future,
    io::{self, Write as _},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use fe_net::{Client, GrammarFetchError, NetError, grammar_fetch, sha256_hex};
use fe_registry::grammars::FileEntry;
use fe_syntax::MAX_GRAMMAR_BYTES;
use fe_workbench::{DetectGrammar, GrammarInstallError, GrammarOffer, GrammarOffers, GrammarPack};
use gpui::AppContext as _;
use kiln::command::{Capability, Grants};
use kiln_l10n::tr;
use serde::Deserialize;

use crate::grammar_registry::{Catalog, Entry};

/// The hosts a download may reach: the releases, and where GitHub's redirects
/// send them. `release-assets` is the host it uses now, and `objects` the one
/// it used before.
pub(crate) const GRANTED_HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// A plain file name, so that it stays in the extension's directory, and not
/// one of the extension's other files.
fn is_wasm_file_name(name: &str) -> bool {
    let stem = name.strip_suffix(".wasm").unwrap_or_default();
    !stem.is_empty() && !stem.starts_with('.') && !name.contains(['/', '\\'])
}

/// A query's tokens, for [`injected_languages`].
#[derive(Clone, Copy, PartialEq)]
enum Token<'a> {
    Open,
    Close,
    /// A string's text, between its quotes.
    Str(&'a str),
    /// Anything else, such as `#set!`, `@capture` or `[`.
    Atom(&'a str),
}

/// `query`'s tokens, outside its comments.
fn tokens(query: &str) -> Vec<Token<'_>> {
    let bytes = query.as_bytes();
    let mut tokens = Vec::new();
    let mut ix = 0;
    while ix < bytes.len() {
        match bytes[ix] {
            b';' => {
                while ix < bytes.len() && bytes[ix] != b'\n' {
                    ix += 1;
                }
                continue;
            }
            b'(' => tokens.push(Token::Open),
            b')' => tokens.push(Token::Close),
            b'"' => {
                let start = ix + 1;
                ix = start;
                while ix < bytes.len() && bytes[ix] != b'"' {
                    ix += if bytes[ix] == b'\\' { 2 } else { 1 };
                }
                tokens.push(Token::Str(&query[start..ix.min(bytes.len())]));
            }
            byte if byte.is_ascii_whitespace() => {}
            _ => {
                let start = ix;
                ix += 1;
                while ix < bytes.len()
                    && !bytes[ix].is_ascii_whitespace()
                    && !b"();\"".contains(&bytes[ix])
                {
                    ix += 1;
                }
                tokens.push(Token::Atom(&query[start..ix]));
                continue;
            }
        }
        ix += 1;
    }
    tokens
}

/// The languages `query` injects whatever the text: each
/// `(#set! injection.language "…")`, its key quoted or not, perhaps scoped to a
/// capture named anywhere among its arguments, outside its comments. A
/// predicate such as `#is?` sets nothing, and a language taken from a capture
/// (`@injection.language`) isn't known until a file is parsed.
pub(crate) fn injected_languages(query: &str) -> Vec<&str> {
    const KEY: &str = "injection.language";
    let tokens = tokens(query);
    let mut found = Vec::new();
    for (ix, token) in tokens.iter().enumerate() {
        if *token != Token::Open {
            continue;
        }
        let mut rest = tokens[ix + 1..].iter().copied();
        if rest.next() != Some(Token::Atom("#set!")) {
            continue;
        }
        // Its arguments to its `)`, but a capture, which may come anywhere.
        let mut arguments = Vec::new();
        let closed = loop {
            match rest.next() {
                Some(Token::Close) => break true,
                Some(Token::Open) | None => break false,
                Some(Token::Atom(capture)) if capture.starts_with('@') => {}
                Some(argument) => arguments.push(argument),
            }
        };
        if closed && let [Token::Atom(KEY) | Token::Str(KEY), Token::Str(language)] = arguments[..]
        {
            found.push(language);
        }
    }
    found
}

/// Why a grammar wasn't installed. The extensions directory is as it was, unless
/// it's [`InstallError::NotRestored`].
#[derive(Debug)]
pub(crate) enum InstallError {
    UnknownLanguage(String),
    /// The fetch step's bundle of downloads isn't one it made.
    Bundle(String),
    /// An injection-only language the install didn't download, as it was
    /// installed then, isn't any more.
    Removed(String),
    /// An index entry's files or manifest aren't what an install may write.
    Refused {
        language: String,
        why: String,
    },
    /// The wasm couldn't be had, or isn't what the manifest recorded.
    Fetch(GrammarFetchError),
    Io {
        path: PathBuf,
        source: io::Error,
    },
    /// The new extension couldn't be put in place, nor the old one put back:
    /// `previous` is the only copy of what was at `target`.
    NotRestored {
        target: PathBuf,
        previous: PathBuf,
        source: io::Error,
        restoring: io::Error,
    },
}

impl From<GrammarFetchError> for InstallError {
    fn from(error: GrammarFetchError) -> Self {
        InstallError::Fetch(error)
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::UnknownLanguage(language) => write!(
                f,
                "{}",
                tr!(
                    "fastedit-grammar-unknown-language",
                    language = language.as_str()
                )
            ),
            InstallError::Bundle(error) => {
                write!(
                    f,
                    "{}",
                    tr!("fastedit-grammar-bundle", error = error.as_str())
                )
            }
            InstallError::Removed(language) => write!(
                f,
                "{}",
                tr!("fastedit-grammar-removed", language = language.as_str())
            ),
            InstallError::Refused { language, why } => write!(
                f,
                "{}",
                tr!(
                    "fastedit-grammar-refused",
                    language = language.as_str(),
                    why = why.as_str()
                )
            ),
            InstallError::Fetch(error) => std::fmt::Display::fmt(error, f),
            InstallError::Io { path, source } => write!(
                f,
                "{}",
                tr!(
                    "fastedit-grammar-io",
                    path = path.display().to_string(),
                    error = source.to_string()
                )
            ),
            InstallError::NotRestored {
                target,
                previous,
                source,
                restoring,
            } => write!(
                f,
                "{}",
                tr!(
                    "fastedit-grammar-not-restored",
                    path = target.display().to_string(),
                    error = source.to_string(),
                    restoring = restoring.to_string(),
                    previous = previous.display().to_string()
                )
            ),
        }
    }
}

impl std::error::Error for InstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InstallError::Fetch(error) => error.source(),
            InstallError::Io { source, .. } | InstallError::NotRestored { source, .. } => {
                Some(source)
            }
            _ => None,
        }
    }
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> InstallError {
    let path = path.to_owned();
    move |source| InstallError::Io { path, source }
}

/// Whether a language has registered, or never will as it's disabled.
pub(crate) type Registered = Rc<dyn Fn(&str, &gpui::App) -> bool>;

/// How often an install looks for the injection-only languages its language
/// needs to have registered, as the workbench looks for the language.
const INJECTED_POLL: Duration = Duration::from_millis(250);

/// How long before it warns that they haven't, as the workbench does for the
/// language.
const INJECTED_DEADLINE: Duration = Duration::from_secs(60);

/// How often it looks after that, in case they still do.
const INJECTED_SLOW_POLL: Duration = Duration::from_secs(2);

/// Waits for every one of `needed`, the injection-only languages installing
/// `language` (called `name`) brings, to have registered, those installed
/// before too. Past [`INJECTED_DEADLINE`] it warns, as the workbench does when
/// the language itself is late, and goes on looking, less often. Once they all
/// have, it highlights again the editors already showing the language
/// ([`refresh_editors`]), whose injections of them were left plain.
async fn await_injected(
    needed: &[String],
    language: &str,
    name: &str,
    registered: &Registered,
    executor: &gpui::BackgroundExecutor,
    cx: &mut gpui::AsyncApp,
) {
    let deadline = executor.now() + INJECTED_DEADLINE;
    let mut warned = false;
    loop {
        let waiting = cx.update(|cx| needed.iter().find(|l| !registered(l, cx)).cloned());
        let Some(waiting) = waiting else {
            break;
        };
        if !warned && executor.now() >= deadline {
            warned = true;
            log::warn!("{waiting}, which {language} injects, hasn't registered");
            let message = tr!(
                "fastedit-grammar-injected-slow",
                name = name,
                waiting = waiting.as_str()
            )
            .to_string();
            cx.update(|cx| {
                fe_workbench::Workbench::notify(cx, log::Level::Warn, message, None);
            });
        }
        let poll = if warned {
            INJECTED_SLOW_POLL
        } else {
            INJECTED_POLL
        };
        executor.timer(poll).await;
    }
    if !needed.is_empty() {
        cx.update(|cx| refresh_editors(language, cx));
    }
}

/// Highlights again each editor showing `language`, in every window, whether
/// the language was detected or chosen: one that started before what the
/// language injects had registered left those injections plain, and nothing
/// else tells it they've come.
fn refresh_editors(language: &str, cx: &mut gpui::App) {
    let mut editors: Vec<gpui::Entity<fe_gpui::EditorView>> = Vec::new();
    let workbenches = (cx.windows().into_iter())
        .filter_map(|window| window.downcast::<fe_workbench::Workbench>());
    for window in workbenches.collect::<Vec<_>>() {
        let found = window.read_with(cx, |workbench, cx| {
            let mut found = Vec::new();
            for pane in workbench.pane_group(cx).panes() {
                for tab in pane.read(cx).tabs() {
                    found.extend(tab.editor().cloned());
                    if let Some(diff) = tab.item().downcast_ref::<fe_gpui::DiffView>() {
                        let diff = diff.read(cx);
                        found.extend([diff.left().clone(), diff.right().clone()]);
                    }
                }
            }
            found
        });
        editors.extend(found.unwrap_or_default());
    }
    for editor in editors {
        editor.update(cx, |view, cx| {
            if view.syntax().map(|syntax| syntax.language().name()) != Some(language) {
                return;
            }
            if view.is_language_chosen() {
                view.set_language(Some(language), cx);
            } else {
                // Detected again, as it was: still not chosen.
                view.set_language(None, cx);
                view.detect_language(cx);
            }
        });
    }
}

/// Whether `language` is built in or registered, or its extension is under
/// `extensions`: at launch it may not have registered yet, and it may be
/// disabled, neither of which downloading it again would help.
pub(crate) fn is_installed(language: &str, extensions: &Path) -> bool {
    fe_syntax::language_names().any(|name| name == language) || on_disk(language, extensions)
}

/// Whether `language`'s extension is under `extensions`, which is what an
/// injection-only language counts as installed by: its registration alone is
/// no sign it'll stay, as the loader withdraws one whose directory goes. A link
/// to nothing isn't an extension, and an install replaces it.
fn on_disk(language: &str, extensions: &Path) -> bool {
    extensions.join(extension_id(language)).exists()
}

/// `error` as the toast tells it.
pub(crate) fn install_failure(error: &InstallError) -> GrammarInstallError {
    let offline = |reason: &str| GrammarInstallError {
        reason: reason.to_owned(),
        offline: true,
    };
    let reason = match error {
        InstallError::Fetch(GrammarFetchError::Net(
            NetError::Connect(_) | NetError::Timeout | NetError::Io(_),
        )) => return offline(&tr!("fastedit-grammar-offline")),
        InstallError::Fetch(GrammarFetchError::HashMismatch { .. }) => {
            tr!("fastedit-grammar-hash-mismatch").to_string()
        }
        error => error.to_string(),
    };
    GrammarInstallError {
        reason,
        offline: false,
    }
}

/// The capabilities a download needs: [`GRANTED_HOSTS`].
pub(crate) fn grants() -> Grants {
    GRANTED_HOSTS
        .iter()
        .flat_map(|host| Capability::parse(&format!("net:{host}")).ok())
        .collect()
}

fn rename(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

/// Held while an install writes: one at a time, so that what one finds
/// installed stays so until it's done. The app is one process per user (a
/// second launch hands over to the first).
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One extension to write: its language's, with its files by name, and how
/// to check them before anything is written.
pub(crate) struct Staged<'a> {
    pub(crate) language: String,
    pub(crate) files: Vec<(&'a str, &'a [u8])>,
    pub(crate) verify: Box<dyn Fn() -> Result<(), InstallError> + 'a>,
}

/// The directory `language`'s extension is in, and its ID.
pub(crate) fn extension_id(language: &str) -> String {
    format!("tenmoku.grammar_{language}")
}

/// Writes `main`'s extension, and each of `injected`'s whose extension isn't
/// under `extensions`, all or none. Each of `needed`, the injection-only
/// languages `main` brings, must have its extension there or be in
/// `injected`: one that was there when the install began may have gone since.
/// (Whether one's registered doesn't count: the loader may have registered one
/// another install has since taken out, and will withdraw it.) Every one to
/// write is verified before anything is written. The directory it wrote for
/// `main`.
///
/// Each is staged in a temporary directory under `extensions` and renamed into
/// place, so it's on the same filesystem however `extensions` is linked. The
/// staged copy is a level down: the loader looks for a manifest directly in
/// each directory under `extensions`, so it never sees one half written. The
/// temporary directories hold nothing it would load, and go on any return or
/// unwind but [`InstallError::NotRestored`]; a crash can leave them behind, and
/// they stay ignored. The injected ones go in first, and come out again if
/// `main`'s can't follow; no other install can have seen them meanwhile.
pub(crate) fn write_staged(
    main: Staged<'_>,
    injected: Vec<Staged<'_>>,
    needed: &[String],
    extensions: &Path,
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> Result<PathBuf, InstallError> {
    let _writing = WRITING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let installed = |language: &str| on_disk(language, extensions);
    for language in needed {
        let brought = (injected.iter()).any(|staged| staged.language == *language);
        if !brought && !installed(language) {
            return Err(InstallError::Removed(language.clone()));
        }
    }
    let target = |language: &str| extensions.join(extension_id(language));
    let main_target = target(&main.language);
    let set: Vec<Staged<'_>> = (injected.into_iter())
        .filter(|staged| !installed(&staged.language))
        .chain([main])
        .collect();
    for staged in &set {
        (staged.verify)()?;
    }
    fs::create_dir_all(extensions).map_err(io_error(extensions))?;
    let mut staging_dirs = Vec::with_capacity(set.len());
    for staged in &set {
        let staging = tempfile::Builder::new()
            .prefix(".tenmoku-grammar-install-")
            .tempdir_in(extensions)
            .map_err(io_error(extensions))?;
        let dir = stage_extension(&staged.language, &staged.files, staging.path())?;
        staging_dirs.push((target(&staged.language), staging, dir));
    }

    let mut published: Vec<PathBuf> = Vec::new();
    for (to, staging, dir) in staging_dirs {
        if let Err(error) = publish(staging, &dir, &to, rename) {
            for injected in &published {
                if let Err(error) = fs::remove_dir_all(injected) {
                    log::warn!("couldn't remove {}: {error}", injected.display());
                }
            }
            return Err(error);
        }
        published.push(to);
    }
    Ok(main_target)
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

/// Writes `language`'s extension, `files` by name, in a new directory under
/// `staging`, which it returns.
pub(crate) fn stage_extension(
    language: &str,
    files: &[(&str, &[u8])],
    staging: &Path,
) -> Result<PathBuf, InstallError> {
    let staged = staging.join(extension_id(language));
    fs::create_dir(&staged).map_err(io_error(&staged))?;
    for (name, contents) in files {
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

/// One language to install from the grammar index: its entry's files, in its
/// release ([`Grammar::url`]).
#[derive(Clone, Debug)]
pub(crate) struct Grammar {
    pub(crate) language: String,
    /// Every file of its extension, as the entry lists them, sorted by name.
    pub(crate) files: Vec<FileEntry>,
    /// `url_template` with `{tag}` filled in, and `{file}` still to be.
    release: String,
}

/// The names an entry's files may have besides its wasm, which its
/// `extension.toml` names.
fn allowed_name(name: &str) -> bool {
    matches!(
        name,
        "extension.toml" | "highlights.scm" | "injections.scm" | "LICENSE" | "NOTICE"
    ) || name
        .strip_prefix("LICENSE-")
        .is_some_and(|rest| !rest.is_empty())
}

/// A name that stays in the extension's directory: not empty, no separator,
/// not hidden.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\'])
}

/// What the install checks of a fetched `extension.toml`.
#[derive(Deserialize)]
struct EntryManifest {
    id: String,
    contributes: EntryContributes,
}

#[derive(Deserialize)]
struct EntryContributes {
    #[serde(default)]
    grammars: Vec<EntryGrammar>,
}

#[derive(Deserialize)]
struct EntryGrammar {
    language: String,
    wasm: String,
}

impl Grammar {
    /// `entry`'s, its files in the release `url_template` names with its tag.
    pub(crate) fn from_entry(url_template: &str, entry: Entry<'_>) -> Grammar {
        Grammar {
            language: entry.language.to_owned(),
            files: entry.entry.files.clone(),
            release: url_template.replace("{tag}", &entry.entry.tag),
        }
    }

    /// Where its file `name` is.
    pub(crate) fn url(&self, name: &str) -> String {
        self.release.replace("{file}", name)
    }

    fn file(&self, name: &str) -> Option<&FileEntry> {
        self.files.iter().find(|file| file.name == name)
    }

    fn refused(&self, why: String) -> InstallError {
        InstallError::Refused {
            language: self.language.clone(),
            why,
        }
    }

    /// Refuses an entry with a file that isn't a plain name in the allowed
    /// set, or a wasm (the manifest says which, [`Grammar::check_manifest`]),
    /// or with no `extension.toml`: before anything is fetched.
    fn check_names(&self) -> Result<(), InstallError> {
        for file in &self.files {
            let name = file.name.as_str();
            if !is_plain_name(name) || !(allowed_name(name) || is_wasm_file_name(name)) {
                let why = tr!(
                    "fastedit-grammar-file-not-allowed",
                    file = format!("{name:?}")
                );
                return Err(self.refused(why.to_string()));
            }
        }
        if self.file("extension.toml").is_none() {
            return Err(self.refused(tr!("fastedit-grammar-no-manifest").to_string()));
        }
        Ok(())
    }

    /// The wasm `manifest`, its fetched `extension.toml`, names, once it's this
    /// language's extension alone, with one grammar, for this language, whose
    /// wasm is the entry's one wasm.
    fn check_manifest(&self, manifest: &[u8]) -> Result<String, InstallError> {
        let manifest = std::str::from_utf8(manifest)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                toml::from_str::<EntryManifest>(text).map_err(|error| error.to_string())
            })
            .map_err(|error| {
                let language = self.language.as_str();
                let why = tr!(
                    "fastedit-grammar-manifest-error",
                    language = language,
                    error = error
                );
                self.refused(why.to_string())
            })?;
        let id = extension_id(&self.language);
        if manifest.id != id {
            let why = tr!(
                "fastedit-grammar-manifest-other-id",
                id = manifest.id.as_str()
            );
            return Err(self.refused(why.to_string()));
        }
        let [grammar] = manifest.contributes.grammars.as_slice() else {
            let count = manifest.contributes.grammars.len();
            let why = tr!("fastedit-grammar-manifest-grammars", count = count);
            return Err(self.refused(why.to_string()));
        };
        if grammar.language != self.language {
            let other = grammar.language.as_str();
            let why = tr!("fastedit-grammar-manifest-other-language", other = other);
            return Err(self.refused(why.to_string()));
        }
        if !is_wasm_file_name(&grammar.wasm) || self.file(&grammar.wasm).is_none() {
            let wasm = format!("{:?}", grammar.wasm);
            let why = tr!("fastedit-grammar-manifest-other-wasm", wasm = wasm);
            return Err(self.refused(why.to_string()));
        }
        // Any other wasm is a file outside the set.
        if let Some(other) = (self.files.iter())
            .find(|file| is_wasm_file_name(&file.name) && file.name != grammar.wasm)
        {
            let file = format!("{:?}", other.name);
            let why = tr!("fastedit-grammar-file-not-allowed", file = file);
            return Err(self.refused(why.to_string()));
        }
        Ok(grammar.wasm.clone())
    }

    /// The most bytes of `file` fetched: its size, which for the wasm may be
    /// no more than fe-syntax would load.
    fn max_bytes(&self, file: &FileEntry) -> Result<u64, InstallError> {
        if is_wasm_file_name(&file.name) && file.size > MAX_GRAMMAR_BYTES {
            let max_bytes = MAX_GRAMMAR_BYTES;
            return Err(GrammarFetchError::TooLarge { max_bytes }.into());
        }
        Ok(file.size)
    }

    /// `Ok` if `files` are exactly the entry's, each of its size and SHA-256,
    /// with a manifest that [`Grammar::check_manifest`] takes.
    fn verify(&self, files: &[(String, Vec<u8>)]) -> Result<(), InstallError> {
        self.check_names()?;
        let fetched = files.iter().map(|(name, _)| name.as_str());
        if fetched.ne(self.files.iter().map(|file| file.name.as_str())) {
            return Err(self.refused(tr!("fastedit-grammar-files-differ").to_string()));
        }
        for (entry, (_, bytes)) in self.files.iter().zip(files) {
            check_file(entry, bytes)?;
        }
        let manifest = files.iter().find(|(name, _)| name == "extension.toml");
        self.check_manifest(&manifest.expect("checked above").1)?;
        Ok(())
    }
}

/// `Ok` if `bytes` are `entry`'s: no longer than its size, and of its SHA-256,
/// which a body of another size can't have.
fn check_file(entry: &FileEntry, bytes: &[u8]) -> Result<(), InstallError> {
    if bytes.len() as u64 > entry.size {
        let max_bytes = entry.size;
        return Err(GrammarFetchError::TooLarge { max_bytes }.into());
    }
    let actual = sha256_hex(bytes);
    if actual != entry.sha256 {
        let expected = entry.sha256.clone();
        return Err(GrammarFetchError::HashMismatch { expected, actual }.into());
    }
    Ok(())
}

/// A language's files as fetched: by name, in its entry's order.
pub(crate) type Files = Vec<(String, Vec<u8>)>;

/// One file's download: a URL, its SHA-256 and the most bytes to read.
pub(crate) type FileDownload =
    std::pin::Pin<Box<dyn Future<Output = Result<Vec<u8>, GrammarFetchError>> + Send>>;

/// How the index's offers download a file.
pub(crate) type FetchFile = Arc<dyn Fn(String, String, u64) -> FileDownload + Send + Sync>;

/// `url` with [`grammar_fetch`], under [`grants`]: its hash and size checked,
/// and only the release hosts reached.
pub(crate) fn fetch_file(url: String, sha256: String, max_bytes: u64) -> FileDownload {
    Box::pin(async move {
        let client = Client::new(&grants());
        grammar_fetch(&client, &url, &sha256, max_bytes).await
    })
}

/// Fetches every one of `grammar`'s files with `fetch`, its `extension.toml`
/// first: nothing else is fetched if the names or the manifest are refused.
/// Each is checked again as [`Grammar::verify`] checks them all. Dropping the
/// future stops it; nothing is written.
pub(crate) async fn fetch_grammar(
    grammar: &Grammar,
    fetch: &FetchFile,
) -> Result<Files, InstallError> {
    grammar.check_names()?;
    let get = async |file: &FileEntry| -> Result<Vec<u8>, InstallError> {
        let max_bytes = grammar.max_bytes(file)?;
        let url = grammar.url(&file.name);
        let bytes = fetch(url, file.sha256.clone(), max_bytes).await?;
        // Whatever the fetcher checked, before the manifest is parsed.
        check_file(file, &bytes)?;
        Ok(bytes)
    };
    let manifest_entry = grammar.file("extension.toml").expect("checked above");
    let manifest = get(manifest_entry).await?;
    grammar.check_manifest(&manifest)?;
    let mut manifest = Some(manifest);
    let mut files = Vec::with_capacity(grammar.files.len());
    for file in &grammar.files {
        let bytes = match file.name.as_str() {
            "extension.toml" => manifest.take().expect("one manifest"),
            _ => get(file).await?,
        };
        files.push((file.name.clone(), bytes));
    }
    grammar.verify(&files)?;
    Ok(files)
}

/// Whether `entry` claims no file, by extension, name or shebang: only
/// another language injecting it installs it.
fn is_injection_only(entry: &fe_registry::grammars::Language) -> bool {
    entry.extensions.is_empty() && entry.file_names.is_empty() && entry.shebangs.is_empty()
}

/// The injection-only languages of `catalog` that installing `language`
/// brings: those its `injections.scm` names, and theirs in turn, as far as
/// `injections` (a language's query, if it's known) can tell.
fn injection_closure(
    catalog: &Catalog,
    language: &str,
    injections: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut reached = Vec::new();
    let mut seen = BTreeSet::from([language.to_owned()]);
    let mut queries: Vec<String> = injections(language).into_iter().collect();
    while let Some(query) = queries.pop() {
        for injected in injected_languages(&query) {
            if !seen.insert(injected.to_owned()) {
                continue;
            }
            if catalog
                .get(injected)
                .is_some_and(|entry| is_injection_only(entry.entry))
            {
                reached.push(injected.to_owned());
                queries.extend(injections(injected));
            }
        }
    }
    reached
}

/// `language`'s `injections.scm` among `set`, or else installed under
/// `extensions`.
fn injections_in(set: &[(String, Files)], extensions: &Path, language: &str) -> Option<String> {
    match set.iter().find(|(name, _)| name == language) {
        Some((_, files)) => (files.iter())
            .find(|(name, _)| name == "injections.scm")
            .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned()),
        None => fs::read_to_string(
            extensions
                .join(extension_id(language))
                .join("injections.scm"),
        )
        .ok(),
    }
}

/// Fetches, with `fetch`, `language`'s files from `catalog`'s entry and those
/// of the injection-only languages it brings whose extensions aren't under
/// `extensions`: the language first. The first failure fails it all; dropping
/// the future stops it, with nothing written.
pub(crate) async fn fetch_entry_set(
    catalog: &Catalog,
    url_template: &str,
    language: &str,
    extensions: &Path,
    fetch: &FetchFile,
) -> Result<Vec<(String, Files)>, InstallError> {
    let grammar_of = |language: &str| {
        (catalog.get(language))
            .map(|entry| Grammar::from_entry(url_template, entry))
            .ok_or_else(|| InstallError::UnknownLanguage(language.to_owned()))
    };
    let first = grammar_of(language)?;
    let mut set = vec![(language.to_owned(), fetch_grammar(&first, fetch).await?)];
    // Each injection-only language's query is known once it's fetched.
    loop {
        let reached = injection_closure(catalog, language, &|l| injections_in(&set, extensions, l));
        let missing: Vec<String> = (reached.into_iter())
            .filter(|l| !set.iter().any(|(name, _)| name == l) && !on_disk(l, extensions))
            .collect();
        if missing.is_empty() {
            return Ok(set);
        }
        for injected in missing {
            let files = fetch_grammar(&grammar_of(&injected)?, fetch).await?;
            set.push((injected, files));
        }
    }
}

/// `set` as one run of bytes for the workbench to hand back: each language's
/// name, its number of files, and each file's name and bytes, each part led by
/// its length.
fn bundle_files(set: &[(String, Files)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut put = |part: &[u8]| {
        bytes.extend((part.len() as u64).to_le_bytes());
        bytes.extend(part);
    };
    for (language, files) in set {
        put(language.as_bytes());
        put(&(files.len() as u64).to_le_bytes());
        for (name, contents) in files {
            put(name.as_bytes());
            put(contents);
        }
    }
    bytes
}

/// The languages and files [`bundle_files`] made `bytes` of.
fn unbundle_files(mut bytes: &[u8]) -> Result<Vec<(String, Files)>, InstallError> {
    let broken = || InstallError::Bundle(tr!("fastedit-grammar-bundle-short").to_string());
    let not_utf8 = || InstallError::Bundle(tr!("fastedit-grammar-bundle-name").to_string());
    let take = |bytes: &mut &[u8]| -> Result<Vec<u8>, InstallError> {
        let (len, rest) = bytes.split_first_chunk::<8>().ok_or_else(broken)?;
        let len = usize::try_from(u64::from_le_bytes(*len)).map_err(|_| broken())?;
        let part = rest.get(..len).ok_or_else(broken)?;
        *bytes = &rest[len..];
        Ok(part.to_vec())
    };
    let mut set = Vec::new();
    while !bytes.is_empty() {
        let language = String::from_utf8(take(&mut bytes)?).map_err(|_| not_utf8())?;
        let count: [u8; 8] = take(&mut bytes)?.try_into().map_err(|_| broken())?;
        let mut files = Vec::new();
        for _ in 0..u64::from_le_bytes(count) {
            let name = String::from_utf8(take(&mut bytes)?).map_err(|_| not_utf8())?;
            files.push((name, take(&mut bytes)?));
        }
        set.push((language, files));
    }
    Ok(set)
}

/// Installs under `extensions` what [`fetch_entry_set`] fetched for
/// `language`, bundled: every file checked against `catalog`'s entries, then
/// written through [`write_staged`]. The directory it wrote, and the
/// injection-only languages the language brings, installed before or not.
pub(crate) fn write_entry_bundle(
    catalog: &Catalog,
    url_template: &str,
    language: &str,
    bundled: &[u8],
    extensions: &Path,
) -> Result<(PathBuf, Vec<String>), InstallError> {
    let set = unbundle_files(bundled)?;
    if set.first().is_none_or(|(first, _)| first != language) {
        let error = tr!("fastedit-grammar-bundle-other", language = language);
        return Err(InstallError::Bundle(error.to_string()));
    }
    let needed = injection_closure(catalog, language, &|l| injections_in(&set, extensions, l));
    let mut staged = Vec::with_capacity(set.len());
    for (ix, (name, files)) in set.iter().enumerate() {
        if ix > 0 && !needed.contains(name) {
            let error = tr!("fastedit-grammar-bundle-other", language = language);
            return Err(InstallError::Bundle(error.to_string()));
        }
        let entry =
            (catalog.get(name)).ok_or_else(|| InstallError::UnknownLanguage(name.clone()))?;
        let grammar = Grammar::from_entry(url_template, entry);
        staged.push(Staged {
            language: name.clone(),
            files: (files.iter())
                .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
                .collect(),
            verify: Box::new(move || grammar.verify(files)),
        });
    }
    let main = staged.remove(0);
    let dir = write_staged(main, staged, &needed, extensions, &rename)?;
    Ok((dir, needed))
}

/// Installs `language` from `catalog` under `extensions`, with the
/// injection-only languages it brings, downloading with `fetch`: the two steps
/// the offers take, run together. The directory it wrote.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "the app runs its two steps apart")
)]
pub(crate) async fn install_entry(
    catalog: &Catalog,
    url_template: &str,
    language: &str,
    extensions: &Path,
    fetch: &FetchFile,
) -> Result<PathBuf, InstallError> {
    let set = fetch_entry_set(catalog, url_template, language, extensions, fetch).await?;
    let bundled = bundle_files(&set);
    write_entry_bundle(catalog, url_template, language, &bundled, extensions).map(|(dir, _)| dir)
}

/// How [`index_offers_with`]'s offers download a file, and tell that a
/// language has registered.
#[derive(Clone)]
pub(crate) struct IndexHooks {
    pub(crate) fetch: FetchFile,
    pub(crate) registered: Registered,
}

/// Whether `language` has registered, or never will as its extension is
/// disabled.
fn app_registered(language: &str, cx: &gpui::App) -> bool {
    let id = extension_id(language);
    fe_syntax::language_names().any(|name| name == language)
        || cx
            .try_global::<kiln::GlobalSettings<fe_config::Settings>>()
            .is_some_and(|global| global.settings().extensions.disabled.contains(&id))
}

/// What the workbench may offer to install: the languages of `catalog`, the
/// verified index's for this app (none without one), with their packs, found
/// for a file by [`Catalog::language_for`]. Every file comes from its release
/// through [`fe_net::grammar_fetch`]. An install goes under `extensions`, and
/// Don't ask is kept in `never_file`.
pub(crate) fn offers(
    catalog: Option<Catalog>,
    extensions: PathBuf,
    never_file: Option<PathBuf>,
) -> GrammarOffers {
    let url_template = match crate::grammar_registry::baked() {
        Ok(baked) => baked.url_template,
        Err(error) => {
            log::error!("the compiled-in registry.toml: {error}");
            String::new()
        }
    };
    let catalog = catalog.unwrap_or_else(|| {
        let empty = fe_registry::grammars::Index {
            schema: fe_registry::grammars::SCHEMA,
            serial: 0,
            url_template: url_template.clone(),
            packs: BTreeMap::new(),
            languages: BTreeMap::new(),
            plugins: BTreeMap::new(),
        };
        Catalog::new(empty, &crate::grammar_registry::EngineSupport::running())
    });
    let hooks = IndexHooks {
        fetch: Arc::new(fetch_file),
        registered: Rc::new(app_registered),
    };
    index_offers_with(
        Arc::new(catalog),
        url_template,
        hooks,
        extensions,
        never_file,
    )
}

/// [`offers`] from `catalog`, its files at `url_template`, with `hooks`.
///
/// The fetch step downloads the language's files and those of the
/// injection-only languages it brings ([`fetch_entry_set`]), in one bundle of
/// bytes the workbench passes on untouched; the install step checks and writes
/// them all ([`write_entry_bundle`]) and is done once they're written. The
/// injection-only ones are then watched for on a task of their own
/// ([`await_injected`]), which highlights the language's editors again when
/// they've registered. The workbench highlights the language's tabs once it has
/// registered.
pub(crate) fn index_offers_with(
    catalog: Arc<Catalog>,
    url_template: String,
    hooks: IndexHooks,
    extensions: PathBuf,
    never_file: Option<PathBuf>,
) -> GrammarOffers {
    let offered = (catalog.index().languages.keys())
        .filter_map(|language| catalog.get(language))
        .map(|Entry { language, entry }| GrammarOffer {
            language: language.to_owned(),
            name: entry.name.clone(),
            extensions: entry.extensions.clone(),
            file_names: entry.file_names.clone(),
            shebangs: entry.shebangs.clone(),
            packs: catalog.packs_of(language).to_vec(),
            size: entry.files.iter().map(|file| file.size).sum(),
            sha256: entry.digest.clone(),
        })
        .collect();
    let packs = (catalog.index().packs.iter())
        .map(|(id, pack)| GrammarPack {
            id: id.clone(),
            name: pack.name.clone(),
            languages: (catalog.pack_languages(id).into_iter())
                .map(|entry| entry.language.to_owned())
                .collect(),
        })
        .collect();
    let detect: DetectGrammar = {
        let (catalog, extensions) = (catalog.clone(), extensions.clone());
        Rc::new(move |path: &Path, first_line: Option<&str>| {
            let found = catalog.language_to_offer(path, first_line, &extensions)?;
            Some(found.language.to_owned())
        })
    };
    let installed = {
        let extensions = extensions.clone();
        Rc::new(move |language: &str| is_installed(language, &extensions))
    };
    let fetch = {
        let (catalog, url_template) = (catalog.clone(), url_template.clone());
        let (fetch, extensions) = (hooks.fetch.clone(), extensions.clone());
        Rc::new(move |offer: &GrammarOffer, cx: &mut gpui::App| {
            let (catalog, url_template) = (catalog.clone(), url_template.clone());
            let (fetch, extensions) = (fetch.clone(), extensions.clone());
            let language = offer.language.clone();
            cx.background_spawn(async move {
                let set =
                    fetch_entry_set(&catalog, &url_template, &language, &extensions, &fetch).await;
                set.map(|set| bundle_files(&set))
                    .map_err(|error| install_failure(&error))
            })
        })
    };
    let install = Rc::new(
        move |offer: &GrammarOffer, bundled: Vec<u8>, cx: &mut gpui::App| {
            let writing = {
                let (catalog, url_template) = (catalog.clone(), url_template.clone());
                let (language, extensions) = (offer.language.clone(), extensions.clone());
                cx.background_spawn(async move {
                    write_entry_bundle(&catalog, &url_template, &language, &bundled, &extensions)
                })
            };
            let registered = hooks.registered.clone();
            let executor = cx.background_executor().clone();
            let (language, name) = (offer.language.clone(), offer.name.clone());
            cx.spawn(async move |cx| {
                let (_, needed) = writing.await.map_err(|error| install_failure(&error))?;
                // Written: what it injects is watched for apart, so that one
                // that never registers holds up nothing after it, such as the
                // rest of a pack.
                cx.spawn(async move |cx| {
                    await_injected(&needed, &language, &name, &registered, &executor, cx).await;
                })
                .detach();
                Ok(())
            })
        },
    );
    GrammarOffers {
        catalog: offered,
        packs,
        detect: Some(detect),
        installed,
        fetch,
        install,
        never_file,
    }
}

#[cfg(test)]
mod offers_tests {
    use fe_net::{GrammarFetchError, NetError};

    use super::*;

    #[test]
    fn a_language_counts_as_installed_built_in_or_on_disk() {
        let extensions = tempfile::tempdir().unwrap();
        assert!(!is_installed("not-a-language", extensions.path()));
        assert!(is_installed("rust", extensions.path()));
        fs::create_dir(extensions.path().join("tenmoku.grammar_not-a-language")).unwrap();
        assert!(is_installed("not-a-language", extensions.path()));
    }

    #[test]
    fn only_a_lost_connection_counts_as_offline() {
        for error in [
            NetError::Connect("refused".into()),
            NetError::Timeout,
            NetError::Io("reset".into()),
        ] {
            let failure = install_failure(&GrammarFetchError::Net(error).into());
            assert!(failure.offline, "{failure:?}");
            assert!(!failure.reason.contains('\n'));
        }
        let mismatch = install_failure(
            &GrammarFetchError::HashMismatch {
                expected: "a".repeat(64),
                actual: "b".repeat(64),
            }
            .into(),
        );
        assert!(!mismatch.offline);
        assert_eq!(
            mismatch.reason,
            "the download didn't match its recorded hash"
        );
        let status = install_failure(&GrammarFetchError::Status(404).into());
        assert!(!status.offline);
        assert!(status.reason.contains("404"), "{status:?}");
    }
}
