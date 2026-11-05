// SPDX-License-Identifier: Apache-2.0

//! Plugin API version compatibility: which plugins load on which host.
//!
//! A plugin names the API version it needs; the host serves that version when the major matches
//! and the minor is not above its own. wasmtime's linker already matches semver-compatible
//! interface versions, so an adapter is only needed for a breaking difference, and only for a
//! minor the host still supports. A major is dropped only after a deprecation window.

use kiln_core::ApiVersion;
use wasmtime::component::Linker;

/// The plugin API version this host implements.
pub const HOST_API: ApiVersion = ApiVersion { major: 1, minor: 0 };

/// Majors older than the host's that still load: each major's last supported host version.
/// A plugin of that major loads while the host is at or below it; none yet.
pub const DEPRECATED: &[(u32, ApiVersion)] = &[];

/// Why a plugin cannot load on this host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Incompatible {
    #[error("plugin needs API {needed}, but this host only provides {host}")]
    NeedsNewerHost {
        needed: ApiVersion,
        host: ApiVersion,
    },
    #[error("plugin needs API {needed}, but this host ({host}) does not support that major")]
    UnsupportedMajor {
        needed: ApiVersion,
        host: ApiVersion,
    },
}

/// A host's API version and its deprecation table.
#[derive(Clone, Copy, Debug)]
pub struct Compat {
    pub host: ApiVersion,
    pub deprecated: &'static [(u32, ApiVersion)],
}

impl Compat {
    /// This build's host.
    pub const CURRENT: Compat = Compat {
        host: HOST_API,
        deprecated: DEPRECATED,
    };

    /// Whether a plugin needing `needed` loads on this host.
    pub fn check(&self, needed: ApiVersion) -> Result<(), Incompatible> {
        let host = self.host;
        if needed.major == host.major {
            return if needed.minor <= host.minor {
                Ok(())
            } else {
                Err(Incompatible::NeedsNewerHost { needed, host })
            };
        }
        let in_window = self
            .deprecated
            .iter()
            .any(|&(major, last)| major == needed.major && host <= last);
        if in_window {
            Ok(())
        } else {
            Err(Incompatible::UnsupportedMajor { needed, host })
        }
    }
}

/// Links the interfaces of an older minor that a breaking change left behind.
pub struct Adapter<T: 'static> {
    /// The oldest API version this adapter serves.
    pub serves: ApiVersion,
    pub link: fn(&mut Linker<T>) -> wasmtime::Result<()>,
}

/// Registers `adapters` that serve a version below `compat`'s host and are still in its reach.
pub fn link_adapters<T: 'static>(
    compat: &Compat,
    adapters: &[Adapter<T>],
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    for adapter in adapters {
        if compat.check(adapter.serves).is_ok() && adapter.serves < compat.host {
            (adapter.link)(linker)?;
        }
    }
    Ok(())
}

/// Adds the imports older API versions need to `linker`; none yet.
pub fn link<T: 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    link_adapters(&Compat::CURRENT, &[], linker)
}
