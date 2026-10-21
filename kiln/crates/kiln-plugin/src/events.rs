// SPDX-License-Identifier: LicenseRef-Proprietary

//! Events the host delivers to plugins.
//!
//! Stub: every import is [`unimplemented`] until a later card of the plugin series fills it.

use crate::bindings::kiln::extension::events::{EventKinds, Host};
use crate::bindings::{Error, HostState, unimplemented};

/// The events slice of [`HostState`].
pub struct State;

impl Host for HostState {
    fn subscribe(&mut self, _kinds: EventKinds) -> Result<(), Error> {
        unimplemented("kiln:extension/events.subscribe")
    }
}
