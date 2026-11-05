//! The `fastedit:editor/diagnostics` imports, run by the guest in `tests/guests/editor/` against
//! buffers held here: what the host keeps of each set, and what it refuses.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use fe_buffer::{Buffer, BufferSnapshot};
use fe_plugin::decorations::{BufferVersions, MAX_ENTRIES_PER_BUFFER};
use fe_plugin::diagnostics::{DiagnosticStore, State};
use fe_plugin::fastedit::editor::diagnostics::Host as _;
use fe_plugin::{EditorExtension, HostState};
use kiln_command::capability::{Capability, Grants};
use kiln_plugin::bindings::Error;
use kiln_plugin::bindings::kiln::extension::types::{Diagnostic, Range, Severity};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};

const PLUGIN: &str = "lint";

/// Open buffers; a plugin sees version 1 of each, as it was opened.
struct Buffers {
    open: HashMap<u64, Buffer>,
    shown: HashMap<u64, BufferSnapshot>,
}

impl BufferVersions for Buffers {
    fn now(&mut self, buffer: u64) -> Option<BufferSnapshot> {
        Some(self.open.get(&buffer)?.snapshot())
    }

    fn at(&mut self, buffer: u64, version: u64) -> Option<BufferSnapshot> {
        (version == 1).then(|| self.shown.get(&buffer).cloned())?
    }
}

fn buffers(id: u64, text: &str) -> Arc<Mutex<Buffers>> {
    let buffer = Buffer::new(text);
    Arc::new(Mutex::new(Buffers {
        shown: HashMap::from([(id, buffer.snapshot())]),
        open: HashMap::from([(id, buffer)]),
    }))
}

fn state(grants: &[&str], store: &DiagnosticStore, buffers: &Arc<Mutex<Buffers>>) -> HostState {
    let grants: Grants = grants
        .iter()
        .map(|c| Capability::parse(c).unwrap())
        .collect();
    HostState {
        diagnostics: State::new(PLUGIN, &grants, store, Box::new(Arc::clone(buffers))),
        ..HostState::default()
    }
}

/// The guest, run with its commands.
struct Guest {
    store: Store<HostState>,
    extension: EditorExtension,
}

impl Guest {
    fn new(state: HostState) -> Self {
        static COMPILED: OnceLock<(Engine, Component, Linker<HostState>)> = OnceLock::new();
        let (engine, component, linker) = COMPILED.get_or_init(|| {
            let mut config = Config::new();
            config.wasm_component_model(true);
            let engine = Engine::new(&config).unwrap();
            let component =
                Component::from_file(&engine, "tests/guests/editor/editor.wasm").unwrap();
            let mut linker = Linker::new(&engine);
            fe_plugin::add_to_linker(&mut linker).unwrap();
            linker.define_unknown_imports_as_traps(&component).unwrap();
            (engine, component, linker)
        });
        let mut store = Store::new(engine, state);
        let extension = EditorExtension::instantiate(&mut store, component, linker).unwrap();
        assert_eq!(extension.call_init(&mut store).unwrap(), Ok(()));
        Self { store, extension }
    }

    fn run(&mut self, command: &str, args: &str) -> Result<(), Error> {
        self.extension
            .kiln_extension_command_handler()
            .call_run_command(&mut self.store, &command.to_owned(), args)
            .unwrap()
    }
}

/// The messages of every diagnostic on buffer 1 now, by plugin's set.
fn messages(store: &DiagnosticStore, buffers: &Arc<Mutex<Buffers>>) -> Vec<(String, String)> {
    let now = buffers.lock().unwrap().open[&1].snapshot();
    let mut found: Vec<_> = (store.diagnostics(1, &now).into_iter())
        .map(|d| (d.set, d.diagnostic.message))
        .collect();
    found.sort();
    found
}

fn pair(set: &str, message: &str) -> (String, String) {
    (set.to_owned(), message.to_owned())
}

fn diagnostic(start: u64, end: u64) -> Diagnostic {
    Diagnostic {
        range: Range { start, end },
        severity: Severity::Warning,
        message: "m".into(),
        source: None,
        code: None,
    }
}

