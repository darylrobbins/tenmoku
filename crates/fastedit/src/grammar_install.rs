//! Installs a first-party WebAssembly grammar as an ordinary user extension
//! (the decision record: `docs/decisions/c1pt00h80c-grammar-fetch.md`).
//!
//! Each directory under `extensions/grammars/` is a first-party grammar, its
//! extension's files compiled in (`build.rs` lists them). Only the wasm
//! travels. `extensions/grammars/pack.toml`, compiled in too, says which
//! release the wasm are in, and what each recorded one hashes to: a language
//! it has no entry for isn't released yet, and every install of it is refused. The result is `<extensions>/tenmoku.grammar_<language>/`,
//! which the extension loader ([`crate::config::with_extensions`]) registers as
//! it would any other.
//!
//! Nothing here runs on its own: a caller asks for one language, after the
//! user's consent. The wasm comes from a fetcher the caller passes
//! ([`install_first_party_with`]) or a language pack ([`install_from_pack_dir`]),
//! and is checked against the manifest before anything is written.
//!
//! A first-party language claiming no extension or file name is
//! injection-only: another language's `injections.scm` names it (Blade's PHP
//! expressions are `php_only`). It's never offered on its own; installing a
//! language that injects it, by any route, installs it too, under the same
//! consent, if it's released and not installed yet ([`Pack::with_injected`]).
//! The two are written together or not at all.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    future::Future,
    io::{self, Read as _, Write as _},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use fe_net::{Client, GrammarFetchError, NetError, grammar_fetch, sha256_hex};
use fe_syntax::MAX_GRAMMAR_BYTES;
use fe_workbench::{GrammarInstallError, GrammarOffer, GrammarOffers};
use gpui::AppContext as _;
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
    /// The languages a release has been recorded for.
    #[serde(default)]
    languages: BTreeMap<String, PackEntry>,
    /// The first-party languages' files: [`FIRST_PARTY`], but in tests.
    #[serde(skip, default = "first_party_table")]
    first_party: &'static [(&'static str, ExtensionFiles)],
}

fn first_party_table() -> &'static [(&'static str, ExtensionFiles)] {
    FIRST_PARTY
}

