//! fastedit's binding of the plugin host to the editor.
//!
//! The `fastedit:editor@1.0.0` world, in `wit/`, includes `kiln:extension/extension@1.0.0` and
//! adds the editor's imports. bindgen resolves the cross-package dependency by reading two
//! `path`s, kiln-plugin's `wit/` first and then this crate's, so the kiln package is read from
//! its one copy; there is no `wit/deps` copy to drift. `with` maps every `kiln:extension`
//! interface onto [`kiln_plugin::bindings`], so the editor world shares kiln's generated types
//! and runs kiln's imports against kiln's own [`kiln_plugin::bindings::HostState`] and the
//! `Host` impls its slices hold.
//!
//! [`HostState`] wraps that state with one `State` per editor slice, each defined in its module
//! (`editor`, `decorations`, `diagnostics`, `status`) beside its `Host` impl. A slice whose card has not landed
//! answers every call with [`kiln_plugin::bindings::unimplemented`].

pub mod decorations;
pub mod diagnostics;
pub mod editor;
pub mod net;
pub mod status;

use wasmtime::component::{HasSelf, Linker};

wasmtime::component::bindgen!({
    world: "fastedit:editor/editor-extension",
    path: ["../../kiln/crates/kiln-plugin/wit", "wit"],
    with: {
        "kiln:extension": kiln_plugin::bindings::kiln::extension,
    },
    additional_derives: [PartialEq, Eq],
});

/// The data an editor plugin's store holds.
#[derive(Default)]
pub struct HostState {
    pub kiln: kiln_plugin::bindings::HostState,
    pub editor: editor::State,
    pub decorations: decorations::State,
    pub diagnostics: diagnostics::State,
    pub status: status::State,
}

/// Adds every `kiln:extension` and `fastedit:editor` import to `linker`.
pub fn add_to_linker(linker: &mut Linker<HostState>) -> wasmtime::Result<()> {
    kiln_plugin::bindings::add_to_linker(linker, |state| &mut state.kiln)?;
    fastedit::editor::buffer::add_to_linker::<_, HasSelf<HostState>>(linker, |state| state)?;
    fastedit::editor::decorations::add_to_linker::<_, HasSelf<HostState>>(linker, |state| state)?;
    fastedit::editor::diagnostics::add_to_linker::<_, HasSelf<HostState>>(linker, |state| state)?;
    fastedit::editor::status::add_to_linker::<_, HasSelf<HostState>>(linker, |state| state)
}
