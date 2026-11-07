//! Ed25519 signatures over the exact bytes of an update feed, in Sparkle's
//! formats: a signature is the base64 of its 64 raw bytes, a public key the
//! base64 of its 32 raw bytes, and a private key the base64 of its 32-byte
//! seed, which is what Sparkle 2's `generate_keys -x` exports for any key it
//! made. Only ring and base64, no velopack or GPUI, so the signer example
//! (`examples/sign_update_feed.rs`) shares this file.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ring::{
    rand::{SecureRandom, SystemRandom},
    signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey},
};

/// Why a key or signature was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SignatureError {
    #[error("the public key is not base64: {0}")]
    PublicKeyNotBase64(String),
    #[error("the public key is {0} bytes, not 32")]
    PublicKeyLength(usize),
    #[error("the private key is not base64: {0}")]
    PrivateKeyNotBase64(String),
    #[error(
        "the private key is Sparkle's older 96-byte format (an expanded key with no \
         seed), which can't sign here; make a new key"
    )]
    PrivateKeyLegacy,
    #[error("the private key's last 32 bytes are not the public key of its seed")]
    PrivateKeyMismatch,
    #[error("the private key is {0} bytes, not a 32-byte seed")]
    PrivateKeyLength(usize),
    #[error("the signature is not base64: {0}")]
    SignatureNotBase64(String),
    #[error("the signature is {0} bytes, not 64")]
    SignatureLength(usize),
    #[error(
        "the signature does not verify against the public key (changed bytes or a different key)"
    )]
    Mismatch,
}

/// A public key to check signatures with.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PublicKey([u8; 32]);

impl std::fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PublicKey({})", STANDARD.encode(self.0))
    }
}

impl PublicKey {
    /// The key from the base64 of its 32 bytes; surrounding whitespace is
    /// ignored.
    pub(crate) fn from_base64(text: &str) -> Result<PublicKey, SignatureError> {
        let bytes = STANDARD
            .decode(text.trim())
            .map_err(|error| SignatureError::PublicKeyNotBase64(error.to_string()))?;
        let bytes: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| SignatureError::PublicKeyLength(bytes.len()))?;
        Ok(PublicKey(bytes))
    }

    /// Checks `sig_text`, the base64 of a 64-byte signature with any trailing
    /// whitespace (a file's last newline), against exactly `bytes`.
    pub(crate) fn verify(&self, bytes: &[u8], sig_text: &str) -> Result<(), SignatureError> {
        let sig = STANDARD
            .decode(sig_text.trim_end())
            .map_err(|error| SignatureError::SignatureNotBase64(error.to_string()))?;
        if sig.len() != 64 {
            return Err(SignatureError::SignatureLength(sig.len()));
        }
        UnparsedPublicKey::new(&ED25519, &self.0)
            .verify(bytes, &sig)
            .map_err(|_| SignatureError::Mismatch)
    }
}

/// A signing key, held as its seed.
pub(crate) struct PrivateKey(Ed25519KeyPair);

impl PrivateKey {
    /// The key from base64 text, surrounding whitespace ignored: a 32-byte
    /// seed (ours, and Sparkle 2's export), or a 64-byte seed followed by its
    /// public key (libsodium's layout), whose halves must agree. Sparkle's
    /// older 96-byte keys hold no seed, so they're refused by name.
    pub(crate) fn from_base64(text: &str) -> Result<PrivateKey, SignatureError> {
        let bytes = STANDARD
            .decode(text.trim())
            .map_err(|error| SignatureError::PrivateKeyNotBase64(error.to_string()))?;
        let pair = match bytes.len() {
            32 => Ed25519KeyPair::from_seed_unchecked(&bytes)
                .map_err(|_| SignatureError::PrivateKeyLength(32))?,
            64 => Ed25519KeyPair::from_seed_and_public_key(&bytes[..32], &bytes[32..])
                .map_err(|_| SignatureError::PrivateKeyMismatch)?,
            96 => return Err(SignatureError::PrivateKeyLegacy),
            other => return Err(SignatureError::PrivateKeyLength(other)),
        };
        Ok(PrivateKey(pair))
    }

    /// The base64 of the matching public key.
    pub(crate) fn public_base64(&self) -> String {
        STANDARD.encode(self.0.public_key().as_ref())
    }
}

/// The base64 signature of exactly `bytes`.
pub(crate) fn sign(seed: &PrivateKey, bytes: &[u8]) -> String {
    STANDARD.encode(seed.0.sign(bytes).as_ref())
}

/// A new key pair from the system's random source: (public, private), each as
/// base64.
pub(crate) fn generate() -> (String, String) {
    let mut seed = [0u8; 32];
    SystemRandom::new()
        .fill(&mut seed)
        .expect("the system's random source");
    let private = STANDARD.encode(seed);
    let key = PrivateKey::from_base64(&private).expect("a 32-byte seed");
    (key.public_base64(), private)
}
