//! One wasmtime in the workspace: the plugin engine is tree-sitter's.

#[test]
fn the_plugin_engine_is_tree_sitters_wasmtime() {
    fn takes_tree_sitters(_: &tree_sitter::wasmtime::Engine) {}
    let engine = wasmtime::Engine::default();
    takes_tree_sitters(&engine);
}
