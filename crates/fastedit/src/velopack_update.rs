//! Velopack: the installer and updater on Windows and Linux (T1909,
//! docs/decisions/q01csgae9p-installers.md), and the spike beside Sparkle on
//! macOS (T1427, docs/decisions/mh0vr0a5jj-velopack-spike.md). Off unless the
//! build baked in a feed and a public key (`TENMOKU_UPDATE_FEED`,
//! `TENMOKU_UPDATE_PUBLIC_KEY`, as `scripts/velopack-pack.sh` and the release
//! workflow do) and the app is a Velopack install: Setup.exe's on Windows, an
//! AppImage on Linux, a packed `.app` on macOS. A release build of Windows or
//! Linux reads its OS and arch's own channel ([`release_channel`]).
//!
//! Velopack itself only checks each package's size and SHA-256 against the
//! feed it was handed, so the feed is what we sign: [`SignedSource`] checks
//! `releases.<channel>.json` against `releases.<channel>.json.sig` with the
//! baked key before parsing it, and downloads only packages that verified feed
//! lists, which pins each one's SHA-256.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
};

use fe_workbench::update_check::{UpdaterHook, set_updater_hook};
use gpui::App;
use velopack::{
    Error, UpdateCheck, UpdateInfo, UpdateManager, UpdateOptions, VelopackApp, VelopackAsset,
    VelopackAssetFeed,
    bundle::Manifest,
    locator::VelopackLocatorConfig,
    sources::{FileSource, HttpSource, UpdateSource},
};

use crate::update_signature::{PublicKey, SignatureError};

/// The feed baked in at build time: an https URL, or an absolute local
/// directory (the spike's).
const FEED: Option<&str> = option_env!("TENMOKU_UPDATE_FEED");
/// The base64 Ed25519 public key baked in at build time.
const PUBLIC_KEY: Option<&str> = option_env!("TENMOKU_UPDATE_PUBLIC_KEY");
/// Set to 1 to check, download and apply at once, headless, and exit.
const UPDATE_NOW: &str = "TENMOKU_VELOPACK_UPDATE_NOW";
/// The whole of a feed or signature fetch: velopack's default is no limit.
const FEED_TIMEOUT_MS: u64 = 30_000;
/// The whole of a package download.
const PACKAGE_TIMEOUT_MS: u64 = 30 * 60_000;

fn http_options(timeout_ms: u64) -> velopack::HttpOptions {
    velopack::HttpOptions {
        TimeoutMilliseconds: timeout_ms,
        ..Default::default()
    }
}

/// Where the feed is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Feed {
    /// An https base URL.
    Https(String),
    /// An absolute local directory.
    Dir(PathBuf),
}

impl Feed {
    /// An https URL or an absolute path; never http, another scheme, or a
    /// relative path.
    fn parse(text: &str) -> Option<Feed> {
        let text = text.trim();
        let scheme = text.split_once("://").map(|(scheme, _)| scheme);
        match scheme {
            Some(scheme) if scheme.eq_ignore_ascii_case("https") => {
                // A host must follow: not `https://` alone, nor `https:///x`.
                let rest = &text["https://".len()..];
                (!rest.is_empty() && !rest.starts_with('/')).then(|| Feed::Https(text.to_owned()))
            }
            Some(_) => None,
            None => {
                let path = Path::new(text);
                path.is_absolute().then(|| Feed::Dir(path.to_owned()))
            }
        }
    }

    /// Where the feed's file `name` is: what the app fetches for it.
    pub(crate) fn file_url(&self, name: &str) -> String {
        match self {
            Feed::Https(base) => join_url(base, name),
            Feed::Dir(dir) => dir.join(name).display().to_string(),
        }
    }

    /// The fetcher for the feed's own files.
    fn fetcher(&self) -> Fetcher {
        match self {
            Feed::Https(url) => Fetcher::Https(url.clone()),
            Feed::Dir(dir) => Fetcher::Dir(dir.clone()),
        }
    }
}

