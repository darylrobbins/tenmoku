//! The fastedit:editor world's generated bindings take a plain kiln:extension component.

use std::sync::{Arc, Mutex};

use fe_plugin::{EditorExtension, HostState};
use kiln_plugin::bindings::Level;
use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker},
};

/// kiln-plugin's fixture that imports every kiln:extension interface and logs from `init`.
const STRINGS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../kiln/crates/kiln-plugin/tests/fixtures/strings.wat"
);

#[test]
fn the_editor_world_includes_the_kiln_world() {
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::from_file(&engine, STRINGS).unwrap();
    let mut linker = Linker::new(&engine);
    fe_plugin::add_to_linker(&mut linker).unwrap();

    let logged = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&logged);
    let state = HostState {
        kiln: kiln_plugin::bindings::HostState::new(Box::new(move |level, message| {
            sink.lock().unwrap().push((level, message));
        })),
        ..HostState::default()
    };
    let mut store = Store::new(&engine, state);
    let extension = EditorExtension::instantiate(&mut store, &component, &linker).unwrap();
    assert_eq!(extension.call_init(&mut store).unwrap(), Ok(()));
    // The kiln import ran against the kiln state inside the editor's.
    assert_eq!(
        *logged.lock().unwrap(),
        [(
            Level::Info,
            "strings cross: héllo, wörld ✓ 日本語".to_owned()
        )],
    );
    extension.call_shutdown(&mut store).unwrap();
}
