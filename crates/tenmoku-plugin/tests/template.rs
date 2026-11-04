//! The plugin in `template/`, as its prebuilt `word_count.wasm` and `extension.toml`, against
//! fe-plugin's host. CI never builds guests, so this is what a WIT change that breaks the template
//! fails. After such a change, run `template/build.zsh`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fe_buffer::{Buffer, Origin};
use fe_plugin::editor::{BufferAccess, BufferText, Proposal};
use fe_plugin::status::StatusItems;
use fe_plugin::{EditorExtension, HostState, add_to_linker, editor, status};
use kiln_command::{Capability, Grants};
use kiln_core::{ApiVersion, ExtensionApi, load_manifest};
use kiln_plugin::bindings::Error;
use kiln_plugin::bindings::kiln::extension::events::Event;
use kiln_plugin::commands::{CommandHost, Dispatcher, Request};
use kiln_plugin::limits::Limits;
use kiln_plugin::manifest::{ExtensionOrigin, PluginManifest, plugin_kind};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};

const ID: &str = "example.word_count";
const BUFFER: u64 = 7;

fn template() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("template")
}

/// The template's manifest entry, read as the app reads a user's extension.
fn manifest() -> PluginManifest {
    let api = ExtensionApi {
        app: "tenmoku".into(),
        version: ApiVersion { major: 1, minor: 1 },
        reserved: vec![],
        kinds: vec![plugin_kind()],
        allowed_licenses: None,
        only_kinds: None,
    };
    let (manifest, diagnostics) = load_manifest(&template(), &api);
    assert_eq!(diagnostics, [], "the template's manifest has problems");
    PluginManifest::from_extension(&manifest.unwrap(), ExtensionOrigin::User)
        .unwrap()
        .expect("the template contributes a plugin")
}

/// One open buffer.
struct OneBuffer(Buffer);

impl BufferAccess for OneBuffer {
    fn snapshot(&mut self, buffer: u64) -> Option<BufferText> {
        (buffer == BUFFER).then(|| BufferText {
            version: 1,
            snapshot: self.0.snapshot(),
            path: None,
            language: None,
        })
    }

    fn edit(
        &mut self,
        _: u64,
        _: u64,
        _: Vec<(std::ops::Range<usize>, String)>,
        _: Origin,
    ) -> Result<u64, Error> {
        panic!("the template only reads");
    }

    fn propose(&mut self, _: u64, _: Proposal) -> Result<(), Error> {
        panic!("the template only reads");
    }
}

/// The template invokes no commands.
struct NoInvocations;

impl Dispatcher for NoInvocations {
    fn dispatch(&self, _: Request) {
        panic!("the template invokes no commands");
    }
}

#[test]
fn the_template_manifest_is_a_plugin() {
    let manifest = manifest();
    assert_eq!(manifest.id, ID);
    let directory = template().canonicalize().unwrap();
    assert_eq!(manifest.wasm, directory.join("word_count.wasm"));
    let capabilities: Grants = [
        Capability::BufferRead,
        Capability::Status,
        Capability::Commands,
    ]
    .into_iter()
    .collect();
    assert_eq!(manifest.capabilities, capabilities);
}

#[test]
fn the_template_loads_and_counts_words() {
    let manifest = manifest();
    let grants = &manifest.capabilities;

    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::from_file(&engine, &manifest.wasm).unwrap();
    let mut linker = Linker::new(&engine);
    add_to_linker(&mut linker).unwrap();
    // The guest's std imports WASI, which it never calls.
    linker.define_unknown_imports_as_traps(&component).unwrap();

    let commands = CommandHost::new([], Arc::new(NoInvocations));
    let items = StatusItems::new();
    let logged = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&logged);
    let mut kiln = kiln_plugin::bindings::HostState::new(Box::new(move |_, message| {
        sink.lock().unwrap().push(message);
    }));
    kiln.commands = kiln_plugin::commands::State::new(ID, grants, &commands).unwrap();
    let buffer = Arc::new(Mutex::new(OneBuffer(Buffer::new("one two three"))));
    let state = HostState {
        kiln,
        editor: editor::State::new(ID, grants, &Limits::default(), Box::new(buffer)),
        status: status::State::new(ID, grants, &items),
        ..HostState::default()
    };
    let mut store = Store::new(&engine, state);
    let plugin = EditorExtension::instantiate(&mut store, &component, &linker).unwrap();

    store.data_mut().kiln.commands.begin_init();
    assert_eq!(plugin.call_init(&mut store).unwrap(), Ok(()));
    store.data_mut().kiln.commands.end_init();
    let registered: Vec<_> = commands
        .registered(ID)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(registered, ["example.word_count.count"]);

    assert_eq!(items.items(), []);
    plugin
        .kiln_extension_event_handler()
        .call_handle_events(&mut store, &[Event::FocusChanged(Some(BUFFER))])
        .unwrap();
    let shown = items.item(ID, "words").expect("a status item");
    assert_eq!(shown.text, "3 words");

    let run = plugin.kiln_extension_command_handler().call_run_command(
        &mut store,
        &"example.word_count.count".to_owned(),
        "",
    );
    assert_eq!(run.unwrap(), Ok(()));
    assert_eq!(*logged.lock().unwrap(), ["3"]);

    plugin.call_shutdown(&mut store).unwrap();
}