impl std::fmt::Display for Feed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Feed::Https(url) => f.write_str(url),
            Feed::Dir(dir) => write!(f, "{}", dir.display()),
        }
    }
}

/// `name` under the https base `base`.
fn join_url(base: &str, name: &str) -> String {
    format!("{}/{name}", base.trim_end_matches('/'))
}

/// The feed file of `channel`: `releases.<channel>.json`, with its signature
/// beside it as `releases.<channel>.json.sig`.
pub(crate) fn feed_file(channel: &str) -> String {
    format!("releases.{channel}.json")
}

/// An OS, for its install layout and its release channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Os {
    Mac,
    Windows,
    Linux,
    Other,
}

impl Os {
    /// The OS this binary was built for.
    pub(crate) fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            Os::Other
        }
    }
}

/// The Velopack channel that the release of this OS and arch (a Rust
/// `target_arch`) is packed on, named as its downloads are, so the three
/// share one release's assets: `releases.<channel>.json` and the packages it
/// lists. None where no release ships one: macOS stays on Sparkle, and its
/// spike build reads the channel it was packed with.
pub(crate) fn release_channel(os: Os, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        (Os::Windows, "x86_64") => Some("windows-x64-stable"),
        (Os::Windows, "aarch64") => Some("windows-arm64-stable"),
        (Os::Linux, "x86_64") => Some("linux-x86_64-stable"),
        _ => None,
    }
}

/// What the updater does in this process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Policy {
    /// Not running, and why.
    Off(&'static str),
    /// Running against this feed, checked with this key.
    On { feed: Feed, key: PublicKey },
}

impl Policy {
    /// On only with both build-time values present and valid (an https URL or
    /// an absolute path; a key of 32 bytes) in a Velopack install, and never
    /// for a `--perf` or `--wait` launch or under test.
    pub(crate) fn decide(
        feed: Option<&str>,
        key: Option<&str>,
        installed: bool,
        perf: bool,
        wait_launch: bool,
        under_test: bool,
    ) -> Policy {
        // Actions passes a missing repository variable as an empty value.
        let present = |value: &&str| !value.trim().is_empty();
        let (Some(feed), Some(key)) = (feed.filter(present), key.filter(present)) else {
            return Policy::Off(
                "this build has no TENMOKU_UPDATE_FEED and TENMOKU_UPDATE_PUBLIC_KEY",
            );
        };
        let Some(feed) = Feed::parse(feed) else {
            return Policy::Off("the built-in feed is neither an https URL nor an absolute path");
        };
        let Ok(key) = PublicKey::from_base64(key) else {
            return Policy::Off("the built-in public key is not the base64 of 32 bytes");
        };
        if !installed {
            Policy::Off(
                "this app is not a Velopack install (no Update.exe, UpdateNix or UpdateMac beside \
                 its sq.version)",
            )
        } else if perf {
            Policy::Off("a --perf launch")
        } else if wait_launch {
            Policy::Off("a --wait launch")
        } else if under_test {
            Policy::Off("under test")
        } else {
            Policy::On { feed, key }
        }
    }
}

/// Reads the feed's own files: `releases.<channel>.json` and its `.sig`.
#[derive(Clone, Debug)]
pub(crate) enum Fetcher {
    /// From a local directory.
    Dir(PathBuf),
    /// From an https base URL, through velopack's ureq.
    Https(String),
}

impl Fetcher {
    fn fetch(&self, name: &str) -> Result<Vec<u8>, String> {
        match self {
            Fetcher::Dir(dir) => {
                let path = dir.join(name);
                std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))
            }
            Fetcher::Https(base) => {
                let url = join_url(base, name);
                velopack::download::download_url_as_string(
                    &url,
                    Some(&http_options(FEED_TIMEOUT_MS)),
                )
                .map(String::into_bytes)
                .map_err(|error| format!("{url}: {error}"))
            }
        }
    }
}

