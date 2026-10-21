//! Status-bar items plugins contribute.
//!
//! Stub: every import is [`unimplemented`] until a later card of the plugin series fills it.

use kiln_plugin::bindings::{Error, unimplemented};

use crate::HostState;
use crate::fastedit::editor::status::{Host, Item};

/// The status slice of [`HostState`].
#[derive(Default)]
pub struct State;

impl Host for HostState {
    fn set_item(&mut self, _item: Item) -> Result<(), Error> {
        unimplemented("fastedit:editor/status.set-item")
    }

    fn remove_item(&mut self, _id: String) -> Result<(), Error> {
        unimplemented("fastedit:editor/status.remove-item")
    }
}
