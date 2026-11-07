//! The Velopack spike's own checks: the feed signature in front of velopack,
//! the downloads it allows, and when the updater is on. Flat functions, so the
//! ids stay `velopack_update_tests::<name>`.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::Sender,
    },
};

use ring::digest::{SHA1_FOR_LEGACY_USE_ONLY, SHA256, digest};
use velopack::{
    Error, UpdateCheck, UpdateInfo, UpdateManager, VelopackAsset, VelopackAssetFeed,
    bundle::Manifest,
    locator::VelopackLocatorConfig,
    sources::{FileSource, UpdateSource},
};

use crate::{
    update_signature::{PrivateKey, PublicKey, SignatureError, generate, sign},
    velopack_update::{
        Feed, Fetcher, Install, Policy, Refusal, SignedSource, check_and_download, check_package,
        download_checked, update_now, utf8_args,
    },
};

const CHANNEL: &str = "spike";
const PACK_ID: &str = "TenmokuTest";

/// The inner source, counting what it's asked to download.
#[derive(Clone)]
struct Spy {
    inner: FileSource,
    downloads: Arc<AtomicUsize>,
}

impl UpdateSource for Spy {
    fn get_release_feed(&self, _: &str, _: &Manifest, _: &str) -> Result<VelopackAssetFeed, Error> {
        panic!("the signed source fetches the feed itself");
    }