/// Why a feed or a package was refused.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Refusal {
    #[error("the feed {name} could not be read: {why}")]
    FeedUnreadable { name: String, why: String },
    #[error("the feed has no signature: {name} could not be read ({why})")]
    MissingSignature { name: String, why: String },
    #[error("the feed signature check failed for {name}: {error}")]
    BadSignature { name: String, error: SignatureError },
    #[error("the signed feed {name} is not a Velopack feed: {why}")]
    NotAFeed { name: String, why: String },
    #[error(
        "the signed feed lists {file} with no SHA-256, which would let velopack fall back to SHA-1"
    )]
    NoSha256 { file: String },
    #[error(
        "{file} is not in the last verified feed (by name, SHA-256 and size), so it was not downloaded"
    )]
    NotInVerifiedFeed { file: String },
    #[error(
        "the package {file} does not match the signed feed's size and SHA-256, so it was deleted"
    )]
    PackageMismatch { file: String },
}

impl From<Refusal> for Error {
    fn from(refusal: Refusal) -> Error {
        Error::Other(refusal.to_string())
    }
}

/// An update source that checks the feed's signature before velopack sees it,
/// and hands `S` (which downloads the packages) only assets of that feed.
pub(crate) struct SignedSource<S> {
    fetcher: Fetcher,
    key: PublicKey,
    inner: S,
    /// The assets of the last feed that verified.
    verified: Mutex<Vec<VelopackAsset>>,
}

impl<S: UpdateSource> SignedSource<S> {
    pub(crate) fn new(fetcher: Fetcher, key: PublicKey, inner: S) -> SignedSource<S> {
        SignedSource {
            fetcher,
            key,
            inner,
            verified: Mutex::new(Vec::new()),
        }
    }

    /// Fetches and verifies `releases.<channel>.json`, then parses it. A
    /// refusal forgets any earlier verified feed.
    pub(crate) fn check_feed(&self, channel: &str) -> Result<VelopackAssetFeed, Refusal> {
        let result = self.fetch_verified(channel);
        let mut verified = self
            .verified
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match &result {
            Ok(feed) => verified.clone_from(&feed.Assets),
            Err(_) => verified.clear(),
        }
        result
    }

    fn fetch_verified(&self, channel: &str) -> Result<VelopackAssetFeed, Refusal> {
        let name = feed_file(channel);
        let sig_name = format!("{name}.sig");
        let bytes = self
            .fetcher
            .fetch(&name)
            .map_err(|why| Refusal::FeedUnreadable {
                name: name.clone(),
                why,
            })?;
        let sig = self
            .fetcher
            .fetch(&sig_name)
            .map_err(|why| Refusal::MissingSignature {
                name: sig_name.clone(),
                why,
            })?;
        // Not UTF-8 can't be base64, so it fails the decode as any bad text does.
        let sig = String::from_utf8_lossy(&sig);
        self.key
            .verify(&bytes, &sig)
            .map_err(|error| Refusal::BadSignature {
                name: name.clone(),
                error,
            })?;
        let feed: VelopackAssetFeed =
            serde_json::from_slice(&bytes).map_err(|error| Refusal::NotAFeed {
                name: name.clone(),
                why: error.to_string(),
            })?;
        if let Some(asset) = feed
            .Assets
            .iter()
            .find(|asset| asset.SHA256.trim().is_empty())
        {
            return Err(Refusal::NoSha256 {
                file: asset.FileName.clone(),
            });
        }
        Ok(feed)
    }

    /// Whether `asset` is one the last verified feed lists, by file name,
    /// SHA-256 and size.
    fn is_verified(&self, asset: &VelopackAsset) -> bool {
        let verified = self
            .verified
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        verified.iter().any(|known| {
            known.FileName == asset.FileName
                && known.Size == asset.Size
                && !known.SHA256.is_empty()
                && known.SHA256.eq_ignore_ascii_case(&asset.SHA256)
        })
    }
}

impl<S: UpdateSource> UpdateSource for SignedSource<S> {
    fn get_release_feed(
        &self,
        channel: &str,
        _app: &Manifest,
        _staged_user_id: &str,
    ) -> Result<VelopackAssetFeed, Error> {
        Ok(self.check_feed(channel)?)
    }

