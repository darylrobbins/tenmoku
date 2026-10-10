// SPDX-License-Identifier: LicenseRef-Proprietary

//! What differs between operating systems, as data. This directory and
//! `kiln-platform`'s sources are the only places in kiln that branch on the
//! operating system (`scripts/check_kiln_boundary.py`); everything else
//! asks [`Os`], so it can be tested "as Linux" on any host.

pub(crate) mod fs;

use serde::{Deserialize, Serialize};

/// An operating system kiln runs on.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    MacOs,
    Linux,
    Windows,
}

impl Os {
    /// The one this process runs on. The BSDs and other Unixes count as
    /// Linux: they share its conventions (XDG directories, ctrl shortcuts).
    pub const fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}
