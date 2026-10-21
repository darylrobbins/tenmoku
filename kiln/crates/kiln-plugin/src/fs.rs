// SPDX-License-Identifier: LicenseRef-Proprietary

//! The host's file-system imports for plugins.
//!
//! The interface is empty: files come through WASI preopens, which [`configure`] sets up from
//! the plugin's grants. Until a later card of the plugin series fills it, a plugin gets none.

use std::path::PathBuf;

use kiln_command::Grants;
use wasmtime_wasi::WasiCtxBuilder;

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

/// Preopens in `wasi` the directories `grants` reach in `workspace`.
///
/// Stub: preopens nothing.
pub fn configure(_wasi: &mut WasiCtxBuilder, _grants: &Grants, _workspace: &Workspace) {}