    fn download_release_entry(
        &self,
        asset: &VelopackAsset,
        local_file: &Path,
        progress_sender: Option<Sender<i16>>,
    ) -> Result<(), Error> {
        if !self.is_verified(asset) {
            return Err(Refusal::NotInVerifiedFeed {
                file: asset.FileName.clone(),
            }
            .into());
        }
        self.inner
            .download_release_entry(asset, local_file, progress_sender)
    }
}

/// This app as a Velopack install, found from its executable: on macOS its
/// `.app` bundle (from the canonicalized executable, so a symlink's launch,
/// 'tenmoku' in PATH, counts as its bundle's); on Windows the folder Setup.exe
/// installed, with `Update.exe` beside `current\`; on Linux the AppImage that
/// `$APPIMAGE` names, mounted with `UpdateNix` beside the binary. Each is the
/// layout velopack's own locator expects there.
#[derive(Clone, Debug)]
pub(crate) struct Install {
    pub(crate) config: VelopackLocatorConfig,
    /// The channel checked, when this OS and arch has a release one; else the
    /// one the install was packed with.
    pub(crate) channel: Option<&'static str>,
}

impl Install {
    fn locate() -> Option<Install> {
        let os = Os::current();
        let exe = std::env::current_exe().ok()?;
        // Not on Windows, whose canonical paths take a `\\?\` prefix that
        // velopack's own locator never sees.
        let exe = if os == Os::Windows {
            exe
        } else {
            std::fs::canonicalize(exe).ok()?
        };
        let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Install::locate_at(
            os,
            std::env::consts::ARCH,
            &exe,
            appimage.as_deref(),
            home.as_deref(),
        )
    }

    /// The install that `exe` (the running binary) is part of on `os` and
    /// `arch`, with `$APPIMAGE` and `$HOME` as given: None when it isn't one.
    pub(crate) fn locate_at(
        os: Os,
        arch: &str,
        exe: &Path,
        appimage: Option<&Path>,
        home: Option<&Path>,
    ) -> Option<Install> {
        let config = match os {
            Os::Mac => mac_layout(exe, home?)?,
            Os::Windows => windows_layout(exe)?,
            Os::Linux => appimage_layout(exe, appimage?)?,
            Os::Other => return None,
        };
        Some(Install {
            config,
            channel: release_channel(os, arch),
        })
    }

    /// What the update manager is told: this install's release channel, if
    /// it has one. A downgrade stays refused.
    pub(crate) fn options(&self) -> UpdateOptions {
        UpdateOptions {
            ExplicitChannel: self.channel.map(str::to_owned),
            ..Default::default()
        }
    }

    /// The update manager over our signed source.
    pub(crate) fn manager(&self, feed: &Feed, key: &PublicKey) -> Result<UpdateManager, Error> {
        let fetcher = feed.fetcher();
        match feed {
            Feed::Https(url) => UpdateManager::new(
                SignedSource::new(
                    fetcher,
                    key.clone(),
                    HttpSource::new_with_options(url, http_options(PACKAGE_TIMEOUT_MS)),
                ),
                Some(self.options()),
                Some(self.config.clone()),
            ),
            Feed::Dir(dir) => UpdateManager::new(
                SignedSource::new(fetcher, key.clone(), FileSource::new(dir)),
                Some(self.options()),
                Some(self.config.clone()),
            ),
        }
    }
}

/// The id in the Velopack manifest at `path`.
fn manifest_id(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(velopack::bundle::read_manifest_from_string(&text).ok()?.id)
}

