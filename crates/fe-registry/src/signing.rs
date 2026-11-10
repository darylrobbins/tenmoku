//! The index's signature: Ed25519 over the exact bytes of `index.json`, in
//! the update feed's scheme (T1427, `crates/fastedit/src/update_signature.rs`).
//! `index.json.sig` holds the base64 of the 64-byte signature and a newline;
//! `registry.toml`'s `public_key` is the base64 of the 32-byte public key, and
//! a private key is the base64 of a 32-byte seed.
//!
//! [`sign_index`] refuses a private key whose public half isn't
//! `registry.toml`'s, before it writes anything, so CI can't publish an
//! index the app would refuse. [`verify_index`] is what the publish script
//! and a person checking a published index run.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{
    grammars::{self, Index, Registry},
    update_signature::{self, PrivateKey, PublicKey},
};

/// What signing or verifying refused, or couldn't read or write.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error(transparent)]
    Registry(grammars::Error),
    #[error("{}: no public_key, which signing and verifying need", path.display())]
    NoPublicKey { path: PathBuf },
    #[error("{}: public_key: {message}", path.display())]
    PublicKey { path: PathBuf, message: String },
    #[error("the private key: {0}")]
    PrivateKey(String),
    #[error(
        "the private key's public half is {key}, not {}'s public_key {registry}: the app \
         would refuse what it signs, so nothing was written",
        path.display()
    )]
    KeyMismatch {
        path: PathBuf,
        key: String,
        registry: String,
    },
    #[error("{}: {source}", path.display())]
    Index {
        path: PathBuf,
        source: grammars::Error,
    },
    #[error(
        "{} doesn't verify with {} against {}'s public_key: {message}",
        index.display(),
        sig.display(),
        registry.display()
    )]
    Signature {
        index: PathBuf,
        sig: PathBuf,
        registry: PathBuf,
        message: String,
    },
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

/// `<index>.sig`, where [`sign_index`] writes the signature.
pub fn sig_path(index: &Path) -> PathBuf {
    let mut path = index.as_os_str().to_owned();
    path.push(".sig");
    PathBuf::from(path)
}

/// `registry.toml`'s `public_key`, as given (base64) and parsed.
fn registry_key(registry_toml: &Path) -> Result<(String, PublicKey), Error> {
    let registry = Registry::read_file(registry_toml).map_err(Error::Registry)?;
    let text = registry.public_key.ok_or_else(|| Error::NoPublicKey {
        path: registry_toml.to_owned(),
    })?;
    let key = PublicKey::from_base64(&text).map_err(|error| Error::PublicKey {
        path: registry_toml.to_owned(),
        message: error.to_string(),
    })?;
    Ok((text.trim().to_owned(), key))
}

/// Reads `index` and checks that it parses as an index the app would take.
fn read_index(index: &Path) -> Result<Vec<u8>, Error> {
    let bytes = fs::read(index).map_err(io_error(index))?;
    let invalid = |source| Error::Index {
        path: index.to_owned(),
        source,
    };
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| invalid(grammars::Error::Index("it isn't UTF-8".to_owned())))?;
    Index::parse(text).map_err(invalid)?;
    Ok(bytes)
}

/// Signs the exact bytes of `index` with `private_key`, the base64 of a
/// 32-byte seed, and writes [`sig_path`] (whole: a temporary file renamed
/// over it). Before it writes, it checks that the index parses and that the
/// key's public half is `registry_toml`'s `public_key`. Returns the
/// signature's path.
pub fn sign_index(index: &Path, registry_toml: &Path, private_key: &str) -> Result<PathBuf, Error> {
    let (registry_text, registry) = registry_key(registry_toml)?;
    let key = PrivateKey::from_base64(private_key)
        .map_err(|error| Error::PrivateKey(error.to_string()))?;
    let public = key.public_base64();
    if PublicKey::from_base64(&public).ok().as_ref() != Some(&registry) {
        return Err(Error::KeyMismatch {
            path: registry_toml.to_owned(),
            key: public,
            registry: registry_text,
        });
    }
    let bytes = read_index(index)?;
    let signature = update_signature::sign(&key, &bytes);
    let sig = sig_path(index);
    // What the app will do with it, before it's written.
    registry
        .verify(&bytes, &signature)
        .map_err(|error| Error::Signature {
            index: index.to_owned(),
            sig: sig.clone(),
            registry: registry_toml.to_owned(),
            message: error.to_string(),
        })?;
    let mut temp = sig.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    fs::write(&temp, signature + "\n").map_err(io_error(&temp))?;
    fs::rename(&temp, &sig).map_err(io_error(&sig))?;
    Ok(sig)
}

/// Checks `sig` (by default [`sig_path`]) against the exact bytes of `index`
/// and `registry_toml`'s `public_key`, then that the index parses.
pub fn verify_index(index: &Path, sig: Option<&Path>, registry_toml: &Path) -> Result<(), Error> {
    let (_, registry) = registry_key(registry_toml)?;
    let sig = sig.map_or_else(|| sig_path(index), Path::to_owned);
    let bytes = fs::read(index).map_err(io_error(index))?;
    let sig_bytes = fs::read(&sig).map_err(io_error(&sig))?;
    let mismatch = |message: String| Error::Signature {
        index: index.to_owned(),
        sig: sig.clone(),
        registry: registry_toml.to_owned(),
        message,
    };
    let sig_text = std::str::from_utf8(&sig_bytes)
        .map_err(|_| mismatch("the signature file isn't text".to_owned()))?;
    registry
        .verify(&bytes, sig_text)
        .map_err(|error| mismatch(error.to_string()))?;
    read_index(index)?;
    Ok(())
}

/// A new key pair, each half as base64: (public, private). The private half
/// is a 32-byte seed, as [`sign_index`] takes it.
pub fn generate_key() -> (String, String) {
    update_signature::generate()
}
