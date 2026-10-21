// SPDX-License-Identifier: LicenseRef-Proprietary

//! The host's process-spawning imports for plugins.
//!
//! Stub: every import is [`unimplemented`] until a later card of the plugin series fills it.

use crate::bindings::kiln::extension::process::{Host, Output};
use crate::bindings::{Error, HostState, unimplemented};

/// The process slice of [`HostState`].
pub struct State;

impl Host for HostState {
    fn spawn(
        &mut self,
        _program: String,
        _args: Vec<String>,
        _stdin: Option<Vec<u8>>,
    ) -> Result<Output, Error> {
        unimplemented("kiln:extension/process.spawn")
    }
}