/// `<x>.app/Contents/MacOS/<exe>`, with UpdateMac beside it and sq.version
/// there or in Resources; packages in velopack's macOS cache under `home`.
fn mac_layout(exe: &Path, home: &Path) -> Option<VelopackLocatorConfig> {
    let macos = exe.parent().filter(|dir| dir.ends_with("Contents/MacOS"))?;
    let contents = macos.parent()?;
    let app = contents
        .parent()
        .filter(|app| app.extension().is_some_and(|ext| ext == "app"))?;
    let update = macos.join("UpdateMac");
    let manifest = [
        macos.join("sq.version"),
        contents.join("Resources/sq.version"),
    ]
    .into_iter()
    .find(|path| path.is_file())?;
    if !update.is_file() {
        return None;
    }
    let id = manifest_id(&manifest)?;
    // Where velopack's own macOS locator keeps them.
    let packages = home
        .join("Library/Caches/velopack")
        .join(id)
        .join("packages");
    Some(VelopackLocatorConfig {
        RootAppDir: app.to_owned(),
        UpdateExePath: update,
        PackagesDir: packages,
        ManifestPath: manifest,
        CurrentBinaryDir: macos.to_owned(),
        IsPortable: true,
    })
}

/// `<root>\current\<exe>`, with sq.version beside it and `<root>\Update.exe`;
/// packages in `<root>\packages`. Setup.exe installs per user, so the root
/// (`%LocalAppData%\<id>`) is the user's to write.
fn windows_layout(exe: &Path) -> Option<VelopackLocatorConfig> {
    let current = exe
        .parent()
        .filter(|dir| dir.file_name().is_some_and(|name| name.eq_ignore_ascii_case("current")))?;
    let root = current.parent()?;
    let update = root.join("Update.exe");
    let manifest = current.join("sq.version");
    if !update.is_file() {
        return None;
    }
    manifest_id(&manifest)?;
    Some(VelopackLocatorConfig {
        RootAppDir: root.to_owned(),
        UpdateExePath: update,
        PackagesDir: root.join("packages"),
        ManifestPath: manifest,
        CurrentBinaryDir: current.to_owned(),
        IsPortable: root.join(".portable").exists(),
    })
}

/// `<mount>/usr/bin/<exe>`, with UpdateNix and sq.version beside it, and the
/// AppImage file itself at `appimage` (`$APPIMAGE`), which UpdateNix replaces;
/// packages in `/var/tmp/velopack/<id>/packages`, as velopack's own locator
/// keeps them.
fn appimage_layout(exe: &Path, appimage: &Path) -> Option<VelopackLocatorConfig> {
    let bin = exe.parent().filter(|dir| dir.ends_with("usr/bin"))?;
    let update = bin.join("UpdateNix");
    let manifest = bin.join("sq.version");
    if !update.is_file() || !appimage.is_absolute() || !appimage.is_file() {
        return None;
    }
    let id = manifest_id(&manifest)?;
    Some(VelopackLocatorConfig {
        RootAppDir: appimage.to_owned(),
        UpdateExePath: update,
        PackagesDir: Path::new("/var/tmp/velopack").join(id).join("packages"),
        ManifestPath: manifest,
        CurrentBinaryDir: bin.to_owned(),
        IsPortable: true,
    })
}

/// The policy for this process, from the build's values.
fn policy(install: Option<&Install>, perf: bool, wait_launch: bool, under_test: bool) -> Policy {
    Policy::decide(
        FEED,
        PUBLIC_KEY,
        install.is_some(),
        perf,
        wait_launch,
        under_test,
    )
}

/// The arguments as UTF-8, or None if any isn't.
pub(crate) fn utf8_args(args: impl IntoIterator<Item = OsString>) -> Option<Vec<String>> {
    args.into_iter().map(|arg| arg.into_string().ok()).collect()
}

/// Checks and downloads: the update ready to apply, if there is one.
pub(crate) fn check_and_download(
    manager: &UpdateManager,
    packages: &Path,
    mut say: impl FnMut(&str),
) -> Result<Option<UpdateInfo>, Error> {
    let update = match manager.check_for_updates()? {
        UpdateCheck::RemoteIsEmpty => {
            say("the verified feed lists no release");
            return Ok(None);
        }
        UpdateCheck::NoUpdateAvailable => {
            say("the verified feed has nothing newer");
            return Ok(None);
        }
        UpdateCheck::UpdateAvailable(update) => *update,
    };
    let target = &update.TargetFullRelease;
    say(&format!(
        "feed signature verified; {} is available ({}, {} bytes); downloading",
        target.Version, target.FileName, target.Size
    ));
    download_checked(manager, update, packages, say).map(Some)
}

