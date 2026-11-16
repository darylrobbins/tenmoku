//! The signed grammar index (`docs/release/grammar-registry.md`): fetched,
//! verified and cached, so the app knows which languages it can install, what
//! they hash to and whether it can load them. The decision record is
//! `docs/decisions/xdwbnpg9a4-grammar-registry-client.md`.
//!
//! - [`verify_index`] checks the signature with the update feed's Ed25519
//!   code ([`crate::update_signature`], the file `fe-registry` includes)
//!   before anything parses the bytes, then parses with
//!   [`fe_registry::grammars::Index::parse`], which refuses another schema.
//! - [`Cache`] keeps the last good index and its signature under the state
//!   directory, and checks them again on every load.
//! - [`refresh`] fetches the signature and the index, retries once when they
//!   don't match (the publisher uploads them one after the other), refuses an
//!   index older than the cached one, and otherwise keeps the cache.
//! - [`start`] runs it in the background at launch, as [`Policy`] decides.
//! - [`Catalog`] drops what this engine can't load ([`EngineSupport`]), and
//!   finds a language for a file that nothing installed claims
//!   ([`Catalog::language_for`]).
//!
//! Nothing here is shown to the user: card 4 offers and installs from it.

use std::{
    fs, io,
    ops::RangeInclusive,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use fe_net::{Client, GrammarFetchError};
use fe_registry::grammars::{self, Engine, Index, Language, Registry};
use gpui::App;
use kiln::core::ApiVersion;

use crate::update_signature::{PublicKey, SignatureError};

/// `extensions/grammars/registry.toml`, compiled in: where the index is and
/// the key that signs it.
const REGISTRY_TOML: &str = include_str!("../../../extensions/grammars/registry.toml");

/// The most bytes of `index.json.sig` read: the base64 of 64 bytes is 88.
pub(crate) const MAX_SIG_BYTES: u64 = 4 * 1024;

/// The most bytes of `index.json` read.
pub(crate) const MAX_INDEX_BYTES: u64 = 4 * 1024 * 1024;

/// How long [`refresh`] waits before its one retry after a signature that
/// doesn't match: the publisher uploads the signature, then the index.
pub(crate) const RETRY_PAUSE: Duration = Duration::from_secs(5);

/// How old the cache may be before a launch refreshes it.
pub(crate) const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// The cache's directory under the state directory.
const CACHE_DIR: &str = "registry";

/// The signature's file, beside [`grammars::INDEX_FILE`].
const SIG_FILE: &str = "index.json.sig";

/// Held while a launch commits a pair, so two launches refreshing at once
/// can't interleave their writes or roll each other back.
const LOCK_FILE: &str = "index.json.lock";

/// The last good pair, kept beside the cache while a commit replaces it.
const KEPT_INDEX_FILE: &str = "index.json.kept";
const KEPT_SIG_FILE: &str = "index.json.sig.kept";

const LOG: &str = "fastedit::grammar_registry";

/// What the app compiled in from `registry.toml`.
#[derive(Debug)]
pub(crate) struct Baked {
    pub(crate) index_url: String,
    #[cfg_attr(not(test), allow(dead_code, reason = "card 4 installs from it"))]
    pub(crate) url_template: String,
    pub(crate) public_key: PublicKey,
}

/// [`REGISTRY_TOML`], parsed, with its key decoded.
pub(crate) fn baked() -> Result<Baked, String> {
    let path = Path::new("extensions/grammars/registry.toml");
    let registry = Registry::parse(path, REGISTRY_TOML).map_err(|error| error.to_string())?;
    let key = registry
        .public_key
        .ok_or_else(|| "registry.toml has no public_key".to_owned())?;
    let public_key = PublicKey::from_base64(&key).map_err(|error| error.to_string())?;
    Ok(Baked {
        index_url: registry.index_url,
        url_template: registry.url_template,
        public_key,
    })
}

/// `<index_url>.sig`.
pub(crate) fn sig_url(index_url: &str) -> String {
    format!("{index_url}.sig")
}

/// Why an index wasn't taken.
#[derive(Debug, thiserror::Error)]
pub(crate) enum IndexError {
    /// The signature doesn't verify, or isn't a signature.
    #[error("the index's signature: {0}")]
    Signature(SignatureError),
    /// The signature verifies but the index isn't one this reads: another
    /// schema, or an entry that doesn't hold together.
    #[error("the index: {0}")]
    Invalid(String),
    /// A signed index older than the cached one: a replay.
    #[error("the index's serial {found} is lower than the cached {cached}")]
    Rollback { found: u64, cached: u64 },
    /// It couldn't be fetched.
    #[error("fetching {url}: {error}")]
    Fetch {
        url: String,
        error: GrammarFetchError,
    },
    /// The cache couldn't be written.
    #[error("writing the cache: {0}")]
    Io(#[from] io::Error),
}

impl IndexError {
    /// Whether the publisher may be partway through an upload: a signature
    /// that doesn't match the index, or a file briefly missing while it's
    /// replaced. Worth one retry.
    fn may_be_mid_publish(&self) -> bool {
        match self {
            IndexError::Signature(_) => true,
            IndexError::Fetch { error, .. } => matches!(error, GrammarFetchError::Status(_)),
            _ => false,
        }
    }
}

/// `bytes`, an `index.json`, once `sig_text`, its `index.json.sig`, verifies
/// against `key` over exactly those bytes; only then is it parsed, and an
/// unknown schema refused.
pub(crate) fn verify_index(
    bytes: &[u8],
    sig_text: &str,
    key: &PublicKey,
) -> Result<Index, IndexError> {
    key.verify(bytes, sig_text).map_err(IndexError::Signature)?;
    let text =
        std::str::from_utf8(bytes).map_err(|_| IndexError::Invalid("it isn't UTF-8".to_owned()))?;
    Index::parse(text).map_err(|error| IndexError::Invalid(error.to_string()))
}

/// The last good index and its signature: `<state dir>/registry/index.json`
/// and `index.json.sig`.
#[derive(Clone, Debug)]
pub(crate) struct Cache {
    dir: PathBuf,
}

impl Cache {
    /// The cache under `state_dir`.
    pub(crate) fn new(state_dir: &Path) -> Cache {
        Cache {
            dir: state_dir.join(CACHE_DIR),
        }
    }

    /// The cache under [`fe_config::paths::state_dir`], if there is one.
    pub(crate) fn in_state_dir() -> Option<Cache> {
        fe_config::paths::state_dir().map(|dir| Cache::new(&dir))
    }

    pub(crate) fn index_path(&self) -> PathBuf {
        self.dir.join(grammars::INDEX_FILE)
    }

    pub(crate) fn sig_path(&self) -> PathBuf {
        self.dir.join(SIG_FILE)
    }

    fn kept_index_path(&self) -> PathBuf {
        self.dir.join(KEPT_INDEX_FILE)
    }

    fn kept_sig_path(&self) -> PathBuf {
        self.dir.join(KEPT_SIG_FILE)
    }

    /// The cached index, verified against `key` again: one that's missing,
    /// or that was changed on disk, is none. After a commit interrupted
    /// between its two renames, it's the last good pair the commit kept.
    pub(crate) fn load(&self, key: &PublicKey) -> Option<Index> {
        verified(&self.index_path(), &self.sig_path(), key)
            .or_else(|| verified(&self.kept_index_path(), &self.kept_sig_path(), key))
            .map(|(index, ..)| index)
    }

    /// How long ago the cached index was written, if the cache holds a pair
    /// that verifies: none if it's missing, unreadable or doesn't verify, or
    /// if only a kept pair does, so that any of those is due a refresh. A
    /// modification time in the future counts as just written.
    pub(crate) fn usable_age(&self, key: &PublicKey, now: SystemTime) -> Option<Duration> {
        verified(&self.index_path(), &self.sig_path(), key)?;
        let modified = fs::metadata(self.index_path()).ok()?.modified().ok()?;
        Some(now.duration_since(modified).unwrap_or_default())
    }

    /// Writes `index` and `sig`, an index with `serial` verified against
    /// `key`, as the cached pair, unless the cached index's serial is higher:
    /// see [`Cache::commit_with`].
    pub(crate) fn commit(
        &self,
        key: &PublicKey,
        serial: u64,
        index: &[u8],
        sig: &[u8],
    ) -> Result<(), IndexError> {
        self.commit_with(key, serial, index, sig, || Ok(()))
    }

    /// [`Cache::commit`], calling `between` between its two renames.
    ///
    /// Under the cache's lock, it reads the cached serial again (another
    /// launch may have committed since this one fetched) and refuses a lower
    /// one. It copies the cached pair aside, writes the new pair to temporary
    /// files and renames them into place, the signature first, then removes
    /// the copy. A reader between the renames, or after a failure or a crash
    /// there, sees a pair that doesn't verify and takes the kept one.
    pub(crate) fn commit_with(
        &self,
        key: &PublicKey,
        serial: u64,
        index: &[u8],
        sig: &[u8],
        between: impl FnOnce() -> io::Result<()>,
    ) -> Result<(), IndexError> {
        fs::create_dir_all(&self.dir)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.dir.join(LOCK_FILE))?;
        // Released when `lock` is dropped.
        lock.lock()?;
        let current = verified(&self.index_path(), &self.sig_path(), key);
        let cached = match &current {
            Some((index, ..)) => Some(index.serial),
            None => verified(&self.kept_index_path(), &self.kept_sig_path(), key)
                .map(|(index, ..)| index.serial),
        };
        if let Some(cached) = cached
            && serial < cached
        {
            return Err(IndexError::Rollback {
                found: serial,
                cached,
            });
        }
        if let Some((_, current_index, current_sig)) = &current {
            self.write(&self.kept_sig_path(), current_sig)?;
            self.write(&self.kept_index_path(), current_index)?;
        }
        let (index_file, sig_file) = (self.temporary(index)?, self.temporary(sig)?);
        sig_file
            .persist(self.sig_path())
            .map_err(|error| error.error)?;
        between()?;
        index_file
            .persist(self.index_path())
            .map_err(|error| error.error)?;
        for kept in [self.kept_index_path(), self.kept_sig_path()] {
            if let Err(error) = fs::remove_file(&kept)
                && error.kind() != io::ErrorKind::NotFound
            {
                log::warn!(target: LOG, "removing {}: {error}", kept.display());
            }
        }
        Ok(())
    }

    /// `contents` in a new temporary file in the cache, on disk.
    fn temporary(&self, contents: &[u8]) -> io::Result<tempfile::NamedTempFile> {
        let mut file = tempfile::NamedTempFile::new_in(&self.dir)?;
        io::Write::write_all(&mut file, contents)?;
        file.as_file().sync_all()?;
        Ok(file)
    }

    /// Replaces `path` with `contents` through a temporary file.
    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        self.temporary(contents)?
            .persist(path)
            .map(drop)
            .map_err(|error| error.error)
    }
}

