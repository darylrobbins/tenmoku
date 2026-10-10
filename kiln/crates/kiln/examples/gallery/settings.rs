// SPDX-License-Identifier: LicenseRef-Proprietary

//! The gallery's own settings, from the user's `settings.toml`, which kiln
//! reloads when it changes.

use kiln::serde::{Deserialize, Serialize};

/// The gallery's settings, as written in `settings.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(crate = "kiln::serde", default)]
pub(crate) struct GallerySettings {
    /// The title in the header.
    pub(crate) title: String,
}

impl Default for GallerySettings {
    fn default() -> Self {
        Self {
            title: crate::setup::APP_NAME.into(),
        }
    }
}
