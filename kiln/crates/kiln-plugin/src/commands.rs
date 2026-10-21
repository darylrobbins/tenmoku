// SPDX-License-Identifier: LicenseRef-Proprietary

//! Plugin-contributed commands on the kiln command registry.
//!
//! Stub: every import is [`unimplemented`] until a later card of the plugin series fills it.

use crate::bindings::kiln::extension::commands::{Command, Host};
use crate::bindings::{Error, HostState, unimplemented};

/// The commands slice of [`HostState`].
pub struct State;

impl Host for HostState {
    fn register(&mut self, _command: Command) -> Result<(), Error> {
        unimplemented("kiln:extension/commands.register")
    }

    fn invoke(&mut self, _id: String, _args: String) -> Result<(), Error> {
        unimplemented("kiln:extension/commands.invoke")
    }
}