/// The index at `index_path`, its bytes and those of its signature at
/// `sig_path`, if both read and verify against `key`. Only a pair that's
/// there but doesn't verify is logged.
fn verified(
    index_path: &Path,
    sig_path: &Path,
    key: &PublicKey,
) -> Option<(Index, Vec<u8>, Vec<u8>)> {
    let bytes = fs::read(index_path).ok()?;
    let sig = fs::read(sig_path).ok()?;
    match verify_index(&bytes, &String::from_utf8_lossy(&sig), key) {
        Ok(index) => Some((index, bytes, sig)),
        Err(error) => {
            log::warn!(target: LOG, "ignoring the cached {}: {error}", index_path.display());
            None
        }
    }
}

/// Fetches `index_url`'s signature, then the index, with `fetch` (a URL and
/// the most bytes to read), verifies them against `key` and, unless the
/// index's serial is lower than the cached one's when it commits, writes
/// them to `cache` ([`Cache::commit`]) and returns the index.
///
/// A signature that doesn't match, or a file missing, may be the publisher
/// partway through: after `pause` of [`RETRY_PAUSE`], it tries once more.
/// Any failure leaves the cache as it was.
pub(crate) async fn refresh<Fetch, Fetching, Pause, Pausing>(
    cache: &Cache,
    index_url: &str,
    key: &PublicKey,
    fetch: Fetch,
    pause: Pause,
) -> Result<Index, IndexError>
where
    Fetch: Fn(String, u64) -> Fetching,
    Fetching: Future<Output = Result<Vec<u8>, GrammarFetchError>>,
    Pause: FnOnce(Duration) -> Pausing,
    Pausing: Future<Output = ()>,
{
    let (index, bytes, sig) = match fetch_verified(index_url, key, &fetch).await {
        Err(error) if error.may_be_mid_publish() => {
            log::info!(target: LOG, "{error}; trying once more in {RETRY_PAUSE:?}");
            pause(RETRY_PAUSE).await;
            fetch_verified(index_url, key, &fetch).await?
        }
        fetched => fetched?,
    };
    cache.commit(key, index.serial, &bytes, &sig)?;
    Ok(index)
}