#[test]
fn publish_replaces_a_set_and_clear_removes_it() {
    let buffers = buffers(1, "let x = 1;\nlet y = 2;\n");
    let store = DiagnosticStore::new();
    let changes = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&changes);
    store.subscribe(move |buffer| {
        assert_eq!(buffer, 1);
        count.fetch_add(1, Ordering::SeqCst);
    });
    let mut guest = Guest::new(state(&["decorations"], &store, &buffers));

    guest
        .run(
            "diagnose",
            "1 1 style\n4 5 warning unused x\n15 16 error bad y",
        )
        .unwrap();
    guest.run("diagnose", "1 1 types\n0 3 hint a type").unwrap();
    assert_eq!(
        messages(&store, &buffers),
        [
            pair("style", "bad y"),
            pair("style", "unused x"),
            pair("types", "a type")
        ]
    );
    let now = buffers.lock().unwrap().open[&1].snapshot();
    let found = store.diagnostics(1, &now);
    assert_eq!(found[0].plugin, PLUGIN);
    assert_eq!(found[0].range, 4..5);
    assert_eq!(found[0].diagnostic.severity, Severity::Warning);

    // Publishing a set again replaces it alone.
    guest.run("diagnose", "1 1 style\n0 3 info only").unwrap();
    assert_eq!(
        messages(&store, &buffers),
        [pair("style", "only"), pair("types", "a type")]
    );
    // Clearing removes it alone, and clearing what isn't there is fine.
    guest.run("undiagnose", "1 style").unwrap();
    guest.run("undiagnose", "1 style").unwrap();
    assert_eq!(messages(&store, &buffers), [pair("types", "a type")]);
    assert_eq!(changes.load(Ordering::SeqCst), 4);
}

#[test]
fn diagnostics_need_the_decorations_capability() {
    let buffers = buffers(1, "text");
    let store = DiagnosticStore::new();
    let changes = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&changes);
    store.subscribe(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let mut guest = Guest::new(state(&["buffer.read", "status"], &store, &buffers));
    let publish = guest.run("diagnose", "1 1 set\n0 4 error bad");
    assert!(
        matches!(&publish, Err(Error::Denied(why)) if why.contains("decorations")),
        "{publish:?}"
    );
    let clear = guest.run("undiagnose", "1 set");
    assert!(matches!(clear, Err(Error::Denied(_))), "{clear:?}");
    assert!(messages(&store, &buffers).is_empty());
    assert_eq!(changes.load(Ordering::SeqCst), 0);

    // A store with no diagnostics slice doesn't implement them.
    let publish = HostState::default().publish(1, 1, "set".into(), Vec::new());
    assert!(
        matches!(publish, Err(Error::Unimplemented(_))),
        "{publish:?}"
    );
}

#[test]
fn a_bad_range_or_too_many_changes_nothing() {
    let buffers = buffers(1, "héllo");
    let store = DiagnosticStore::new();
    let mut state = state(&["decorations"], &store, &buffers);
    state
        .publish(1, 1, "set".into(), vec![diagnostic(0, 1)])
        .unwrap();
    let before = messages(&store, &buffers);

    for bad in [diagnostic(2, 3), diagnostic(0, 7), diagnostic(3, 1)] {
        let publish = state.publish(1, 1, "set".into(), vec![diagnostic(0, 1), bad]);
        assert!(
            matches!(publish, Err(Error::InvalidArgument(_))),
            "{publish:?}"
        );
    }
    let many = || vec![diagnostic(0, 1); MAX_ENTRIES_PER_BUFFER + 1];
    let publish = state.publish(1, 1, "set".into(), many());
    assert!(
        matches!(publish, Err(Error::InvalidArgument(_))),
        "{publish:?}"
    );
    // The cap is over all the plugin's sets on the buffer.
    state
        .publish(
            1,
            1,
            "full".into(),
            vec![diagnostic(0, 1); MAX_ENTRIES_PER_BUFFER - 1],
        )
        .unwrap();
    let publish = state.publish(1, 1, "set".into(), vec![diagnostic(0, 1); 2]);
    assert!(
        matches!(publish, Err(Error::InvalidArgument(_))),
        "{publish:?}"
    );

    let unknown = state.publish(1, 9, "set".into(), vec![diagnostic(0, 1)]);
    assert!(matches!(unknown, Err(Error::Conflict(_))), "{unknown:?}");
    let missing = state.publish(2, 1, "set".into(), vec![diagnostic(0, 1)]);
    assert!(matches!(missing, Err(Error::NotFound(_))), "{missing:?}");

    let after = messages(&store, &buffers);
    assert_eq!(after.len(), before.len() + MAX_ENTRIES_PER_BUFFER - 1);
    assert_eq!(after.iter().filter(|(set, _)| set == "set").count(), 1);
}

