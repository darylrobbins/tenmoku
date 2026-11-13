// SPDX-License-Identifier: Apache-2.0

//! Serialises unit tests that switch the process-global locale.
//!
//! The catalogue is global to the test binary and its tests run on threads,
//! so a test that calls `set_locales` would flip the locale under another's
//! assertions. Take [`locale`] first in every such test; dropping the guard
//! returns the locale to the default.

use std::sync::{Mutex, MutexGuard, PoisonError};

static LOCALE: Mutex<()> = Mutex::new(());

/// Holds the lock for as long as it lives; resets the locales on drop.
pub(crate) struct LocaleGuard(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Drop for LocaleGuard {
    fn drop(&mut self) {
        kiln_l10n::set_locales(&[]);
    }
}

/// Takes the lock, ignoring poison from a test that failed holding it.
pub(crate) fn locale() -> LocaleGuard {
    LocaleGuard(LOCALE.lock().unwrap_or_else(PoisonError::into_inner))
}