/// One try of [`refresh`]: the index, its bytes and its signature's.
async fn fetch_verified<Fetch, Fetching>(
    index_url: &str,
    key: &PublicKey,
    fetch: &Fetch,
) -> Result<(Index, Vec<u8>, Vec<u8>), IndexError>
where
    Fetch: Fn(String, u64) -> Fetching,
    Fetching: Future<Output = Result<Vec<u8>, GrammarFetchError>>,
{
    let get = async |url: String, max_bytes| {
        fetch(url.clone(), max_bytes)
            .await
            .map_err(|error| IndexError::Fetch { url, error })
    };
    let sig = get(sig_url(index_url), MAX_SIG_BYTES).await?;
    let bytes = get(index_url.to_owned(), MAX_INDEX_BYTES).await?;
    let index = verify_index(&bytes, &String::from_utf8_lossy(&sig), key)?;
    Ok((index, bytes, sig))
}

/// Whether this launch refreshes the index, as [`sparkle::Policy`] decides
/// whether Sparkle runs.
///
/// [`sparkle::Policy`]: crate::sparkle::Policy
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Policy {
    /// Nothing is fetched until the user asks to install a language.
    Off,
    /// A stale cache is refreshed in the background.
    Refresh,
}

impl Policy {
    /// Off for a `--perf` or `--wait` launch and under test, and with
    /// `update.automatic_checks` off: like the update check, it's the app
    /// reaching GitHub on its own.
    pub(crate) fn decide(
        perf: bool,
        wait_launch: bool,
        under_test: bool,
        automatic_checks: bool,
    ) -> Policy {
        if perf || wait_launch || under_test || !automatic_checks {
            Policy::Off
        } else {
            Policy::Refresh
        }
    }

