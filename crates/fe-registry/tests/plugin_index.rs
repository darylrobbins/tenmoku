//! Plugins in the signed index: entries, and installs checked against them.

use std::{collections::BTreeMap, fs};

use fe_registry::{
    grammars::{Index, SCHEMA},
    plugins::{
        MAX_ARCHIVE_BYTES, MAX_ARCHIVE_ENTRIES, PluginEntry, Refused, verified_entry,
        verified_install,
    },
    signing::{generate_key, sig_path, sign_index},
};
use sha2::{Digest, Sha256};

const ARCHIVE: &[u8] = b"pretend this is a zip";

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn entry() -> PluginEntry {
    PluginEntry {
        id: "acme.cargo".into(),
        name: "Cargo".into(),
        version: "1.2.0".into(),
        api_minor: 1,
        license: "MIT".into(),
        file: "acme.cargo-1.2.0.zip".into(),
        sha256: hex(ARCHIVE),
        size: ARCHIVE.len() as u64,
        entries: 3,
        capabilities: vec!["fs.read:workspace".into(), "process.spawn:cargo".into()],
    }
}

fn index(serial: u64, entry: PluginEntry) -> Index {
    Index {
        schema: SCHEMA,
        serial,
        url_template: "https://example.com/{tag}/{file}".into(),
        packs: BTreeMap::new(),
        languages: BTreeMap::new(),
        plugins: BTreeMap::from([(entry.id.clone(), entry)]),
    }
}

/// A signed index: its bytes, its signature and the public key.
fn signed(index: &Index) -> (Vec<u8>, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let (public, private) = generate_key();
    let registry = dir.path().join("registry.toml");
    fs::write(
        &registry,
        format!("schema = 1\nurl_template = \"u\"\nindex_url = \"u\"\npublic_key = \"{public}\"\n[packs]\n"),
    )
    .unwrap();
    let path = dir.path().join("index.json");
    fs::write(&path, index.to_json()).unwrap();
    sign_index(&path, &registry, &private).unwrap();
    let signature = fs::read_to_string(sig_path(&path)).unwrap();
    (fs::read(&path).unwrap(), signature, public)
}

#[test]
fn plugin_index_entry_round_trips() {
    let index = index(5, entry());
    let (bytes, signature, public) = signed(&index);
    let parsed = Index::parse(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert_eq!(parsed, index);
    let (found, serial) = verified_entry(&bytes, &signature, &public, None, "acme.cargo").unwrap();
    assert_eq!((found, serial), (entry(), 5));
    // A grammar-only index prints and parses as before: no plugins key.
    let bare = Index {
        plugins: BTreeMap::new(),
        ..index
    };
    assert!(!bare.to_json().contains("plugins"));
    assert_eq!(Index::parse(&bare.to_json()).unwrap(), bare);
}

#[test]
fn plugin_install_hash_mismatch_refused() {
    let entry = entry();
    entry.verify_archive(ARCHIVE).unwrap();
    let mut swapped = ARCHIVE.to_vec();
    swapped[0] ^= 1;
    assert!(matches!(
        entry.verify_archive(&swapped),
        Err(Refused::Hash { .. })
    ));
    assert!(matches!(
        entry.verify_archive(b"short"),
        Err(Refused::Size { .. })
    ));
    assert!(entry.verify_capabilities(&["fs.read:workspace"]).is_ok());
    assert_eq!(
        entry.verify_capabilities(&["net:*"]),
        Err(Refused::UnlistedCapability("net:*".into()))
    );
}

#[test]
fn plugin_install_tampered_index_refused() {
    let (bytes, signature, public) = signed(&index(5, entry()));
    let text = String::from_utf8(bytes).unwrap();
    // The hash replaced by another's: the signature no longer matches.
    let forged = text.replace(&hex(ARCHIVE), &hex(b"evil"));
    assert_ne!(forged, text);
    let refused = verified_entry(forged.as_bytes(), &signature, &public, None, "acme.cargo");
    assert!(matches!(refused, Err(Refused::Signature(_))), "{refused:?}");
    // Another key's signature over the same bytes.
    let (other, ..) = generate_key();
    let refused = verified_entry(text.as_bytes(), &signature, &other, None, "acme.cargo");
    assert!(matches!(refused, Err(Refused::Signature(_))), "{refused:?}");
    // Signed but not listing it.
    let refused = verified_entry(text.as_bytes(), &signature, &public, None, "acme.other");
    assert_eq!(refused, Err(Refused::NotListed("acme.other".into())));
}

#[test]
fn plugin_index_rollback_refused() {
    let (bytes, signature, public) = signed(&index(5, entry()));
    let refused = verified_entry(&bytes, &signature, &public, Some(6), "acme.cargo");
    assert_eq!(
        refused,
        Err(Refused::Rollback {
            found: 5,
            cached: 6
        })
    );
    assert!(verified_entry(&bytes, &signature, &public, Some(5), "acme.cargo").is_ok());
}

#[test]
fn oversized_entries_are_not_an_index() {
    for (size, entries) in [
        (MAX_ARCHIVE_BYTES + 1, 3),
        (0, 3),
        (10, MAX_ARCHIVE_ENTRIES + 1),
        (10, 0),
    ] {
        let big = PluginEntry {
            size,
            entries,
            ..entry()
        };
        assert!(big.check_limits().is_err());
        assert!(Index::parse(&index(1, big).to_json()).is_err());
    }
}

#[test]
fn a_tampered_or_rolled_back_index_installs_nothing() {
    let (bytes, signature, public) = signed(&index(5, entry()));
    let id = "acme.cargo";
    let (found, serial) =
        verified_install(&bytes, &signature, &public, Some(5), id, ARCHIVE).unwrap();
    assert_eq!((found.id.as_str(), serial), (id, 5));
    let text = String::from_utf8(bytes.clone()).unwrap();
    let forged = text.replace(&hex(ARCHIVE), &hex(b"evil"));
    let evil = verified_install(forged.as_bytes(), &signature, &public, None, id, b"evil");
    assert!(matches!(evil, Err(Refused::Signature(_))), "{evil:?}");
    let old = verified_install(&bytes, &signature, &public, Some(6), id, ARCHIVE);
    assert!(matches!(old, Err(Refused::Rollback { .. })), "{old:?}");
    let swapped = verified_install(&bytes, &signature, &public, None, id, b"other bytes!");
    assert!(swapped.is_err());
}