/// A language's wasm in the release. An empty `sha256`, and a language with
/// no entry, is one not released yet: its size is 0.
#[derive(Clone, Debug, Default, Deserialize)]
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

    /// A pack with no languages, which offers nothing.
    fn empty() -> Pack {
        Pack {
            base_url: String::new(),
            languages: BTreeMap::new(),
            first_party: &[],
        }
    }

    /// This pack, with `first_party` as the first-party languages' files.
    #[cfg(test)]
    pub(crate) fn with_first_party(
        mut self,
        first_party: &'static [(&'static str, ExtensionFiles)],
    ) -> Pack {
        self.first_party = first_party;
        self
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

    /// The languages a release is recorded for, by name.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "only the manifest's tests list them")
    )]
    pub(crate) fn recorded(&self) -> impl Iterator<Item = &str> {
        self.languages.keys().map(String::as_str)
    }

    /// Each first-party language claiming extensions or file names, with its
    /// size, hash, name, extensions and file names. Its size counts the
    /// injection-only languages installing it would bring, those for which
    /// `installed` is false. An injection-only language, claiming neither, is
    /// left out, as is one whose extension manifest doesn't say, which a test
    /// rules out.
    pub(crate) fn catalog(&self, installed: &dyn Fn(&str) -> bool) -> Vec<GrammarOffer> {
        (self.first_party.iter())
            .filter_map(|(language, files)| {
                let offered = (offer_grammar(language, files))
                    .map_err(InstallError::Manifest)
                    .and_then(|grammar| {
                        let brought = self.with_injected(language, installed)?;
                        Ok((grammar, brought))
                    });
                let (grammar, brought) = match offered {
                    Ok(offered) => offered,
                    Err(error) => {
                        log::error!("{error}");
                        return None;
                    }
                };
                if grammar.is_injection_only() {
                    return None;
                }
                let entry = self.languages.get(*language).cloned().unwrap_or_default();
                Some(GrammarOffer {
                    language: (*language).to_owned(),
                    name: grammar.name,
                    extensions: grammar.extensions,
                    file_names: grammar.file_names,
                    size: entry.size + brought[1..].iter().map(|g| g.size).sum::<u64>(),
                    sha256: entry.sha256,
                })
            })
            .collect()
    }

    /// What installing `language` writes: its grammar, then the injection-only
    /// languages its `injections.scm` names, and theirs in turn, that are
    /// released and for which `installed` is false.
    pub(crate) fn with_injected(
        &self,
        language: &str,
        installed: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<Grammar>, InstallError> {
        let first = self.grammar(language)?;
        let mut queries = vec![first.files.injections];
        let mut set = vec![first];
        let mut seen = BTreeSet::from([language.to_owned()]);
        while let Some(query) = queries.pop() {
            for injected in injected_languages(query) {
                if !seen.insert(injected.to_owned()) {
                    continue;
                }
                let Some(files) = self.first_party_files(injected) else {
                    continue;
                };
                let offered = offer_grammar(injected, &files).map_err(InstallError::Manifest)?;
                if !offered.is_injection_only() {
                    continue;
                }
                // An installed one may still inject one that isn't.
                queries.push(files.injections);
                let grammar = self.grammar(injected)?;
                if !grammar.sha256.is_empty() && !installed(injected) {
                    set.push(grammar);
                }
            }
        }
        Ok(set)
    }

    fn first_party_files(&self, language: &str) -> Option<ExtensionFiles> {
        (self.first_party.iter())
            .find(|(name, _)| *name == language)
            .map(|(_, files)| *files)
    }

    /// What installing `language` takes: its manifest entry, if it has one,
    /// and its files.
    pub(crate) fn grammar(&self, language: &str) -> Result<Grammar, InstallError> {
        let files = (self.first_party_files(language))
            .ok_or_else(|| InstallError::UnknownLanguage(language.to_owned()))?;
        let entry = match self.languages.get(language) {
            Some(entry) => entry.clone(),
            None => PackEntry {
                file: offer_grammar(language, &files)
                    .map_err(InstallError::Manifest)?
                    .wasm,
                ..PackEntry::default()
            },
        };
        Ok(Grammar {
            language: language.to_owned(),
            url: format!("{}/{}", self.base_url.trim_end_matches('/'), entry.file),
            file: entry.file,
            size: entry.size,
            sha256: entry.sha256,
            files,
        })
    }
}

/// What the catalog takes from an extension's manifest.
#[derive(Deserialize)]
struct OfferManifest {
    name: String,
    contributes: OfferContributes,
}

#[derive(Deserialize)]
struct OfferContributes {
    grammars: Vec<OfferGrammar>,
}

#[derive(Deserialize)]
struct OfferGrammar {
    language: String,
    wasm: String,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    file_names: Vec<String>,
}

/// `language`'s grammar in its extension's manifest, `files`', with the
/// manifest's name for it.
struct Offered {
    name: String,
    wasm: String,
    extensions: Vec<String>,
    file_names: Vec<String>,
}

impl Offered {
    /// Whether it claims no file, by extension or by name, so that only
    /// another language injecting it installs it.
    fn is_injection_only(&self) -> bool {
        self.extensions.is_empty() && self.file_names.is_empty()
    }
}

