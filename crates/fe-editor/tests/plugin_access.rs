//! Which editor commands a plugin may invoke, and what each needs of it.

use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

use fe_buffer::{Buffer, Origin};
use fe_command::{
    Actor, Capability, DispatchError, Grants, Invocation, PluginDenied, PluginPolicy, Registry,
};
use fe_editor::{
    EditorModel,
    commands::{self, Editor, EditorTarget, HeadlessHost},
};
use serde_json::json;

const PLUGIN: &str = "acme.tool";

/// The spec's allowlist: these, and no other editor command.
const PLUGIN_CALLABLE: [&str; 10] = [
    "editor.insert",
    "editor.newline",
    "editor.move",
    "editor.move_line_start",
    "editor.move_line_end",
    "editor.move_document_start",
    "editor.move_document_end",
    "editor.select_all",
    "editor.fold",
    "editor.unfold",
];

struct Policy(Grants);

impl PluginPolicy for Policy {
    fn grants(&self, plugin: &str) -> Option<Grants> {
        (plugin == PLUGIN).then(|| self.0.clone())
    }

    fn workspace_roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }
}

fn registry(grants: &[&str]) -> Registry<Editor> {
    let mut registry = commands::registry();
    let grants = grants.iter().map(|c| Capability::parse(c).unwrap());
    registry.set_plugin_policy(Arc::new(Policy(grants.collect())));
    registry
}

fn by_plugin(command: &str) -> Invocation {
    Invocation::of(command)
        .unwrap()
        .actor(Actor::Host(PLUGIN.into()))
        .context(commands::context())
}

#[test]
fn only_the_allowlisted_commands_are_plugin_callable() {
    let every = [
        "buffer.read",
        "buffer.write",
        "commands",
        "decorations",
        "status",
        "fs.read:workspace",
        "fs.write:workspace",
        "process.spawn:*",
        "net:*",
    ];
    let registry = registry(&every);
    let open: BTreeSet<&str> = registry
        .commands()
        .filter(|meta| registry.plugin_access(&meta.id).is_some())
        .map(|meta| meta.id.as_str())
        .collect();
    assert_eq!(open, BTreeSet::from(PLUGIN_CALLABLE));

    let closed: Vec<_> = registry
        .commands()
        .map(|meta| meta.id.clone())
        .filter(|id| !PLUGIN_CALLABLE.contains(&id.as_str()))
        .collect();
    for id in ["editor.save", "editor.undo", "editor.redo", "editor.paste"] {
        assert!(closed.iter().any(|closed| closed.as_str() == id), "{id}");
    }
    for id in closed {
        let mut model = EditorModel::new(Buffer::new("abc\n"), None);
        let mut host = HeadlessHost::new();
        host.clipboard = Some("pasted".into());
        let result = registry.dispatch(
            &mut EditorTarget::new(&mut model, &mut host),
            &by_plugin(id.as_str()),
        );
        assert!(
            matches!(
                result,
                Err(DispatchError::PluginDenied(PluginDenied::Undeclared { .. }))
            ),
            "{id}: {result:?}"
        );
        assert_eq!(model.text().to_string(), "abc\n", "{id}");
        assert_eq!(host.clipboard.as_deref(), Some("pasted"), "{id}");
    }
}

#[test]
fn editor_insert_by_a_plugin_needs_buffer_write_and_uses_the_host_origin() {
    let insert = by_plugin("editor.insert").args(json!({ "text": "hi " }));
    let is_plugin = |origin: &Origin| matches!(origin, Origin::Host(id) if &**id == PLUGIN);

    let denying = registry(&["commands"]);
    let mut model = EditorModel::new(Buffer::new("abc"), None);
    let mut host = HeadlessHost::new();
    let result = denying.dispatch(&mut EditorTarget::new(&mut model, &mut host), &insert);
    match result {
        Err(DispatchError::PluginDenied(PluginDenied::MissingCapability {
            capability, ..
        })) => assert_eq!(capability, Capability::BufferWrite),
        other => panic!("expected a denial for buffer.write, got {other:?}"),
    }
    assert_eq!(model.text().to_string(), "abc");

    let granting = registry(&["commands", "buffer.write"]);
    let dispatched = granting
        .dispatch(&mut EditorTarget::new(&mut model, &mut host), &insert)
        .unwrap();
    assert_eq!(dispatched.transactions.len(), 1, "{dispatched:?}");
    assert_eq!(model.text().to_string(), "hi abc");
    // The edit is the plugin's: undoing the plugin's edits takes it back.
    let undone = model.buffer_mut().undo(is_plugin).unwrap();
    assert_eq!(undone, Some(dispatched.transactions[0]));
    assert_eq!(model.text().to_string(), "abc");
}