    /// Whether a cache of `age` (none: missing) is due a refresh.
    pub(crate) fn due(self, age: Option<Duration>) -> bool {
        self == Policy::Refresh && age.is_none_or(|age| age >= MAX_AGE)
    }
}

/// Refreshes the cached index in the background if [`Policy::decide`] says
/// so and it's missing or older than [`MAX_AGE`]. Never blocks the launch;
/// a failure is logged and the cache kept.
pub(crate) fn start(
    perf: bool,
    wait_launch: bool,
    under_test: bool,
    automatic_checks: bool,
    cx: &mut App,
) {
    let policy = Policy::decide(perf, wait_launch, under_test, automatic_checks);
    if policy == Policy::Off {
        return;
    }
    let Some(cache) = Cache::in_state_dir() else {
        return;
    };
    let executor = cx.background_executor().clone();
    let timers = executor.clone();
    executor
        .spawn(async move {
            let baked = match baked() {
                Ok(baked) => baked,
                Err(error) => {
                    log::error!(target: LOG, "the compiled-in registry.toml: {error}");
                    return;
                }
            };
            // A cache that doesn't verify is as good as missing.
            if !policy.due(cache.usable_age(&baked.public_key, SystemTime::now())) {
                return;
            }
            let client = Client::new(&crate::grammar_install::grants());
            let client = &client;
            let fetched = refresh(
                &cache,
                &baked.index_url,
                &baked.public_key,
                |url, max_bytes| async move {
                    fe_net::grammar_fetch::fetch_bounded(client, &url, max_bytes).await
                },
                |pause| timers.timer(pause),
            )
            .await;
            match fetched {
                Ok(index) => {
                    let catalog = Catalog::new(index, &EngineSupport::running());
                    log::info!(
                        target: LOG,
                        "grammar index {} cached: {} languages this app can load",
                        catalog.index().serial,
                        catalog.index().languages.len()
                    );
                }
                Err(error) => {
                    log::warn!(target: LOG, "kept the cached grammar index: {error}");
                }
            }
        })
        .detach();
}