fn offer_grammar(language: &str, files: &ExtensionFiles) -> Result<Offered, String> {
    let manifest = toml::from_str::<OfferManifest>(files.extension_toml)
        .map_err(|error| format!("{language}'s extension manifest: {error}"))?;
    let grammar = (manifest.contributes.grammars.into_iter())
        .find(|grammar| grammar.language == language)
        .ok_or_else(|| format!("{language}'s extension manifest has no grammar for it"))?;
    if !is_wasm_file_name(&grammar.wasm) {
        return Err(format!(
            "{language}'s wasm {:?} isn't a .wasm file's name",
            grammar.wasm
        ));
    }
    Ok(Offered {
        name: (manifest.name.strip_suffix(" syntax"))
            .unwrap_or(&manifest.name)
            .to_owned(),
        wasm: grammar.wasm,
        extensions: grammar.extensions,
        file_names: grammar.file_names,
    })
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

include!(concat!(env!("OUT_DIR"), "/first_party.rs"));

/// The first-party languages, sorted: one per directory under
/// `extensions/grammars/`.
#[cfg_attr(not(test), allow(dead_code, reason = "only the tests list them"))]
pub(crate) fn first_party_languages() -> impl Iterator<Item = &'static str> {
    FIRST_PARTY.iter().map(|(language, _)| *language)
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
    /// The fetch step's bundle of downloads isn't one it made.
    #[error("the downloaded grammars: {0}")]
    Bundle(String),
    /// An injection-only language the install didn't download, as it was
    /// installed then, isn't any more.
    #[error("{0}, which it injects, was removed while it downloaded: try again")]
    Removed(String),
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

/// What the workbench may offer to install: the manifest's languages, with the
/// names and extensions of their extension manifests. An install goes under
/// `extensions`, and Never is kept in `never_file`.
pub(crate) fn offers(extensions: PathBuf, never_file: Option<PathBuf>) -> GrammarOffers {
    let pack = Pack::embedded().unwrap_or_else(|error| {
        log::error!("{error}");
        Pack::empty()
    });
    let hooks = OfferHooks {
        fetch: Arc::new(|grammar: &Grammar| -> Download { Box::pin(fetch_release(grammar)) }),
        registered: Rc::new(|language: &str, cx: &gpui::App| {
            // A disabled extension never registers.
            let id = format!("tenmoku.grammar_{language}");
            fe_syntax::language_names().any(|name| name == language)
                || cx
                    .try_global::<kiln::GlobalSettings<fe_config::Settings>>()
                    .is_some_and(|global| global.settings().extensions.disabled.contains(&id))
        }),
        republish: Rc::new(|offers: GrammarOffers, cx: &mut gpui::App| {
            fe_workbench::set_grammar_offers(cx, offers);
        }),
    };
    offers_with(Arc::new(pack), hooks, extensions, never_file)
}

/// One grammar's download.
pub(crate) type Download =
    std::pin::Pin<Box<dyn Future<Output = Result<Vec<u8>, GrammarFetchError>> + Send>>;

/// Puts offers in place of the ones that made them.
pub(crate) type Republish = Rc<dyn Fn(GrammarOffers, &mut gpui::App)>;

/// Whether a language has registered, or never will as it's disabled.
pub(crate) type Registered = Rc<dyn Fn(&str, &gpui::App) -> bool>;

/// How [`offers_with`]'s offers download a grammar, tell that a language has
/// registered, and replace themselves after an install.
#[derive(Clone)]
pub(crate) struct OfferHooks {
    pub(crate) fetch: Arc<dyn Fn(&Grammar) -> Download + Send + Sync>,
    pub(crate) registered: Registered,
    pub(crate) republish: Republish,
}

/// How often an install looks for the injection-only languages its language
/// needs to have registered, as the workbench looks for the language.
const INJECTED_POLL: Duration = Duration::from_millis(250);

/// How long before it warns that they haven't, as the workbench does for the
/// language.
const INJECTED_DEADLINE: Duration = Duration::from_secs(60);

/// How often it looks after that, in case they still do.
const INJECTED_SLOW_POLL: Duration = Duration::from_secs(2);

/// [`offers`] from `pack`, with `hooks`.
///
/// The fetch step downloads the language and the injection-only languages it
/// brings, in one bundle of bytes the workbench passes on untouched; the
/// install step writes them all. It then republishes the offers, whose sizes
/// count only what's still to install, and finishes once every injection-only
/// language the language needs has registered, those installed before too.
/// Past [`INJECTED_DEADLINE`] it warns, as the workbench does when the
/// language itself is late, and goes on looking, less often. Once they all
/// have, it highlights again the editors already showing the language
/// ([`refresh_editors`]), whose injections of them were left plain, and
/// finishes; the workbench then highlights the language's other tabs once the
/// language has registered.
pub(crate) fn offers_with(
    pack: Arc<Pack>,
    hooks: OfferHooks,
    extensions: PathBuf,
    never_file: Option<PathBuf>,
) -> GrammarOffers {
    let catalog = pack.catalog(&|language| on_disk(language, &extensions));
    let installed = {
        let extensions = extensions.clone();
        Rc::new(move |language: &str| is_installed(language, &extensions))
    };
    let fetch = {
        let (pack, fetch, extensions) = (pack.clone(), hooks.fetch.clone(), extensions.clone());
        Rc::new(move |offer: &GrammarOffer, cx: &mut gpui::App| {
            let (pack, fetch) = (pack.clone(), fetch.clone());
            let (language, extensions) = (offer.language.clone(), extensions.clone());
            cx.background_spawn(async move {
                (fetch_set(&pack, &language, &extensions, &*fetch).await)
                    .map(|fetched| bundle(&fetched))
                    .map_err(|error| install_failure(&error))
            })
        })
    };
    let install = {
        let never_file = never_file.clone();
        Rc::new(
            move |offer: &GrammarOffer, bundled: Vec<u8>, cx: &mut gpui::App| {
                let writing = {
                    let (pack, extensions) = (pack.clone(), extensions.clone());
                    let language = offer.language.clone();
                    cx.background_spawn(async move {
                        write_bundle(&pack, &language, &bundled, &extensions)
                    })
                };
                let (pack, hooks) = (pack.clone(), hooks.clone());
                let (extensions, never_file) = (extensions.clone(), never_file.clone());
                let executor = cx.background_executor().clone();
                let (language, name) = (offer.language.clone(), offer.name.clone());
                cx.spawn(async move |cx| {
                    writing.await.map_err(|error| install_failure(&error))?;
                    let needed = needed(&pack, &language).map_err(|e| install_failure(&e))?;
                    let republish = hooks.republish.clone();
                    let registered = hooks.registered.clone();
                    let offers = offers_with(pack, hooks, extensions, never_file);
                    cx.update(|cx| republish(offers, cx));
                    let deadline = executor.now() + INJECTED_DEADLINE;
                    let mut warned = false;
                    loop {
                        let waiting =
                            cx.update(|cx| needed.iter().find(|l| !registered(l, cx)).cloned());
                        let Some(waiting) = waiting else {
                            break;
                        };
                        if !warned && executor.now() >= deadline {
                            warned = true;
                            log::warn!("{waiting}, which {language} injects, hasn't registered");
                            let message = format!(
                                "Installed {name} syntax, but {waiting}, which it uses, hasn't \
                                 loaded: it may after a restart"
                            );
                            cx.update(|cx| {
                                fe_workbench::Workbench::notify(
                                    cx,
                                    log::Level::Warn,
                                    message,
                                    None,
                                );
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
                        cx.update(|cx| refresh_editors(&language, cx));
                    }
                    Ok(())
                })
            },
        )
    };
    GrammarOffers {
        catalog,
        installed,
        fetch,
        install,
        never_file,
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
    extensions
        .join(format!("tenmoku.grammar_{language}"))
        .exists()
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
        )) => return offline("no connection to the download"),
        InstallError::Fetch(GrammarFetchError::HashMismatch { .. }) => {
            "the download didn't match its recorded hash".to_owned()
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

/// Downloads `grammar`'s wasm from its release through `client`.
pub(crate) async fn fetch_from_release(
    client: &Client,
    grammar: &Grammar,
) -> Result<Vec<u8>, GrammarFetchError> {
    grammar_fetch(client, &grammar.url, &grammar.sha256, grammar.max_bytes()).await
}

/// Downloads `grammar`'s wasm from its release, with a client of its own.
fn fetch_release(
    grammar: &Grammar,
) -> impl Future<Output = Result<Vec<u8>, GrammarFetchError>> + Send + use<> {
    let grammar = grammar.clone();
    async move {
        let client = Client::new(&grants());
        fetch_from_release(&client, &grammar).await
    }
}

/// Downloads, with `fetch`, `language`'s wasm and those of the injection-only
/// languages it brings whose extensions aren't under `extensions`
/// ([`Pack::with_injected`]),
/// checked when they're written. An unreleased `language` is refused without
/// calling `fetch`; the first failure fails it all. Dropping the future stops
/// it, with nothing written.
pub(crate) async fn fetch_set<Fetch, Fetching>(
    pack: &Pack,
    language: &str,
    extensions: &Path,
    fetch: &Fetch,
) -> Result<Vec<(Grammar, Vec<u8>)>, InstallError>
where
    Fetch: Fn(&Grammar) -> Fetching + ?Sized,
    Fetching: Future<Output = Result<Vec<u8>, GrammarFetchError>>,
{
    let set = pack.with_injected(language, &|language| on_disk(language, extensions))?;
    if set[0].sha256.is_empty() {
        return Err(GrammarFetchError::NotReleased.into());
    }
    let mut fetched = Vec::with_capacity(set.len());
    for grammar in set {
        let wasm = fetch(&grammar).await?;
        fetched.push((grammar, wasm));
    }
    Ok(fetched)
}

/// `fetched` as one run of bytes for the workbench to hand back: each
/// language's name and wasm, each led by its length.
fn bundle(fetched: &[(Grammar, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (grammar, wasm) in fetched {
        for part in [grammar.language.as_bytes(), wasm] {
            bytes.extend((part.len() as u64).to_le_bytes());
            bytes.extend(part);
        }
    }
    bytes
}

/// The languages and wasm [`bundle`] made `bytes` of.
fn unbundle(mut bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, InstallError> {
    let broken = || InstallError::Bundle("it's cut short".to_owned());
    let take = |bytes: &mut &[u8]| -> Result<Vec<u8>, InstallError> {
        let (len, rest) = bytes.split_first_chunk::<8>().ok_or_else(broken)?;
        let len = usize::try_from(u64::from_le_bytes(*len)).map_err(|_| broken())?;
        let part = rest.get(..len).ok_or_else(broken)?;
        *bytes = &rest[len..];
        Ok(part.to_vec())
    };
    let mut fetched = Vec::new();
    while !bytes.is_empty() {
        let language = String::from_utf8(take(&mut bytes)?)
            .map_err(|_| InstallError::Bundle("a language's name isn't UTF-8".to_owned()))?;
        fetched.push((language, take(&mut bytes)?));
    }
    Ok(fetched)
}

/// Installs under `extensions` what [`fetch_set`] downloaded for `language`,
/// bundled. An unreleased `language` is refused whatever the bytes.
pub(crate) fn write_bundle(
    pack: &Pack,
    language: &str,
    bundled: &[u8],
    extensions: &Path,
) -> Result<PathBuf, InstallError> {
    if pack.grammar(language)?.sha256.is_empty() {
        return Err(GrammarFetchError::NotReleased.into());
    }
    let mut fetched = Vec::new();
    for (name, wasm) in unbundle(bundled)? {
        fetched.push((pack.grammar(&name)?, wasm));
    }
    match fetched.split_first() {
        Some(((grammar, wasm), injected)) if grammar.language == language => {
            let needed = needed(pack, language)?;
            write_set(grammar, wasm, injected, &needed, extensions, &rename)
        }
        _ => Err(InstallError::Bundle(format!("it isn't {language}'s"))),
    }
}

/// Installs the first-party grammar for `language` under `extensions`, from its
/// release, with the injection-only languages it brings. The directory it wrote.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "the app runs its two steps apart")
)]
pub(crate) async fn install_first_party(
    language: &str,
    extensions: &Path,
) -> Result<PathBuf, InstallError> {
    install_first_party_with(&Pack::embedded()?, language, extensions, &fetch_release).await
}

/// [`install_first_party`] from `pack`, downloading with `fetch`.
pub(crate) async fn install_first_party_with<Fetch, Fetching>(
    pack: &Pack,
    language: &str,
    extensions: &Path,
    fetch: &Fetch,
) -> Result<PathBuf, InstallError>
where
    Fetch: Fn(&Grammar) -> Fetching,
    Fetching: Future<Output = Result<Vec<u8>, GrammarFetchError>>,
{
    let fetched = fetch_set(pack, language, extensions, fetch).await?;
    let ((grammar, wasm), injected) = fetched.split_first().expect("the language comes first");
    let needed = needed(pack, language)?;
    write_set(grammar, wasm, injected, &needed, extensions, &rename)
}

/// The injection-only languages installing `language` from `pack` brings,
/// installed or not.
fn needed(pack: &Pack, language: &str) -> Result<Vec<String>, InstallError> {
    let set = pack.with_injected(language, &|_| false)?;
    Ok(set[1..]
        .iter()
        .map(|grammar| grammar.language.clone())
        .collect())
}

/// Installs `grammar` alone under `extensions`, its wasm from `fetch`,
/// replacing the extension's directory whole if it's there. The directory it
/// wrote.
///
/// A grammar not released yet is refused without calling `fetch`. The wasm is
/// checked against the manifest whatever `fetch` checked, and nothing is
/// written until it passes. Dropping the future while `fetch` runs stops it:
/// nothing has been written by then. Writing blocks, briefly.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the routes install a language with what it brings"
    )
)]
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
    write_set(grammar, &wasm, &[], &[], extensions, &rename)
}

/// Installs `language` under `extensions`, with the injection-only languages it
/// brings, their wasm from the language pack unpacked at `pack_dir`, which has a
/// `tenmoku.grammar_<language>/` for each language, without any network. Each
/// wasm is checked as [`fetch_set`] checks it. The directory it wrote.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "the language pack's card calls it")
)]
pub(crate) fn install_from_pack_dir(
    pack: &Pack,
    language: &str,
    pack_dir: &Path,
    extensions: &Path,
) -> Result<PathBuf, InstallError> {
    let set = pack.with_injected(language, &|language| on_disk(language, extensions))?;
    if set[0].sha256.is_empty() {
        return Err(GrammarFetchError::NotReleased.into());
    }
    let mut fetched = Vec::with_capacity(set.len());
    for grammar in set {
        let wasm = read_from_pack_dir(&grammar, pack_dir)?;
        fetched.push((grammar, wasm));
    }
    let ((grammar, wasm), injected) = fetched.split_first().expect("the language comes first");
    let needed = needed(pack, language)?;
    write_set(grammar, wasm, injected, &needed, extensions, &rename)
}

