//! The editor commands a plugin may invoke, and what each needs of it
//! beyond `commands`. Every other editor command, the clipboard's and
//! undo's included, refuses plugins.
//!
//! `editor.save` writes the buffer to its own file, so it needs
//! `fs.write:workspace` as well as `buffer.write`: a host refuses it for an
//! untitled buffer or a file outside the workspace roots, which this
//! registry can't tell.

use fe_command::{Capability, FsScope, PluginAccess, Registry};

use crate::commands::Editor;

/// The editor commands open to plugins, with the capabilities they need.
const PLUGIN_CALLABLE: [(&str, &[Capability]); 11] = [
    ("editor.insert", &[Capability::BufferWrite]),
    (
        "editor.save",
        &[
            Capability::BufferWrite,
            Capability::FsWrite(FsScope::Workspace),
        ],
    ),
    ("editor.newline", &[Capability::BufferWrite]),
    ("editor.move", &[]),
    ("editor.move_line_start", &[]),
    ("editor.move_line_end", &[]),
    ("editor.move_document_start", &[]),
    ("editor.move_document_end", &[]),
    ("editor.select_all", &[]),
    ("editor.fold", &[]),
    ("editor.unfold", &[]),
];

/// Opens the [`PLUGIN_CALLABLE`] commands to plugins.
pub(crate) fn allow_plugins(registry: &mut Registry<Editor>) {
    for (id, capabilities) in PLUGIN_CALLABLE {
        let access = PluginAccess {
            capabilities: capabilities.to_vec(),
            path_args: Vec::new(),
        };
        registry
            .allow_plugins(id, access)
            .expect("plugin-callable editor commands are registered");
    }
}
