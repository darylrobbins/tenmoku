// SPDX-License-Identifier: Apache-2.0

//! The plugin crate runs components on its own `wasmtime::Engine`.

use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker},
};

#[test]
fn a_wat_component_runs_on_the_plugin_engine() {
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::new(
        &engine,
        r#"(component
            (core module $m
                (func (export "answer") (result i32) i32.const 42))
            (core instance $i (instantiate $m))
            (func (export "answer") (result u32) (canon lift (core func $i "answer"))))"#,
    )
    .unwrap();
    let mut store = Store::new(&engine, ());
    let instance = Linker::new(&engine)
        .instantiate(&mut store, &component)
        .unwrap();
    let answer = instance
        .get_typed_func::<(), (u32,)>(&mut store, "answer")
        .unwrap();
    assert_eq!(answer.call(&mut store, ()).unwrap(), (42,));
}