#[test]
fn unloading_or_closing_drops_the_plugins_diagnostics() {
    let buffers = buffers(1, "one two");
    let store = DiagnosticStore::new();
    let notified = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&notified);
    store.subscribe(move |buffer| sink.lock().unwrap().push(buffer));
    let mut state = state(&["decorations"], &store, &buffers);
    state
        .publish(1, 1, "set".into(), vec![diagnostic(0, 3)])
        .unwrap();
    notified.lock().unwrap().clear();

    store.unload(PLUGIN);
    assert!(messages(&store, &buffers).is_empty());
    assert_eq!(*notified.lock().unwrap(), [1]);
    // What the unloaded plugin's store still holds publishes nothing.
    let after = state.publish(1, 1, "set".into(), vec![diagnostic(0, 3)]);
    assert!(matches!(after, Err(Error::Denied(_))), "{after:?}");
    assert!(messages(&store, &buffers).is_empty());

    // Loaded again, then the buffer closes.
    let mut state = self::state(&["decorations"], &store, &buffers);
    state
        .publish(1, 1, "set".into(), vec![diagnostic(4, 7)])
        .unwrap();
    notified.lock().unwrap().clear();
    store.close(1);
    assert!(messages(&store, &buffers).is_empty());
    assert_eq!(*notified.lock().unwrap(), [1]);
    // Closing a buffer with nothing on it tells no one.
    store.close(1);
    store.unload(PLUGIN);
    assert_eq!(*notified.lock().unwrap(), [1]);
}

#[test]
fn reading_after_edits_compaction_or_from_an_older_snapshot_does_not_panic() {
    let buffers = buffers(1, "one two");
    let store = DiagnosticStore::new();
    let mut state = state(&["decorations"], &store, &buffers);
    let old = buffers.lock().unwrap().open[&1].snapshot();
    state
        .publish(1, 1, "set".into(), vec![diagnostic(4, 7)])
        .unwrap();

    // An edit before it moves it; once rebased, an older snapshot reads nothing, not a panic.
    buffers
        .lock()
        .unwrap()
        .open
        .get_mut(&1)
        .unwrap()
        .edit([(0..0, "zero ")], fe_buffer::Origin::Host("test".into()));
    let now = buffers.lock().unwrap().open[&1].snapshot();
    assert_eq!(store.diagnostics(1, &now)[0].range, 9..12);
    assert!(store.diagnostics(1, &old).is_empty());

    // Compacting the log to the anchors' version keeps them.
    state
        .publish(1, 1, "late".into(), vec![diagnostic(0, 4)])
        .unwrap();
    let floor = now.version();
    buffers
        .lock()
        .unwrap()
        .open
        .get_mut(&1)
        .unwrap()
        .compact_log(floor);
    let now = buffers.lock().unwrap().open[&1].snapshot();
    assert_eq!(store.diagnostics(1, &now).len(), 2);

    // Compacted past the anchors before any read: dropped, not a panic.
    buffers
        .lock()
        .unwrap()
        .open
        .get_mut(&1)
        .unwrap()
        .edit([(0..0, "x")], fe_buffer::Origin::Host("test".into()));
    let newest = buffers.lock().unwrap().open[&1].snapshot();
    buffers
        .lock()
        .unwrap()
        .open
        .get_mut(&1)
        .unwrap()
        .compact_log(newest.version());
    let newest = buffers.lock().unwrap().open[&1].snapshot();
    let found = store.diagnostics(1, &newest);
    assert!(found.is_empty(), "{found:?}");
}