/// What the running app can load: an index entry needing more isn't offered
/// or detected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EngineSupport {
    /// The kiln extension API version ([`crate::config::EXTENSION_API`]).
    pub(crate) api: ApiVersion,
    /// The tree-sitter ABIs it loads.
    pub(crate) abi: RangeInclusive<u32>,
    /// The highlight capture names' version it colours with.
    pub(crate) captures: u32,
}

impl EngineSupport {
    /// This app's.
    pub(crate) fn running() -> EngineSupport {
        EngineSupport {
            api: crate::config::EXTENSION_API,
            abi: tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION as u32
                ..=tree_sitter::LANGUAGE_VERSION as u32,
            captures: grammars::CAPTURES,
        }
    }

    /// Why `engine` isn't one this loads, if it isn't: an extension API of
    /// another major or a higher minor (as kiln refuses a manifest), an ABI
    /// out of range, or a higher captures version.
    pub(crate) fn refuses(&self, engine: &Engine) -> Option<String> {
        let api = engine.tenmoku.split_once('.').and_then(|(major, minor)| {
            Some((major.parse::<u32>().ok()?, minor.parse::<u32>().ok()?))
        });
        match api {
            Some((major, minor)) if major == self.api.major && minor <= self.api.minor => {}
            _ => {
                return Some(format!(
                    "it needs extension API {}, and this app has {}",
                    engine.tenmoku, self.api
                ));
            }
        }
        if !self.abi.contains(&engine.abi) {
            return Some(format!(
                "its tree-sitter ABI is {}, and this app loads {} to {}",
                engine.abi,
                self.abi.start(),
                self.abi.end()
            ));
        }
        if engine.captures > self.captures {
            return Some(format!(
                "it needs capture names version {}, and this app has {}",
                engine.captures, self.captures
            ));
        }
        None
    }
}

/// An index with only the languages this engine loads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Catalog {
    index: Index,
}

/// A language of a [`Catalog`]: its key and its entry.
#[cfg_attr(not(test), allow(dead_code, reason = "card 4 offers from it"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry<'a> {
    pub(crate) language: &'a str,
    pub(crate) entry: &'a Language,
}

#[cfg_attr(not(test), allow(dead_code, reason = "card 4 offers from it"))]
impl Catalog {
    /// `index` without the languages `engine` refuses, each logged once
    /// here, and without them in its packs' lists. A pack left with none is
    /// dropped too.
    pub(crate) fn new(mut index: Index, engine: &EngineSupport) -> Catalog {
        index.languages.retain(|language, entry| {
            let refused = engine.refuses(&entry.engine);
            if let Some(reason) = &refused {
                log::info!(target: LOG, "not offering {language} {}: {reason}", entry.version);
            }
            refused.is_none()
        });
        for pack in index.packs.values_mut() {
            pack.languages
                .retain(|language| index.languages.contains_key(language));
        }
        index.packs.retain(|_, pack| !pack.languages.is_empty());
        Catalog { index }
    }

    /// The cached index, if it verifies, as a catalog for this app.
    #[allow(dead_code, reason = "card 4 offers from it")]
    pub(crate) fn cached() -> Option<Catalog> {
        let baked = baked().ok()?;
        let index = Cache::in_state_dir()?.load(&baked.public_key)?;
        Some(Catalog::new(index, &EngineSupport::running()))
    }

    pub(crate) fn index(&self) -> &Index {
        &self.index
    }

