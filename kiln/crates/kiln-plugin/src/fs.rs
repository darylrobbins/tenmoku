// SPDX-License-Identifier: Apache-2.0

//! The host's file-system imports for plugins.
//!
//! The interface is empty: files come through WASI preopens, which [`configure`] sets up from
//! the plugin's grants. A plugin reaches the workspace's folders only with `fs.read:workspace`
//! or `fs.write:workspace`, and nothing else on disk at all. WASI resolves every path inside
//! the preopen it starts from, so neither `..` nor a symlink leads out of it.

use std::path::PathBuf;

use kiln_command::Grants;
use kiln_command::capability::{Capability, FsScope};
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

use crate::bindings::HostState;
use crate::bindings::kiln::extension::fs::Host;

/// The file-system slice of [`HostState`].
pub struct State;

impl Host for HostState {}

/// The folders open in the app, which `fs.read:workspace` and `fs.write:workspace` reach.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Workspace {
    pub roots: Vec<PathBuf>,
}

/// Sets `wasi`, a fresh builder, up as `grants` allow in `workspace`: each root preopened,
/// read-only for `fs.read:workspace` and read-write for `fs.write:workspace` (WASI has no
/// write-only directory), at its own absolute path, so the paths a plugin sees are the app's.
/// Without either grant nothing is preopened. Sockets and name lookups are denied. The rest is
/// left as [`WasiCtxBuilder::new`] makes it: no environment, no arguments, stdin closed, and
/// stdout and stderr discarded.
///
/// A root that can't be opened is left out, with a warning.
pub fn configure(wasi: &mut WasiCtxBuilder, grants: &Grants, workspace: &Workspace) {
    wasi.allow_tcp(false)
        .allow_udp(false)
        .allow_ip_name_lookup(false)
        .socket_addr_check(|_, _| Box::pin(async { false }));
    let perms = if grants.allows(&Capability::FsWrite(FsScope::Workspace)) {
        FsPerms::ReadWrite
    } else if grants.allows(&Capability::FsRead(FsScope::Workspace)) {
        FsPerms::ReadOnly
    } else {
        return;
    };
    for root in &workspace.roots {
        let Some(guest_path) = root.to_str().filter(|_| root.is_absolute()) else {
            log::warn!(
                "not preopening {}: not an absolute UTF-8 path",
                root.display()
            );
            continue;
        };
        if let Err(error) = wasi.preopened_dir(root, guest_path, perms) {
            log::warn!("not preopening {}: {error:#}", root.display());
        }
    }
}
