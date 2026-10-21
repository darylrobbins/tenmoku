//! What plugins may do to buffers and the editor.
//!
//! Stub: every import is [`unimplemented`] until a later card of the plugin series fills it.

use kiln_plugin::bindings::{Error, unimplemented};

use crate::HostState;
use crate::fastedit::editor::buffer::{Host, ProposedChange, Range, Snapshot, TextEdit};

/// The buffer slice of [`HostState`].
#[derive(Default)]
pub struct State;

impl Host for HostState {
    fn read(&mut self, _buffer: u64) -> Result<Snapshot, Error> {
        unimplemented("fastedit:editor/buffer.read")
    }

    fn read_ranges(
        &mut self,
        _buffer: u64,
        _version: u64,
        _ranges: Vec<Range>,
    ) -> Result<Vec<String>, Error> {
        unimplemented("fastedit:editor/buffer.read-ranges")
    }

    fn edit(&mut self, _buffer: u64, _version: u64, _edits: Vec<TextEdit>) -> Result<u64, Error> {
        unimplemented("fastedit:editor/buffer.edit")
    }

    fn propose(&mut self, _change: ProposedChange) -> Result<(), Error> {
        unimplemented("fastedit:editor/buffer.propose")
    }
}