    /// `language`'s entry.
    pub(crate) fn get(&self, language: &str) -> Option<Entry<'_>> {
        let (language, entry) = self.index.languages.get_key_value(language)?;
        Some(Entry { language, entry })
    }

    /// The language to offer for `path`, whose first line is `first_line`:
    /// one that isn't built in or installed under `extensions`
    /// ([`crate::grammar_install::is_installed`]), matched by the whole file
    /// name, else by the extension (case-insensitive, the longest first),
    /// else by the `#!` interpreter. None if a registered language already
    /// claims the file ([`fe_syntax::language_name_for_path`], or
    /// [`fe_syntax::language_name_for_shebang`] for the interpreter). When
    /// several match equally, the lowest name is taken and the tie logged.
    pub(crate) fn language_for(
        &self,
        path: &Path,
        first_line: Option<&str>,
        extensions: &Path,
    ) -> Option<Entry<'_>> {
        if fe_syntax::language_name_for_path(path).is_some() {
            return None;
        }
        let candidates = || {
            self.index
                .languages
                .iter()
                .filter(|(language, _)| !crate::grammar_install::is_installed(language, extensions))
        };
        let name = path.file_name()?.to_str()?;
        let by_name = candidates()
            .filter(|(_, entry)| entry.file_names.iter().any(|file| file == name))
            .map(|matched| (0, matched));
        if let Some(found) = pick(path, by_name) {
            return Some(found);
        }
        let by_extension = candidates().filter_map(|matched| {
            let longest = matched
                .1
                .extensions
                .iter()
                .filter(|extension| has_extension(name, extension))
                .map(String::len)
                .max()?;
            Some((usize::MAX - longest, matched))
        });
        if let Some(found) = pick(path, by_extension) {
            return Some(found);
        }
        let first_line = first_line?;
        if fe_syntax::language_name_for_shebang(first_line).is_some() {
            return None;
        }
        let program = fe_syntax::shebang_program(first_line)?;
        let by_shebang = candidates()
            .filter(|(_, entry)| {
                entry
                    .shebangs
                    .iter()
                    .any(|interpreter| interpreter_matches(interpreter, &program))
            })
            .map(|matched| (0, matched));
        pick(path, by_shebang)
    }

    /// The packs `language` is in, as its entry lists them: the first is its
    /// main pack. Empty for a language the catalog doesn't have.
    pub(crate) fn packs_of(&self, language: &str) -> &[String] {
        self.index
            .languages
            .get(language)
            .map_or(&[], |entry| entry.packs.as_slice())
    }

    /// The languages in `pack` that this app loads, sorted.
    pub(crate) fn pack_languages(&self, pack: &str) -> Vec<Entry<'_>> {
        self.index
            .packs
            .get(pack)
            .into_iter()
            .flat_map(|pack| &pack.languages)
            .filter_map(|language| self.get(language))
            .collect()
    }
}

/// The best of `matches`, each with a rank (lower is better), in the index's
/// order: the lowest rank, and of those the lowest name. A tie is logged.
#[cfg_attr(not(test), allow(dead_code, reason = "card 4 offers from it"))]
fn pick<'a>(
    path: &Path,
    matches: impl Iterator<Item = (usize, (&'a String, &'a Language))>,
) -> Option<Entry<'a>> {
    let mut best: Vec<(usize, &'a String, &'a Language)> = Vec::new();
    for (rank, (language, entry)) in matches {
        match best.first() {
            Some(&(top, ..)) if rank > top => {}
            Some(&(top, ..)) if rank == top => best.push((rank, language, entry)),
            _ => best = vec![(rank, language, entry)],
        }
    }
    let &(_, language, entry) = best.first()?;
    if best.len() > 1 {
        let names: Vec<&str> = best.iter().map(|(_, name, _)| name.as_str()).collect();
        log::warn!(
            target: LOG,
            "the grammar index has {} for {}; taking {language}",
            names.join(", "),
            path.display()
        );
    }
    Some(Entry { language, entry })
}

/// Whether the file `name` ends in `.extension`, ignoring ASCII case, with
/// something before the dot.
#[cfg_attr(not(test), allow(dead_code, reason = "card 4 offers from it"))]
fn has_extension(name: &str, extension: &str) -> bool {
    let Some(cut) = name.len().checked_sub(extension.len() + 1) else {
        return false;
    };
    cut > 0
        && name.is_char_boundary(cut)
        && name[cut..].starts_with('.')
        && name[cut + 1..].eq_ignore_ascii_case(extension)
}

/// Whether a shebang table's `interpreter` runs `program`: the same name,
/// or it with a version suffix (`python3.12` for `python`).
#[cfg_attr(not(test), allow(dead_code, reason = "card 4 offers from it"))]
fn interpreter_matches(interpreter: &str, program: &str) -> bool {
    program.strip_prefix(interpreter).is_some_and(|suffix| {
        suffix.is_empty()
            || suffix
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    })
}