/// `grammar`'s wasm from the language pack unpacked at `pack_dir`, never read
/// past the most it may be.
fn read_from_pack_dir(grammar: &Grammar, pack_dir: &Path) -> Result<Vec<u8>, InstallError> {
    let path = pack_dir.join(grammar.extension_id()).join(&grammar.file);
    let max_bytes = grammar.max_bytes();
    let mut wasm = Vec::new();
    fs::File::open(&path)
        .and_then(|file| file.take(max_bytes + 1).read_to_end(&mut wasm))
        .map_err(io_error(&path))?;
    if wasm.len() as u64 > max_bytes {
        return Err(GrammarFetchError::TooLarge { max_bytes }.into());
    }
    Ok(wasm)
}

fn rename(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

/// Held while an install writes: one at a time, so that what one finds
/// installed stays so until it's done. The app is one process per user (a
/// second launch hands over to the first).
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Writes `grammar`'s extension with `wasm`, and each of `injected`'s whose
/// extension isn't under `extensions`, all or none. Each of `needed`, the
/// injection-only languages `grammar` brings, must have its extension there or
/// be in `injected`: one that was there when the install began may have gone
/// since. (Whether one's registered doesn't count: the loader may have
/// registered one another install has since taken out, and will withdraw it.)
/// Every wasm is checked against the manifest before anything is written. The
/// directory it wrote for `grammar`.
///
/// Each is staged in a temporary directory under `extensions` and renamed into
/// place, so it's on the same filesystem however `extensions` is linked. The
/// staged copy is a level down: the loader looks for a manifest directly in
/// each directory under `extensions`, so it never sees one half written. The
/// temporary directories hold nothing it would load, and go on any return or
/// unwind but [`InstallError::NotRestored`]; a crash can leave them behind, and
/// they stay ignored. The injected ones go in first, and come out again if
/// `grammar`'s can't follow; no other install can have seen them meanwhile.
pub(crate) fn write_set(
    grammar: &Grammar,
    wasm: &[u8],
    injected: &[(Grammar, Vec<u8>)],
    needed: &[String],
    extensions: &Path,
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> Result<PathBuf, InstallError> {
    let _writing = WRITING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let installed = |language: &str| on_disk(language, extensions);
    for language in needed {
        let brought = (injected.iter()).any(|(grammar, _)| grammar.language == *language);
        if !brought && !installed(language) {
            return Err(InstallError::Removed(language.clone()));
        }
    }
    let target = |grammar: &Grammar| extensions.join(grammar.extension_id());
    let injected: Vec<(&Grammar, &[u8])> = (injected.iter())
        .filter(|(grammar, _)| !installed(&grammar.language))
        .map(|(grammar, wasm)| (grammar, wasm.as_slice()))
        .collect();
    let set: Vec<(&Grammar, &[u8])> = injected.into_iter().chain([(grammar, wasm)]).collect();
    for (grammar, wasm) in &set {
        grammar.verify(wasm)?;
    }
    fs::create_dir_all(extensions).map_err(io_error(extensions))?;
    let mut staged = Vec::with_capacity(set.len());
    for (grammar, wasm) in set {
        let staging = tempfile::Builder::new()
            .prefix(".tenmoku-grammar-install-")
            .tempdir_in(extensions)
            .map_err(io_error(extensions))?;
        let dir = stage_extension(grammar, wasm, staging.path())?;
        staged.push((target(grammar), staging, dir));
    }

    let mut published: Vec<PathBuf> = Vec::new();
    for (to, staging, dir) in staged {
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
    Ok(target(grammar))
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

#[cfg(test)]
mod offers_tests {
    use fe_net::{GrammarFetchError, NetError};
    use gpui::TestAppContext;

    use super::*;

    #[test]
    fn the_catalog_lists_each_language_with_its_extensions_and_file_names() {
        let catalog = Pack::embedded().unwrap().catalog(&|_| false);
        let languages: Vec<&str> = catalog
            .iter()
            .map(|offer| offer.language.as_str())
            .collect();
        // Every directory, released or not, but the injection-only ones.
        let pack = Pack::embedded().unwrap();
        let claiming_files = first_party_languages().filter(|language| {
            let files = pack.first_party_files(language).unwrap();
            !offer_grammar(language, &files).unwrap().is_injection_only()
        });
        assert_eq!(languages, claiming_files.collect::<Vec<_>>());
        let of = |language: &str| catalog.iter().find(|o| o.language == language).unwrap();
        assert_eq!(of("kotlin").name, "Kotlin");
        assert_eq!(of("kotlin").extensions, ["kt", "kts"]);
        assert_eq!(of("csharp").name, "C#");
        assert_eq!(of("cpp").name, "C++");
        assert_eq!(of("sql").name, "SQL");
        assert!(
            (catalog.iter())
                .all(|offer| !offer.extensions.is_empty() || !offer.file_names.is_empty())
        );
    }

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

    #[gpui::test]
    fn the_apps_fetch_refuses_an_unreleased_grammar(cx: &mut TestAppContext) {
        let extensions = tempfile::tempdir().unwrap();
        let offers = offers(extensions.path().to_owned(), None);
        let kotlin = (offers.catalog.iter())
            .find(|offer| offer.language == "kotlin")
            .unwrap();
        assert!(
            kotlin.sha256.is_empty(),
            "this test is for a grammar with no hash"
        );

        let fetching = cx.update(|cx| (offers.fetch)(kotlin, cx));
        let failure = cx.foreground_executor().block_test(fetching).unwrap_err();
        assert!(!failure.offline, "{failure:?}");
        assert!(failure.reason.contains("isn't released"), "{failure:?}");

        let writing = cx.update(|cx| (offers.install)(kotlin, b"wasm".to_vec(), cx));
        let failure = cx.foreground_executor().block_test(writing).unwrap_err();
        assert!(failure.reason.contains("isn't released"), "{failure:?}");
        assert_eq!(fs::read_dir(extensions.path()).unwrap().count(), 0);
    }
}