/// Downloads `update` and checks the full package it leaves against the
/// signed feed. One rebuilt from deltas needn't be byte for byte the packed
/// full package, so when it fails the check, the full package is downloaded
/// instead and checked in turn.
pub(crate) fn download_checked(
    manager: &UpdateManager,
    update: UpdateInfo,
    packages: &Path,
    mut say: impl FnMut(&str),
) -> Result<UpdateInfo, Error> {
    if !update.DeltasToTarget.is_empty() {
        say(&format!(
            "trying {} delta package(s) on the cached {}",
            update.DeltasToTarget.len(),
            update
                .BaseRelease
                .as_ref()
                .map_or("base", |base| base.Version.as_str())
        ));
    }
    manager.download_updates(&update, None)?;
    match check_package(packages, &update.TargetFullRelease) {
        Ok(()) => {}
        Err(error) if !update.DeltasToTarget.is_empty() => {
            say(&format!("{error}; downloading the full package instead"));
            let full = UpdateInfo {
                BaseRelease: None,
                DeltasToTarget: Vec::new(),
                ..update
            };
            manager.download_updates(&full, None)?;
            check_package(packages, &full.TargetFullRelease)?;
            say("downloaded; the package's size and SHA-256 match the signed feed");
            return Ok(full);
        }
        Err(error) => return Err(error),
    }
    say("downloaded; the package's size and SHA-256 match the signed feed");
    Ok(update)
}

/// Checks the full package that `download_updates` left in `packages`
/// against the signed feed. Velopack checks each file it downloads, but not a
/// full package it rebuilt from a cached base and deltas. One that doesn't
/// match is deleted, so nothing applies it.
pub(crate) fn check_package(packages: &Path, target: &VelopackAsset) -> Result<(), Error> {
    let path = packages.join(velopack::misc::get_safe_filename(&target.FileName));
    let size = path.metadata()?.len();
    let (_, sha256) = velopack::misc::calculate_sha1_sha256(&path)?;
    if size == target.Size
        && !target.SHA256.is_empty()
        && sha256.eq_ignore_ascii_case(&target.SHA256)
    {
        return Ok(());
    }
    std::fs::remove_file(&path).ok();
    Err(Refusal::PackageMismatch {
        file: target.FileName.clone(),
    }
    .into())
}

/// Velopack's startup, the first thing `main` does: its `--veloapp-*` hook
/// launches are handled (and exit) here, before the command line is parsed.
/// A downloaded package is never applied here: only a check that just passed
/// applies one.
pub(crate) fn startup() {
    let Some(install) = Install::locate() else {
        return;
    };
    // `VelopackApp::build()` collects `env::args()`, which panics on an
    // argument that isn't UTF-8. Such a launch isn't one of Velopack's hook
    // launches, so it goes on to the CLI without Velopack.
    if utf8_args(std::env::args_os()).is_none() {
        return;
    }
    VelopackApp::build()
        .set_locator(install.config.clone())
        .set_auto_apply_on_startup(false)
        .run();
}

/// With `TENMOKU_VELOPACK_UPDATE_NOW=1`, checks, downloads and applies
/// headless, and exits: called once the command line says whether this is a
/// `--perf` or `--wait` launch, which never updates.
pub(crate) fn update_now_if_asked(perf: bool, wait_launch: bool) {
    if std::env::var_os(UPDATE_NOW).is_some_and(|value| value == "1") {
        // `main` never runs under the test executor.
        let install = Install::locate();
        std::process::exit(update_now(
            install.as_ref(),
            FEED,
            PUBLIC_KEY,
            perf,
            wait_launch,
        ));
    }
}

