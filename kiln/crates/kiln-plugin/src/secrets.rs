// SPDX-License-Identifier: LicenseRef-Proprietary

//! The host's secret-storage imports for plugins.
//!
//! Stub: every import is [`unimplemented`] until a later card of the plugin series fills it.

use crate::bindings::kiln::extension::secrets::Host;
use crate::bindings::{Error, HostState, unimplemented};

/// The secrets slice of [`HostState`].
pub struct State;

impl Host for HostState {
    fn get(&mut self, _name: String) -> Result<Option<String>, Error> {
        unimplemented("kiln:extension/secrets.get")
    }

    fn set(&mut self, _name: String, _value: String) -> Result<(), Error> {
        unimplemented("kiln:extension/secrets.set")
    }

    fn delete(&mut self, _name: String) -> Result<(), Error> {
        unimplemented("kiln:extension/secrets.delete")
    }
}
