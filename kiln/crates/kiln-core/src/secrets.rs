// SPDX-License-Identifier: Apache-2.0

//! Secrets an app keeps out of its settings files, such as an API key: a
//! [`SecretStore`] over the platform keychain ([`KeychainStore`]: the macOS
//! Keychain, Windows' Credential Manager, the Secret Service on Linux and
//! the BSDs), or in memory ([`MemoryStore`]) for tests and for hosts with no
//! keychain.
//!
//! # Naming
//!
//! An entry is named by a service and an account. Every secret an app keeps
//! uses **the bare app id as the service**, [`AppId::reverse_dns`] (such as
//! `com.example.notes`), never a service per feature, and **a name per key
//! as the account**: the built-in chat's API key is `chat.anthropic`, and an
//! extension's secret would be `ext.<extension id>.<name>`. So one app's secrets are found
//! together under one service in the platform's keychain app. The store
//! takes both as arguments and doesn't check them; the convention is the
//! caller's.
//!
//! [`AppId::reverse_dns`]: crate::AppId::reverse_dns

use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex, PoisonError},
};

use keyring_core::{CredentialStore, Entry};

use crate::platform::keychain as platform;

/// A secret read from a store. Its `Debug` doesn't show the value, so a
/// secret logged by accident stays secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value, to hand to whatever needs it (an HTTP header).
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Why a store couldn't do what was asked. No variant carries a secret.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    /// The platform has no store (no Secret Service on a headless Linux
    /// box), or it refused (locked, or no session to ask in).
    #[error("no keychain is available: {0}")]
    Unavailable(String),
    /// The user declined access when the platform asked.
    #[error("access to the keychain was declined")]
    Denied,
    #[error("keychain error: {0}")]
    Other(String),
}

/// Where secrets are kept, by service and account (see the module doc for
/// the names to use).
pub trait SecretStore: Send + Sync {
    /// The secret, or `None` if there's no entry.
    fn get(&self, service: &str, account: &str) -> Result<Option<Secret>, SecretError>;
    /// Sets the secret, replacing any there was.
    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), SecretError>;
    /// Deletes the entry; deleting one that doesn't exist is `Ok`.
    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError>;
}

/// Secrets in memory, gone when it's dropped.
#[derive(Debug, Default)]
pub struct MemoryStore {
    entries: Mutex<HashMap<(String, String), Secret>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), Secret>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<Secret>, SecretError> {
        let key = (service.to_string(), account.to_string());
        Ok(self.entries().get(&key).cloned())
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), SecretError> {
        let key = (service.to_string(), account.to_string());
        self.entries().insert(key, Secret::new(secret));
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError> {
        let key = (service.to_string(), account.to_string());
        self.entries().remove(&key);
        Ok(())
    }
}

/// The platform keychain, through keyring-core: the macOS Keychain (the
/// login keychain's generic passwords), Windows' Credential Manager, and the
/// Secret Service over D-Bus on Linux and the BSDs.
///
/// Entries are made from this store's own value, never keyring-core's
/// process-wide default store, so nothing global is set.
pub struct KeychainStore {
    store: Arc<CredentialStore>,
}

impl fmt::Debug for KeychainStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeychainStore")
            .field("store", &self.store.vendor())
            .finish()
    }
}

impl KeychainStore {
    /// Opens the platform's store. On Linux this connects to the Secret
    /// Service; without one (a headless box, CI) it's
    /// [`SecretError::Unavailable`].
    pub fn new() -> Result<Self, SecretError> {
        let store = platform::store().map_err(|error| match from_keyring(error) {
            SecretError::Other(reason) => SecretError::Unavailable(reason),
            error => error,
        })?;
        Ok(Self { store })
    }

    fn entry(&self, service: &str, account: &str) -> Result<Entry, SecretError> {
        self.store
            .build(service, account, None)
            .map_err(from_keyring)
    }
}

impl SecretStore for KeychainStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<Secret>, SecretError> {
        match self.entry(service, account)?.get_password() {
            Ok(secret) => Ok(Some(Secret(secret))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(from_keyring(error)),
        }
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), SecretError> {
        self.entry(service, account)?
            .set_password(secret)
            .map_err(from_keyring)
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError> {
        match self.entry(service, account)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(from_keyring(error)),
        }
    }
}

/// keyring-core's errors as ours. Its `Display` never includes the secret
/// (bad encodings say only that they are), so the messages are safe to keep.
fn from_keyring(error: keyring_core::Error) -> SecretError {
    use keyring_core::Error as E;
    match error {
        E::NoStorageAccess(platform) if platform::declined(&*platform) => SecretError::Denied,
        E::PlatformFailure(platform) if platform::declined(&*platform) => SecretError::Denied,
        E::NoStorageAccess(platform) => SecretError::Unavailable(platform.to_string()),
        E::NoDefaultStore => SecretError::Unavailable(E::NoDefaultStore.to_string()),
        error => SecretError::Other(error.to_string()),
    }
}
