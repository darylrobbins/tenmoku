//! Decorations plugins draw on the editor.
//!
//! Stub: every import is [`unimplemented`] until a later card of the plugin series fills it.

use kiln_plugin::bindings::{Error, unimplemented};

use crate::HostState;
use crate::fastedit::editor::decorations::{Decoration, GutterEntry, Host};

/// The decorations slice of [`HostState`].
#[derive(Default)]
pub struct State;

impl Host for HostState {
    fn set_decorations(
        &mut self,
        _buffer: u64,
        _version: u64,
        _set: String,
        _decorations: Vec<Decoration>,
    ) -> Result<(), Error> {
        unimplemented("fastedit:editor/decorations.set-decorations")
    }

    fn set_gutter(
        &mut self,
        _buffer: u64,
        _version: u64,
        _set: String,
        _entries: Vec<GutterEntry>,
    ) -> Result<(), Error> {
        unimplemented("fastedit:editor/decorations.set-gutter")
    }

    fn clear(&mut self, _buffer: u64, _set: String) -> Result<(), Error> {
        unimplemented("fastedit:editor/decorations.clear")
    }
}