/// The headless check: each step and the outcome on stderr. 0 when there's
/// no update; on an update it never returns.
pub(crate) fn update_now(
    install: Option<&Install>,
    feed: Option<&str>,
    key: Option<&str>,
    perf: bool,
    wait_launch: bool,
) -> i32 {
    let say = |line: &str| eprintln!("velopack: {line}");
    let policy = Policy::decide(feed, key, install.is_some(), perf, wait_launch, false);
    let (feed, key) = match policy {
        Policy::On { feed, key } => (feed, key),
        Policy::Off(why) => {
            say(&format!("updates are off: {why}"));
            return 1;
        }
    };
    let Some(install) = install else { return 1 };
    let manager = match install.manager(&feed, &key) {
        Ok(manager) => manager,
        Err(error) => {
            say(&format!("error: {error}"));
            return 1;
        }
    };
    say(&format!(
        "installed {} at {}; checking {feed}",
        manager.get_current_version_as_string(),
        install.config.RootAppDir.display()
    ));
    match check_and_download(&manager, &install.config.PackagesDir, say) {
        Ok(None) => 0,
        Ok(Some(update)) => {
            say(&format!(
                "applying {} and restarting",
                update.TargetFullRelease.Version
            ));
            if let Err(error) = manager.apply_updates_and_restart(&update.TargetFullRelease) {
                say(&format!("error: {error}"));
            }
            1
        }
        Err(error) => {
            say(&format!("refused: {error}"));
            1
        }
    }
}

/// Whether a Check for Updates… is running.
static CHECKING: AtomicBool = AtomicBool::new(false);

/// Clears [`CHECKING`] when dropped, a panicking check included.
struct Checking;

impl Drop for Checking {
    fn drop(&mut self) {
        CHECKING.store(false, Ordering::SeqCst);
    }
}

/// Takes over Check for Updates… when [`Policy::decide`] says so, and says
/// whether it did; when not, Sparkle (or GitHub) has it as before. The check
/// and download run on the background executor; an update is then handed to
/// velopack's updater (UpdateMac, Update.exe or UpdateNix), which waits for this process to exit, and the app quits as the
/// Quit command does, asking about unsaved files.
pub(crate) fn start(perf: bool, wait_launch: bool, under_test: bool, cx: &mut App) -> bool {
    let install = Install::locate();
    let (feed, key) = match policy(install.as_ref(), perf, wait_launch, under_test) {
        Policy::On { feed, key } => (feed, key),
        Policy::Off(_) => return false,
    };
    let Some(install) = install else { return false };
    set_updater_hook(
        cx,
        UpdaterHook(Arc::new(move |cx| {
            check_in_background(install.clone(), feed.clone(), key.clone(), cx);
            true
        })),
    );
    true
}

fn check_in_background(install: Install, feed: Feed, key: PublicKey, cx: &mut App) {
    if CHECKING.swap(true, Ordering::SeqCst) {
        log::info!(target: "fastedit", "Velopack: a check is already running");
        return;
    }
    let checking = Checking;
    let work = cx.background_executor().spawn(async move {
        let outcome = (|| {
            let manager = install.manager(&feed, &key)?;
            let say = |line: &str| log::info!(target: "fastedit", "Velopack: {line}");
            let packages = &install.config.PackagesDir;
            Ok::<_, Error>(
                check_and_download(&manager, packages, say)?.map(|update| (manager, update)),
            )
        })();
        // Held until the main thread has handed any update to the updater.
        (checking, outcome)
    });
    cx.spawn(async move |cx| {
        let (_checking, outcome) = work.await;
        match outcome {
            Ok(None) => {}
            Ok(Some((manager, update))) => {
                let target = update.TargetFullRelease;
                if let Err(error) =
                    manager.wait_exit_then_apply_updates(&target, false, true, Vec::<String>::new())
                {
                    log::warn!(target: "fastedit", "Velopack: could not start its updater: {error}");
                    return;
                }
                log::info!(
                    target: "fastedit",
                    "Velopack: its updater applies {} once the app quits",
                    target.Version
                );
                cx.update(crate::quit);
            }
            Err(error) => {
                log::warn!(target: "fastedit", "Velopack: no update: {error}");
            }
        }
    })
    .detach();
}
