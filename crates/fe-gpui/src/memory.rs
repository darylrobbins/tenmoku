//! What an editor holds, for fe-workbench's memory report: its buffer's
//! text and undo history, which editors sharing the buffer hold together,
//! and its own syntax trees. Computed when asked, from the sizes each
//! crate states ([`fe_text::Text::heap_bytes_estimate`],
//! [`fe_buffer::Buffer::history_bytes_total`], [`fe_syntax::Syntax::tree_nodes`]).

use fe_buffer::BufferId;

use crate::EditorView;

/// An editor's share of the memory report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorMemory {
    /// The buffer it edits: another editor of it holds the same text and
    /// history, to count once.
    pub buffer: BufferId,
    /// The buffer's rope, estimated from its length.
    pub text_bytes: u64,
    /// The buffer's undo history, every origin's: its text exactly, its
    /// bookkeeping at a measured size.
    pub history_bytes: u64,
    /// The visible nodes of the editor's own trees (each editor parses for
    /// itself), injected languages' left out.
    pub tree_nodes: u64,
    /// Their memory, at [`fe_syntax::TREE_BYTES_PER_NODE`] a node.
    pub tree_bytes: u64,
}

impl EditorView {
    pub fn memory(&self) -> EditorMemory {
        let buffer = self.model.buffer();
        let tree_nodes = self.syntax().map_or(0, fe_syntax::Syntax::tree_nodes) as u64;
        EditorMemory {
            buffer: buffer.id(),
            text_bytes: buffer.text().heap_bytes_estimate() as u64,
            history_bytes: buffer.history_bytes_total() as u64,
            tree_nodes,
            tree_bytes: tree_nodes * fe_syntax::TREE_BYTES_PER_NODE as u64,
        }
    }
}
