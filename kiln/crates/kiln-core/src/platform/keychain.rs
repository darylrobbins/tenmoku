// SPDX-License-Identifier: Apache-2.0

//! The platform keychain behind [`KeychainStore`](crate::KeychainStore):
//! which keyring-core store each OS uses, and which of its errors mean the
//! user declined.

use std::sync::Arc;

use keyring_core::CredentialStore;

pub(crate) use imp::{declined, store};

pub(crate) type PlatformError = dyn std::error::Error + Send + Sync;

#[cfg(target_os = "macos")]
mod imp {
    use super::*;

    pub(crate) fn store() -> keyring_core::Result<Arc<CredentialStore>> {
        Ok(apple_native_keyring_store::keychain::Store::new()?)
    }

    /// errSecUserCanceled, when the user clicks Deny on the Keychain's
    /// prompt, and errSecAuthFailed, when they don't give the password.
    pub(crate) fn declined(error: &PlatformError) -> bool {
        error
            .downcast_ref::<security_framework::base::Error>()
            .is_some_and(|error| matches!(error.code(), -128 | -25293))
    }
}

#[cfg(windows)]
mod imp {
    use super::*;

    pub(crate) fn store() -> keyring_core::Result<Arc<CredentialStore>> {
        Ok(windows_native_keyring_store::Store::new()?)
    }

    /// Credential Manager doesn't prompt, so there is nothing to decline.
    pub(crate) fn declined(_: &PlatformError) -> bool {
        false
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::*;

    pub(crate) fn store() -> keyring_core::Result<Arc<CredentialStore>> {
        Ok(zbus_secret_service_keyring_store::Store::new()?)
    }

    /// The Secret Service's unlock prompt was dismissed.
    pub(crate) fn declined(error: &PlatformError) -> bool {
        matches!(
            error.downcast_ref::<secret_service::Error>(),
            Some(secret_service::Error::Prompt)
        )
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use super::*;

    pub(crate) fn store() -> keyring_core::Result<Arc<CredentialStore>> {
        Err(keyring_core::Error::NotSupportedByStore(
            "this platform has no keychain".to_string(),
        ))
    }

    pub(crate) fn declined(_: &PlatformError) -> bool {
        false
    }
}
