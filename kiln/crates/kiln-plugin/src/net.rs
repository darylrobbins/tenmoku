// SPDX-License-Identifier: LicenseRef-Proprietary

//! The host's network imports for plugins.
//!
//! Stub: every import is [`unimplemented`] until a later card of the plugin series fills it.

use crate::bindings::kiln::extension::net::{Host, Request, Response};
use crate::bindings::{Error, HostState, unimplemented};

/// The network slice of [`HostState`].
pub struct State;

impl Host for HostState {
    fn fetch(&mut self, _request: Request) -> Result<Response, Error> {
        unimplemented("kiln:extension/net.fetch")
    }
}