    fn download_release_entry(
        &self,
        asset: &VelopackAsset,
        local_file: &Path,
        progress: Option<Sender<i16>>,
    ) -> Result<(), Error> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        self.inner
            .download_release_entry(asset, local_file, progress)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A local feed with one full package of 2.0.0, its install (1.0.0) and a
/// throwaway key pair.
struct Fixture {
    _dir: tempfile::TempDir,
    feed_dir: PathBuf,
    packages: PathBuf,
    config: VelopackLocatorConfig,
    asset: VelopackAsset,
    public: String,
    private: String,
    downloads: Arc<AtomicUsize>,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let feed_dir = dir.path().join("feed");
        let app = dir.path().join("Tenmoku.app");
        let macos = app.join("Contents/MacOS");
        let packages = dir.path().join("packages");
        std::fs::create_dir_all(&feed_dir).unwrap();
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::write(macos.join("UpdateMac"), b"not run").unwrap();
        let manifest = macos.join("sq.version");
        std::fs::write(
            &manifest,
            format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?><package><metadata><id>{PACK_ID}</id>\
                 <version>1.0.0</version><channel>{CHANNEL}</channel><mainExe>tenmoku</mainExe>\
                 </metadata></package>"
            ),
        )
        .unwrap();
        let package = b"a package's bytes; velopack checks only their size and hashes here";
        let file = format!("{PACK_ID}-2.0.0-full.nupkg");
        std::fs::write(feed_dir.join(&file), package).unwrap();
        let asset = VelopackAsset {
            PackageId: PACK_ID.into(),
            Version: "2.0.0".into(),
            Type: "Full".into(),
            FileName: file,
            SHA1: hex(digest(&SHA1_FOR_LEGACY_USE_ONLY, package).as_ref()),
            SHA256: hex(digest(&SHA256, package).as_ref()),
            Size: package.len() as u64,
            ..Default::default()
        };
        let (public, private) = generate();
        Fixture {
            config: VelopackLocatorConfig {
                RootAppDir: app,
                UpdateExePath: macos.join("UpdateMac"),
                PackagesDir: packages.clone(),
                ManifestPath: manifest,
                CurrentBinaryDir: macos,
                IsPortable: true,
            },
            _dir: dir,
            feed_dir,
            packages,
            asset,
            public,
            private,
            downloads: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn feed_path(&self) -> PathBuf {
        self.feed_dir.join(format!("releases.{CHANNEL}.json"))
    }

    fn sig_path(&self) -> PathBuf {
        self.feed_dir.join(format!("releases.{CHANNEL}.json.sig"))
    }

    /// Writes the feed with these assets, and its signature by `private`.
    fn publish(&self, assets: &[VelopackAsset], private: &str) {
        let json = serde_json::to_vec_pretty(&VelopackAssetFeed {
            Assets: assets.to_vec(),
        })
        .unwrap();
        std::fs::write(self.feed_path(), &json).unwrap();
        let key = PrivateKey::from_base64(private).unwrap();
        std::fs::write(self.sig_path(), sign(&key, &json) + "\n").unwrap();
    }

    fn source(&self) -> SignedSource<Spy> {
        self.source_with_key(&self.public)
    }

    fn source_with_key(&self, public: &str) -> SignedSource<Spy> {
        SignedSource::new(
            Fetcher::Dir(self.feed_dir.clone()),
            PublicKey::from_base64(public).unwrap(),
            Spy {
                inner: FileSource::new(&self.feed_dir),
                downloads: self.downloads.clone(),
            },
        )
    }

    fn manager(&self, source: SignedSource<Spy>) -> UpdateManager {
        UpdateManager::new(source, None, Some(self.config.clone())).unwrap()
    }

    fn downloads(&self) -> usize {
        self.downloads.load(Ordering::SeqCst)
    }

    fn downloaded(&self) -> PathBuf {
        self.packages.join(&self.asset.FileName)
    }

    /// The installed app's manifest, as velopack hands it to a source.
    fn app(&self) -> Manifest {
        self.config.load_manifest().unwrap()
    }

    /// Asserts a check through velopack fails, and that asking it to download
    /// the package anyway reaches no inner download.
    fn assert_refused_before_download(&self, source: SignedSource<Spy>) {
        let manager = self.manager(source);
        let error = match manager.check_for_updates() {
            Err(error) => error,
            Ok(_) => panic!("the check went through"),
        };
        assert!(
            error.to_string().contains("feed"),
            "a feed refusal, not {error}"
        );
        let update = velopack::UpdateInfo {
            TargetFullRelease: self.asset.clone(),
            ..Default::default()
        };
        assert!(manager.download_updates(&update, None).is_err());
        assert_eq!(self.downloads(), 0);
        assert!(!self.downloaded().exists());
    }
}

#[test]
fn good_signature_yields_the_feed_and_downloads() {
    let fixture = Fixture::new();
    fixture.publish(std::slice::from_ref(&fixture.asset), &fixture.private);

    let feed = fixture
        .source()
        .get_release_feed(CHANNEL, &fixture.app(), "")
        .unwrap();
    assert_eq!(feed.Assets.len(), 1);
    assert_eq!(feed.Assets[0].SHA256, fixture.asset.SHA256);

    let manager = fixture.manager(fixture.source());
    let update = match manager.check_for_updates().unwrap() {
        UpdateCheck::UpdateAvailable(update) => update,
        _ => panic!("no update found"),
    };
    assert_eq!(update.TargetFullRelease.Version, "2.0.0");
    manager.download_updates(&update, None).unwrap();
    assert_eq!(fixture.downloads(), 1);
    assert_eq!(
        std::fs::read(fixture.downloaded()).unwrap(),
        std::fs::read(fixture.feed_dir.join(&fixture.asset.FileName)).unwrap()
    );

    // The package left behind is checked against the signed feed too, as one
    // rebuilt from deltas isn't by velopack: the same length with a byte
    // changed is deleted.
    check_package(&fixture.packages, &fixture.asset).unwrap();
    let mut bytes = std::fs::read(fixture.downloaded()).unwrap();
    bytes[0] ^= 1;
    std::fs::write(fixture.downloaded(), &bytes).unwrap();
    let error = check_package(&fixture.packages, &fixture.asset).unwrap_err();
    assert!(
        error.to_string().contains("does not match the signed feed"),
        "{error}"
    );
    assert!(!fixture.downloaded().exists());

    // Through the whole check, from a fresh packages directory.
    let fresh = Fixture::new();
    fresh.publish(std::slice::from_ref(&fresh.asset), &fresh.private);
    let update = check_and_download(&fresh.manager(fresh.source()), &fresh.packages, |_| {})
        .unwrap()
        .expect("an update");
    assert_eq!(update.TargetFullRelease.FileName, fresh.asset.FileName);
    assert!(fresh.downloaded().exists());
}

#[test]
fn tampered_feed_is_refused_before_download() {
    let fixture = Fixture::new();
    fixture.publish(std::slice::from_ref(&fixture.asset), &fixture.private);
    // One byte, still a valid feed: the version 2.0.0 becomes 3.0.0.
    let mut bytes = std::fs::read(fixture.feed_path()).unwrap();
    let at = bytes.windows(5).position(|w| w == b"2.0.0").unwrap();
    bytes[at] = b'3';
    std::fs::write(fixture.feed_path(), &bytes).unwrap();
    assert!(serde_json::from_slice::<VelopackAssetFeed>(&bytes).is_ok());

    let source = fixture.source();
    assert!(matches!(
        source.check_feed(CHANNEL),
        Err(Refusal::BadSignature {
            error: SignatureError::Mismatch,
            ..
        })
    ));
    let error = source
        .get_release_feed(CHANNEL, &fixture.app(), "")
        .unwrap_err();
    assert!(error.to_string().contains("signature"), "{error}");
    fixture.assert_refused_before_download(source);
}

#[test]
fn wrong_key_is_refused_before_download() {
    let fixture = Fixture::new();
    let (_, other_private) = generate();
    fixture.publish(std::slice::from_ref(&fixture.asset), &other_private);

    let source = fixture.source();
    assert!(matches!(
        source.check_feed(CHANNEL),
        Err(Refusal::BadSignature {
            error: SignatureError::Mismatch,
            ..
        })
    ));
    fixture.assert_refused_before_download(source);
    // The same files pass with the key that signed them.
    let other_public = PrivateKey::from_base64(&other_private)
        .unwrap()
        .public_base64();
    assert!(
        fixture
            .source_with_key(&other_public)
            .check_feed(CHANNEL)
            .is_ok()
    );
}

#[test]
fn missing_signature_is_refused_before_download() {
    let fixture = Fixture::new();
    fixture.publish(std::slice::from_ref(&fixture.asset), &fixture.private);
    std::fs::remove_file(fixture.sig_path()).unwrap();

    let source = fixture.source();
    assert!(matches!(
        source.check_feed(CHANNEL),
        Err(Refusal::MissingSignature { .. })
    ));
    let error = source
        .get_release_feed(CHANNEL, &fixture.app(), "")
        .unwrap_err();
    assert!(error.to_string().contains("no signature"), "{error}");
    fixture.assert_refused_before_download(source);

    // An empty or garbled .sig is refused too.
    for sig in ["", "not base64!", "AAAA"] {
        std::fs::write(fixture.sig_path(), sig).unwrap();
        assert!(matches!(
            fixture.source().check_feed(CHANNEL),
            Err(Refusal::BadSignature { .. })
        ));
    }
}

#[test]
fn feed_asset_without_sha256_is_refused() {
    let fixture = Fixture::new();
    let unpinned = VelopackAsset {
        SHA256: String::new(),
        ..fixture.asset.clone()
    };
    // Signed by the right key, and its SHA-1 matches: velopack alone would
    // take it on SHA-1.
    fixture.publish(&[unpinned], &fixture.private);

    let source = fixture.source();
    assert!(matches!(
        source.check_feed(CHANNEL),
        Err(Refusal::NoSha256 { .. })
    ));
    let error = source
        .get_release_feed(CHANNEL, &fixture.app(), "")
        .unwrap_err();
    assert!(error.to_string().contains("SHA-256"), "{error}");
    fixture.assert_refused_before_download(source);
}

#[test]
fn asset_outside_the_verified_feed_is_not_downloaded() {
    let fixture = Fixture::new();
    // Before any feed has verified, nothing downloads.
    let scratch = fixture.packages.join("x.partial");
    std::fs::create_dir_all(&fixture.packages).unwrap();
    let source = fixture.source();
    assert!(
        source
            .download_release_entry(&fixture.asset, &scratch, None)
            .is_err()
    );

    fixture.publish(std::slice::from_ref(&fixture.asset), &fixture.private);
    source.check_feed(CHANNEL).unwrap();
    let others = [
        VelopackAsset {
            FileName: format!("{PACK_ID}-2.0.0-delta.nupkg"),
            Type: "Delta".into(),
            ..fixture.asset.clone()
        },
        VelopackAsset {
            SHA256: "00".repeat(32),
            ..fixture.asset.clone()
        },
        VelopackAsset {
            Size: fixture.asset.Size + 1,
            ..fixture.asset.clone()
        },
        VelopackAsset {
            SHA256: String::new(),
            ..fixture.asset.clone()
        },
    ];
    for other in &others {
        let error = source
            .download_release_entry(other, &scratch, None)
            .unwrap_err();
        assert!(
            error.to_string().contains("not in the last verified feed"),
            "{error}"
        );
    }
    assert_eq!(fixture.downloads(), 0);
    source
        .download_release_entry(&fixture.asset, &scratch, None)
        .unwrap();
    assert_eq!(fixture.downloads(), 1);

    // A later refused feed forgets the verified one.
    std::fs::remove_file(fixture.sig_path()).unwrap();
    assert!(source.check_feed(CHANNEL).is_err());
    assert!(
        source
            .download_release_entry(&fixture.asset, &scratch, None)
            .is_err()
    );
    assert_eq!(fixture.downloads(), 1);
}

#[test]
fn signer_and_verifier_agree() {
    let (public, private) = generate();
    let key = PrivateKey::from_base64(&private).unwrap();
    assert_eq!(key.public_base64(), public);
    let verifier = PublicKey::from_base64(&public).unwrap();
    let bytes = b"{\"Assets\":[]}";
    let sig = sign(&key, bytes);
    assert_eq!(verifier.verify(bytes, &sig), Ok(()));
    assert_eq!(verifier.verify(bytes, &format!("{sig}\r\n \n")), Ok(()));
    assert_eq!(
        verifier.verify(b"{\"Assets\":[ ]}", &sig),
        Err(SignatureError::Mismatch)
    );
    assert_eq!(verifier.verify(b"", &sig), Err(SignatureError::Mismatch));
    // Keys with surrounding whitespace, as files have.
    assert!(PrivateKey::from_base64(&format!(" {private}\n")).is_ok());
    assert_eq!(
        PublicKey::from_base64(&format!("{public}\n")).unwrap(),
        verifier
    );

    // Malformed keys and signatures are errors, never panics.
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    assert!(matches!(
        PublicKey::from_base64("not base64!"),
        Err(SignatureError::PublicKeyNotBase64(_))
    ));
    assert_eq!(
        PublicKey::from_base64(&STANDARD.encode([7u8; 31])),
        Err(SignatureError::PublicKeyLength(31))
    );
    assert_eq!(
        PublicKey::from_base64(""),
        Err(SignatureError::PublicKeyLength(0))
    );
    assert!(matches!(
        verifier.verify(bytes, "%%%"),
        Err(SignatureError::SignatureNotBase64(_))
    ));
    assert_eq!(
        verifier.verify(bytes, &STANDARD.encode([0u8; 63])),
        Err(SignatureError::SignatureLength(63))
    );
    assert_eq!(
        verifier.verify(bytes, &STANDARD.encode([0u8; 64])),
        Err(SignatureError::Mismatch)
    );
    // Only trailing whitespace is trimmed.
    assert!(matches!(
        verifier.verify(bytes, &format!(" {sig}")),
        Err(SignatureError::SignatureNotBase64(_))
    ));
    // A key that is no curve point verifies nothing.
    let not_a_point = PublicKey::from_base64(&STANDARD.encode([0xffu8; 32])).unwrap();
    assert!(not_a_point.verify(bytes, &sig).is_err());
    assert!(matches!(
        PrivateKey::from_base64("not base64!"),
        Err(SignatureError::PrivateKeyNotBase64(_))
    ));
    assert!(matches!(
        PrivateKey::from_base64(&STANDARD.encode([1u8; 31])),
        Err(SignatureError::PrivateKeyLength(31))
    ));
    assert!(matches!(
        PrivateKey::from_base64(&STANDARD.encode([1u8; 96])),
        Err(SignatureError::PrivateKeyLegacy)
    ));
    // The 64-byte seed-and-public layout signs as its seed does, and a
    // mismatched public half is refused.
    let seed = STANDARD.decode(&private).unwrap();
    let mut both = seed.clone();
    both.extend(STANDARD.decode(&public).unwrap());
    let joined = PrivateKey::from_base64(&STANDARD.encode(&both)).unwrap();
    assert_eq!(sign(&joined, bytes), sig);
    both[40] ^= 1;
    assert!(matches!(
        PrivateKey::from_base64(&STANDARD.encode(&both)),
        Err(SignatureError::PrivateKeyMismatch)
    ));
}

#[test]
fn policy_off_without_build_keys_or_install() {
    let (public, _) = generate();
    let key = PublicKey::from_base64(&public).unwrap();
    let dir = "/Users/someone/feed";
    let url = "https://updates.example.com/tenmoku";
    let on =
        |feed: &str, key: &str| Policy::decide(Some(feed), Some(key), true, false, false, false);
    let is_off = |policy: Policy| matches!(policy, Policy::Off(_));

    assert_eq!(
        on(dir, &public),
        Policy::On {
            feed: Feed::Dir(dir.into()),
            key: key.clone()
        }
    );
    assert_eq!(
        on(url, &public),
        Policy::On {
            feed: Feed::Https(url.into()),
            key
        }
    );
    assert!(matches!(
        on("HTTPS://updates.example.com", &public),
        Policy::On { .. }
    ));

    // No build values.
    assert!(is_off(Policy::decide(
        None, None, true, false, false, false
    )));
    assert!(is_off(Policy::decide(
        Some(dir),
        None,
        true,
        false,
        false,
        false
    )));
    assert!(is_off(Policy::decide(
        None,
        Some(&public),
        true,
        false,
        false,
        false
    )));
    // Not an install, or a launch that never updates.
    assert!(is_off(Policy::decide(
        Some(dir),
        Some(&public),
        false,
        false,
        false,
        false
    )));
    assert!(is_off(Policy::decide(
        Some(dir),
        Some(&public),
        true,
        true,
        false,
        false
    )));
    assert!(is_off(Policy::decide(
        Some(dir),
        Some(&public),
        true,
        false,
        true,
        false
    )));
    assert!(is_off(Policy::decide(
        Some(dir),
        Some(&public),
        true,
        false,
        false,
        true
    )));
    // Feeds that aren't https or an absolute path.
    for feed in [
        "http://updates.example.com/tenmoku",
        "HTTP://updates.example.com",
        "ftp://updates.example.com",
        "file:///Users/someone/feed",
        "https://",
        "https:///Users/someone/feed",
        "feed",
        "./feed",
        "~/feed",
        "",
    ] {
        assert!(is_off(on(feed, &public)), "{feed:?} turned it on");
    }
    // Keys that aren't 32 bytes of base64.
    for bad in ["", "not base64!", "AAAA"] {
        assert!(is_off(on(dir, bad)), "{bad:?} turned it on");
    }
}

/// Writes `bytes` into the feed as a 2.0.0 delta package, and its asset.
fn delta_asset(feed_dir: &Path, bytes: &[u8]) -> VelopackAsset {
    let file = format!("{PACK_ID}-2.0.0-delta.nupkg");
    std::fs::write(feed_dir.join(&file), bytes).unwrap();
    VelopackAsset {
        PackageId: PACK_ID.into(),
        Version: "2.0.0".into(),
        Type: "Delta".into(),
        FileName: file,
        SHA1: hex(digest(&SHA1_FOR_LEGACY_USE_ONLY, bytes).as_ref()),
        SHA256: hex(digest(&SHA256, bytes).as_ref()),
        Size: bytes.len() as u64,
        ..Default::default()
    }
}

#[cfg(unix)]
#[test]
fn delta_rebuild_that_fails_the_check_falls_back_to_the_full_package() {
    use std::os::unix::fs::PermissionsExt as _;

    for rebuilt_matches in [false, true] {
        let fixture = Fixture::new();
        let delta = delta_asset(&fixture.feed_dir, b"a delta's bytes");
        fixture.publish(&[fixture.asset.clone(), delta.clone()], &fixture.private);
        let full_package = fixture.feed_dir.join(&fixture.asset.FileName);
        // UpdateMac's `patch` stands in as a script: it writes either the
        // packed full package or other bytes of the same length as its output.
        let junk = fixture.feed_dir.join("junk");
        let mut other = std::fs::read(&full_package).unwrap();
        other[0] ^= 1;
        std::fs::write(&junk, &other).unwrap();
        let rebuilt = if rebuilt_matches {
            &full_package
        } else {
            &junk
        };
        let marker = fixture.feed_dir.join("patched");
        let script = &fixture.config.UpdateExePath;
        std::fs::write(
            script,
            format!(
                "#!/bin/sh\nout=\nwhile [ $# -gt 0 ]; do [ \"$1\" = --output ] && out=$2; shift; done\n\
: > '{}'\ncat '{}' > \"$out\"\n",
                marker.display(),
                rebuilt.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let manager = fixture.manager(fixture.source());
        // Verifies the feed; with no cached base, velopack itself offers the
        // full package only.
        let UpdateCheck::UpdateAvailable(offered) = manager.check_for_updates().unwrap() else {
            panic!("no update found");
        };
        assert!(offered.DeltasToTarget.is_empty());
        // What velopack offers once a base is cached.
        let update = UpdateInfo {
            TargetFullRelease: fixture.asset.clone(),
            BaseRelease: Some(VelopackAsset {
                Version: "1.0.0".into(),
                FileName: format!("{PACK_ID}-1.0.0-full.nupkg"),
                ..fixture.asset.clone()
            }),
            DeltasToTarget: vec![delta.clone()],
            IsDowngrade: false,
        };
        let mut said = Vec::new();
        let got = download_checked(&manager, update, &fixture.packages, |line| {
            said.push(line.to_owned())
        })
        .unwrap();

        assert!(marker.exists(), "the delta was never applied");
        assert_eq!(
            std::fs::read(fixture.downloaded()).unwrap(),
            std::fs::read(&full_package).unwrap()
        );
        let fell_back = said
            .iter()
            .any(|line| line.contains("downloading the full package instead"));
        if rebuilt_matches {
            // The delta alone; the rebuilt package is kept.
            assert_eq!(fixture.downloads(), 1, "{said:?}");
            assert_eq!(got.DeltasToTarget.len(), 1);
            assert!(!fell_back, "{said:?}");
        } else {
            // The delta, then the full package.
            assert_eq!(fixture.downloads(), 2, "{said:?}");
            assert!(got.DeltasToTarget.is_empty());
            assert!(fell_back, "{said:?}");
        }
    }
}

#[test]
fn excluded_launches_never_reach_the_check() {
    let fixture = Fixture::new();
    fixture.publish(std::slice::from_ref(&fixture.asset), &fixture.private);
    let install = Install {
        config: fixture.config.clone(),
    };
    let feed = fixture.feed_dir.to_str().unwrap();
    // A check writes velopack's staged-rollout id into the packages
    // directory, when there is one.
    std::fs::create_dir_all(&fixture.packages).unwrap();
    let checked = fixture.packages.join(".betaId");

    for (perf, wait_launch) in [(true, false), (false, true), (true, true)] {
        let code = update_now(
            Some(&install),
            Some(feed),
            Some(&fixture.public),
            perf,
            wait_launch,
        );
        assert_eq!(code, 1);
        assert!(
            !checked.exists(),
            "perf {perf}, wait {wait_launch} reached the check"
        );
        assert!(!fixture.downloaded().exists());
    }
    // The same call does reach it otherwise: here nothing is newer, so it
    // checks and stops.
    let older = VelopackAsset {
        Version: "0.5.0".into(),
        ..fixture.asset.clone()
    };
    fixture.publish(&[older], &fixture.private);
    assert_eq!(
        update_now(
            Some(&install),
            Some(feed),
            Some(&fixture.public),
            false,
            false
        ),
        0
    );
    assert!(checked.exists());
}

/// The app binary that this test binary was built beside.
fn tenmoku_binary() -> PathBuf {
    let mut candidates: Vec<PathBuf> = ["NEXTEST_BIN_EXE_tenmoku", "CARGO_BIN_EXE_tenmoku"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
        candidates.push(PathBuf::from(dir).join("debug/tenmoku"));
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/tenmoku"));
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .expect("the tenmoku binary, built with the tests")
}

#[cfg(target_os = "macos")]
#[test]
fn installed_launch_with_a_non_utf8_argument_does_not_panic() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt as _, process::Command};

    assert!(utf8_args(["a".into(), "b".into()]).is_some());
    assert!(utf8_args([OsStr::from_bytes(b"\xff").to_owned()]).is_none());

    // A Velopack install of this build: its binary, UpdateMac and sq.version.
    let dir = tempfile::tempdir().unwrap();
    let macos = dir.path().join("Tenmoku.app/Contents/MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    // A clone on APFS, not a byte copy.
    std::fs::copy(tenmoku_binary(), macos.join("tenmoku")).unwrap();
    std::fs::write(macos.join("UpdateMac"), b"not run").unwrap();
    std::fs::write(
        macos.join("sq.version"),
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?><package><metadata><id>{PACK_ID}</id>\
             <version>1.0.0</version><channel>{CHANNEL}</channel>\
             <mainExe>Contents/MacOS/tenmoku</mainExe></metadata></package>"
        ),
    )
    .unwrap();
    let launch = |args: &[&OsStr]| {
        Command::new(macos.join("tenmoku"))
            .args(args)
            .env("HOME", dir.path())
            .env("TENMOKU_NO_IPC", "1")
            .env("TENMOKU_VELOPACK_UPDATE_NOW", "1")
            .env_remove("RUST_LOG")
            .output()
            .unwrap()
    };

    // Velopack's startup runs for this install: a hook launch is its, and
    // exits at once.
    let hook = launch(&[OsStr::new("--veloapp-install"), OsStr::new("1.0.0")]);
    let hook_err = String::from_utf8_lossy(&hook.stderr);
    assert_eq!(hook.status.code(), Some(0), "{hook_err}");
    assert!(!hook_err.contains("velopack:"), "{hook_err}");

    // An argument that isn't UTF-8 reaches the CLI, and then the headless
    // check (off in a test build), instead of a panic.
    let odd = launch(&[OsStr::from_bytes(b"not-utf8-\xff.txt")]);
    let odd_err = String::from_utf8_lossy(&odd.stderr);
    assert!(!odd_err.contains("panicked"), "{odd_err}");
    assert_eq!(odd.status.code(), Some(1), "{odd_err}");
    assert!(odd_err.contains("velopack: updates are off"), "{odd_err}");
}
