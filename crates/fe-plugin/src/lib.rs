//! fastedit's binding of the plugin host to the editor.
//!
//! The `fastedit:editor@1.1.0` world, in `wit/`, includes `kiln:extension/extension@1.1.0` and
//! adds the editor's imports. bindgen resolves the cross-package dependency by reading two
//! `path`s, kiln-plugin's `wit/` first and then this crate's, so the kiln package is read from
//! its one copy; there is no `wit/deps` copy to drift. `with` maps every `kiln:extension`
//! interface onto [`kiln_plugin::bindings`], so the editor world shares kiln's generated types
//! and runs kiln's imports against kiln's own [`kiln_plugin::bindings::HostState`] and the
//! `Host` impls its slices hold.
//!
//! As kiln does, it keeps one generated world per supported API minor: each released minor's
//! frozen in `wit/released/<minor>/` beside kiln's, its world in a module of its own ([`v1_0`])
//! over the current types. [`check`] checks a component against the world of the minor it needs.
//!
//! [`HostState`] wraps that state with one `State` per editor slice, each defined in its module
//! (`editor`, `decorations`, `diagnostics`, `formatting`, `status`) beside its `Host` impl. A slice whose card has not landed
//! answers every call with [`kiln_plugin::bindings::unimplemented`].

pub mod decorations;
pub mod diagnostics;
pub mod editor;
pub mod formatting;
pub mod net;
pub mod status;

use kiln_plugin::compat::World;
use wasmtime::component::{HasSelf, InstancePre, Linker};

wasmtime::component::bindgen!({
    world: "fastedit:editor/editor-extension",
    path: ["../../kiln/crates/kiln-plugin/wit", "wit"],
    with: {
        "kiln:extension": kiln_plugin::bindings::kiln::extension,
    },
    additional_derives: [PartialEq, Eq],
});

/// The frozen `fastedit:editor@1.0.0` world, from both crates' `wit/released/1.0`, over the
/// current interfaces' types.
pub mod v1_0 {
    wasmtime::component::bindgen!({
        world: "fastedit:editor/editor-extension",
        path: [
            "../../kiln/crates/kiln-plugin/wit/released/1.0",
            "wit/released/1.0",
        ],
        with: {
            "kiln:extension": kiln_plugin::bindings::kiln::extension,
            "fastedit:editor": crate::fastedit::editor,
        },
    });
}

/// Checks that `pre`'s component has every export the editor world of `world`'s minor asks for,
/// typed as that world types it. Its imports were checked as `pre` was linked: one linker serves
/// every minor's, wasmtime matching `@1.0.0` against `@1.1.0`.
pub fn check<T: 'static>(world: World, pre: &InstancePre<T>) -> wasmtime::Result<()> {
    match world {
        World::V1_0 => v1_0::EditorExtensionPre::new(pre.clone()).map(drop),
        World::V1_1 => EditorExtensionPre::new(pre.clone()).map(drop),
    }
}

/// The data an editor plugin's store holds.
#[derive(Default)]
pub struct HostState {
    pub kiln: kiln_plugin::bindings::HostState,
    pub editor: editor::State,
    pub decorations: decorations::State,
    pub diagnostics: diagnostics::State,
    pub formatting: formatting::State,
    pub status: status::State,
}

/// Adds every `kiln:extension` and `fastedit:editor` import to `linker`.
pub fn add_to_linker(linker: &mut Linker<HostState>) -> wasmtime::Result<()> {
    kiln_plugin::bindings::add_to_linker(linker, |state| &mut state.kiln)?;
    fastedit::editor::buffer::add_to_linker::<_, HasSelf<HostState>>(linker, |state| state)?;
    fastedit::editor::decorations::add_to_linker::<_, HasSelf<HostState>>(linker, |state| state)?;
    fastedit::editor::diagnostics::add_to_linker::<_, HasSelf<HostState>>(linker, |state| state)?;
    fastedit::editor::formatting::add_to_linker::<_, HasSelf<HostState>>(linker, |state| state)?;
    fastedit::editor::status::add_to_linker::<_, HasSelf<HostState>>(linker, |state| state)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use kiln_plugin::compat::{
        World,
        released::{self, Files},
    };
    use wasmtime::component::{Component, Linker};
    use wasmtime::{Config, Engine};

    use super::{HostState, add_to_linker, check};

    /// `wit/released/1.0/`'s digest, as 1.0 was released. It never changes.
    const FROZEN_1_0: &str = "87f7ae02c9c4a753f23e2432d9543d38c2d26037275022f9c567f5234f99224f";

    fn read(path: &str) -> Files {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
        let files = released::read(&dir).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(!files.is_empty(), "{path} has no WIT");
        files
    }

    #[test]
    fn released_wit_1_0_matches_frozen_copy() {
        let frozen = read("wit/released/1.0");
        assert_eq!(
            released::digest(&frozen),
            FROZEN_1_0,
            "wit/released/1.0 changed: it is 1.0 as released"
        );
        let changes = released::changes(&frozen, &read("wit"));
        assert!(
            changes.is_empty(),
            "wit/ changed what 1.0 released; a minor only adds: {changes:#?}"
        );
    }

    /// Whether `component`, a WAT, links against the editor's imports and has the exports of
    /// each world, oldest first.
    fn checked(component: &str) -> Vec<bool> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        let engine = Engine::new(&config).unwrap();
        let component = Component::new(&engine, component).unwrap();
        let mut linker = Linker::<HostState>::new(&engine);
        add_to_linker(&mut linker).unwrap();
        let pre = linker.instantiate_pre(&component).unwrap();
        World::ALL
            .into_iter()
            .map(|world| check(world, &pre).is_ok())
            .collect()
    }

    #[test]
    fn each_minors_editor_world_checks_a_components_exports() {
        // A kiln plugin built for 1.0: the editor world adds only imports to kiln's.
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../kiln/crates/kiln-plugin/tests/fixtures/commands.wat");
        let built_for_1_0 = std::fs::read_to_string(path).unwrap();
        assert!(built_for_1_0.contains("command-handler@1.0.0"));
        assert_eq!(checked(&built_for_1_0), [true, true]);
        let built_for_1_1 = built_for_1_0.replace("@1.0.0", "@1.1.0");
        assert_eq!(checked(&built_for_1_1), [true, true]);
        let export = "(export \"kiln:extension/event-handler@1.0.0\"";
        assert!(built_for_1_0.contains(export));
        let unhandled = built_for_1_0.replace(export, "(export \"unrelated\"");
        assert_eq!(checked(&unhandled), [false, false]);
    }
}
