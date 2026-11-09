//! `editor.save` by a plugin: it writes the buffer to its file only for a
//! plugin granted `buffer.write` and `fs.write:workspace`.

use std::{path::PathBuf, sync::Arc};

use fe_command::{
    Actor, Capability, DispatchError, FsScope, Grants, Invocation, PluginDenied, PluginPolicy,
};
use fe_editor::{
    EditorModel,
    commands::{self, EditorTarget, HeadlessHost},
};
use serde_json::json;

const PLUGIN: &str = "acme.saver";

struct Policy(Grants);

impl PluginPolicy for Policy {
    fn grants(&self, plugin: &str) -> Option<Grants> {
        (plugin == PLUGIN).then(|| self.0.clone())
    }

    fn workspace_roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }
}

#[test]
fn editor_save_is_plugin_callable_with_its_grants() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved.txt");
    std::fs::write(&path, "old").unwrap();
    let mut model = EditorModel::open(&path).unwrap();
    let mut host = HeadlessHost::new();
    let save = Invocation::of("editor.save")
        .unwrap()
        .actor(Actor::Host(PLUGIN.into()))
        .context(commands::context());
    let write = Capability::BufferWrite;
    let fs_write = Capability::FsWrite(FsScope::Workspace);
    let mut run = |grants: &[&Capability], model: &mut EditorModel| {
        let mut registry = commands::registry();
        let grants = std::iter::once(&Capability::Commands).chain(grants.iter().copied());
        registry.set_plugin_policy(Arc::new(Policy(grants.cloned().collect())));
        registry.dispatch(&mut EditorTarget::new(model, &mut host), &save)
    };
    let edit = Invocation::of("editor.insert")
        .unwrap()
        .args(json!({ "text": "new " }))
        .context(commands::context());
    commands::registry()
        .dispatch(
            &mut EditorTarget::new(&mut model, &mut HeadlessHost::new()),
            &edit,
        )
        .unwrap();

    for (grants, missing) in [
        (vec![&write], &fs_write),
        (vec![&fs_write], &write),
        (vec![], &write),
    ] {
        match run(&grants, &mut model) {
            Err(DispatchError::PluginDenied(PluginDenied::MissingCapability {
                capability,
                ..
            })) => assert_eq!(&capability, missing, "{grants:?}"),
            other => panic!("{grants:?}: expected a denial for {missing}, got {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old", "{grants:?}");
        assert!(model.is_dirty());
    }

    run(&[&write, &fs_write], &mut model).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new old");
    assert!(!model.is_dirty());
}
