// SPDX-License-Identifier: Apache-2.0

//! The kiln:extension world's generated bindings, against hand-written `.wat` components.

use std::sync::{Arc, Mutex};

use kiln_plugin::bindings::{Error, Extension, HostState, Level};
use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker},
};

fn engine() -> Engine {
    let mut config = Config::new();
    config.wasm_component_model(true);
    Engine::new(&config).unwrap()
}

fn linker(engine: &Engine) -> Linker<HostState> {
    let mut linker = Linker::new(engine);
    kiln_plugin::bindings::add_to_linker(&mut linker, |state| state).unwrap();
    linker
}

#[test]
fn the_kiln_world_instantiates_the_empty_fixture() {
    let engine = engine();
    let component = Component::from_file(&engine, "tests/fixtures/empty.wat").unwrap();
    let mut store = Store::new(&engine, HostState::default());
    let extension = Extension::instantiate(&mut store, &component, &linker(&engine)).unwrap();
    assert_eq!(extension.call_init(&mut store).unwrap(), Ok(()));
    extension.call_shutdown(&mut store).unwrap();
}

#[test]
fn every_unimplemented_import_returns_the_unimplemented_error() {
    let engine = engine();
    let component = Component::from_file(&engine, "tests/fixtures/strings.wat").unwrap();
    let mut store = Store::new(&engine, HostState::default());
    let instance = linker(&engine).instantiate(&mut store, &component).unwrap();
    Extension::new(&mut store, &instance).unwrap();
    for (export, import) in [
        ("try-register", "kiln:extension/commands.register"),
        ("try-invoke", "kiln:extension/commands.invoke"),
        ("try-subscribe", "kiln:extension/events.subscribe"),
        ("try-get", "kiln:extension/secrets.get"),
        ("try-set", "kiln:extension/secrets.set"),
        ("try-delete", "kiln:extension/secrets.delete"),
    ] {
        let call = instance
            .get_typed_func::<(), (Error,)>(&mut store, export)
            .unwrap();
        let (error,) = call
            .call(&mut store, ())
            .unwrap_or_else(|trap| panic!("{export} trapped: {trap:?}"));
        assert_eq!(error, Error::Unimplemented(import.into()), "{export}");
    }
}

#[test]
fn a_spawn_with_no_grants_is_denied() {
    let engine = engine();
    let component = Component::from_file(&engine, "tests/fixtures/strings.wat").unwrap();
    let mut store = Store::new(&engine, HostState::default());
    let instance = linker(&engine).instantiate(&mut store, &component).unwrap();
    Extension::new(&mut store, &instance).unwrap();
    let call = instance
        .get_typed_func::<(), (Error,)>(&mut store, "try-spawn")
        .unwrap();
    let (error,) = call.call(&mut store, ()).unwrap();
    assert!(matches!(error, Error::Denied(_)), "{error:?}");
}

#[test]
fn strings_cross_the_boundary() {
    let engine = engine();
    let component = Component::from_file(&engine, "tests/fixtures/strings.wat").unwrap();
    let logged = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&logged);
    let state = HostState::new(Box::new(move |level, message| {
        sink.lock().unwrap().push((level, message));
    }));
    let mut store = Store::new(&engine, state);
    let extension = Extension::instantiate(&mut store, &component, &linker(&engine)).unwrap();
    assert_eq!(extension.call_init(&mut store).unwrap(), Ok(()));
    assert_eq!(
        *logged.lock().unwrap(),
        [(
            Level::Info,
            "strings cross: héllo, wörld ✓ 日本語".to_owned()
        )],
    );
}
