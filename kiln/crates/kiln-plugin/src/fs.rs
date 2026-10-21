// SPDX-License-Identifier: LicenseRef-Proprietary

//! The host's file-system imports for plugins.
//!
//! The interface is empty: files come through WASI preopens. A later card of the plugin series
//! sets those up from the plugin's grants.

use crate::bindings::HostState;
use crate::bindings::kiln::extension::fs::Host;

/// The file-system slice of [`HostState`].
pub struct State;

impl Host for HostState {}
