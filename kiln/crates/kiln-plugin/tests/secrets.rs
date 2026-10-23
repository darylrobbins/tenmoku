// SPDX-License-Identifier: LicenseRef-Proprietary

//! A plugin's secrets over a `MemoryStore` and an index file in a temporary directory, never the
//! real keychain.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use wasmtime::component::Instance;
use wasmtime::component::{Component, Lift, Linker};
use wasmtime::{Config, Engine, OptLevel, Store};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxView, WasiView};

use kiln_command::capability::{Capability, Grants};
use kiln_core::secrets::{MemoryStore, Secret, SecretError, SecretStore};
use kiln_plugin::bindings::kiln::extension::secrets::Host;
use kiln_plugin::bindings::{Error, HostState};
use kiln_plugin::fs::Workspace;
use kiln_plugin::runner::wasi_context;
use kiln_plugin::secrets::{PluginSecrets, State};

const SERVICE: &str = "com.example.notes";
const VALUE: &str = "sk-very-secret-value";

fn index_path(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("kiln-plugin-secrets-{}-{name}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    dir.join("plugin-secrets-index.toml")
}

fn plugin(store: &Arc<MemoryStore>, id: &str, index: &Path) -> PluginSecrets {
    PluginSecrets::new(store.clone(), SERVICE, id, index)
}

fn host(secrets: PluginSecrets) -> HostState {
    HostState {
        secrets: State::new(secrets),
        ..HostState::default()
    }
}

#[test]
fn a_plugin_sets_gets_and_deletes_its_own_secret() {
    let store = Arc::new(MemoryStore::new());
    let mut host = host(plugin(&store, "notes", &index_path("round-trip")));
    assert_eq!(host.get("key".into()), Ok(None));
    host.set("key".into(), VALUE.into()).unwrap();
    assert_eq!(host.get("key".into()), Ok(Some(VALUE.into())));
    host.set("key".into(), "second".into()).unwrap();
    assert_eq!(host.get("key".into()), Ok(Some("second".into())));
    host.delete("key".into()).unwrap();
    assert_eq!(host.get("key".into()), Ok(None));
    host.delete("key".into()).unwrap();
}

#[test]
fn entries_are_ext_id_dot_name_in_the_app_service() {
    let store = Arc::new(MemoryStore::new());
    let secrets = plugin(&store, "com.acme.fmt", &index_path("entries"));
    secrets.set("api-key_1", VALUE).unwrap();
    let entry = store.get(SERVICE, "ext.com.acme.fmt.api-key_1").unwrap();
    assert_eq!(entry, Some(Secret::new(VALUE)));
    assert_eq!(store.get("com.acme.fmt", "api-key_1"), Ok(None));
}

#[test]
fn one_plugin_cannot_read_anothers() {
    let store = Arc::new(MemoryStore::new());
    let index = index_path("isolation");
    let a = plugin(&store, "a", &index);
    let b = plugin(&store, "b", &index);
    let a_b = plugin(&store, "a.b", &index);
    a.set("token", VALUE).unwrap();
    a_b.set("c", "other").unwrap();
    assert_eq!(b.get("token"), Ok(None));
    assert_eq!(a.get("c"), Ok(None));
    assert!(matches!(a.get("b.c"), Err(Error::InvalidArgument(_))));
    b.delete("token").unwrap();
    assert_eq!(a.get("token"), Ok(Some(VALUE.into())));
}

#[test]
fn names_with_dots_or_bad_characters_are_refused() {
    let store = Arc::new(MemoryStore::new());
    let secrets = plugin(&store, "a", &index_path("names"));
    for name in ["a.b", "Upper", "", "sp ace", &"x".repeat(65)] {
        assert!(matches!(
            secrets.set(name, VALUE),
            Err(Error::InvalidArgument(_))
        ));
        assert!(matches!(secrets.get(name), Err(Error::InvalidArgument(_))));
        assert!(matches!(
            secrets.delete(name),
            Err(Error::InvalidArgument(_))
        ));
    }
    secrets.set(&"x".repeat(64), VALUE).unwrap();
}

#[test]
fn delete_all_removes_every_secret_of_one_plugin() {
    let store = Arc::new(MemoryStore::new());
    let index = index_path("delete-all");
    let a = plugin(&store, "a", &index);
    let b = plugin(&store, "b", &index);
    a.set("one", "1").unwrap();
    a.set("two", "2").unwrap();
    b.set("one", "3").unwrap();
    assert_eq!(a.names().unwrap(), ["one", "two"]);
    a.delete_all().unwrap();
    assert_eq!(a.names().unwrap(), Vec::<String>::new());
    assert_eq!(store.get(SERVICE, "ext.a.one"), Ok(None));
    assert_eq!(store.get(SERVICE, "ext.a.two"), Ok(None));
    assert_eq!(b.get("one"), Ok(Some("3".into())));
    assert!(!fs::read_to_string(&index).unwrap().contains("[plugins.a]"));
    b.delete_all().unwrap();
    assert!(!fs::read_to_string(&index).unwrap().contains("one"));
}

#[test]
fn the_index_survives_a_restart() {
    let store = Arc::new(MemoryStore::new());
    let index = index_path("restart");
    plugin(&store, "a", &index).set("one", "1").unwrap();
    let reopened = plugin(&store, "a", &index);
    assert_eq!(reopened.names().unwrap(), ["one"]);
    reopened.delete_all().unwrap();
    assert_eq!(store.get(SERVICE, "ext.a.one"), Ok(None));
}

/// A store that fails every call with `error`.
struct Failing(SecretError);

impl SecretStore for Failing {
    fn get(&self, _: &str, _: &str) -> Result<Option<Secret>, SecretError> {
        Err(self.0.clone())
    }
    fn set(&self, _: &str, _: &str, _: &str) -> Result<(), SecretError> {
        Err(self.0.clone())
    }
    fn delete(&self, _: &str, _: &str) -> Result<(), SecretError> {
        Err(self.0.clone())
    }
}

#[test]
fn an_unavailable_keychain_is_an_error() {
    for (error, denied) in [
        (SecretError::Unavailable("no session".into()), false),
        (SecretError::Denied, true),
    ] {
        let secrets = PluginSecrets::new(
            Arc::new(Failing(error)),
            SERVICE,
            "a",
            index_path("unavailable"),
        );
        let mut host = host(secrets);
        let results = [
            host.get("k".into()).map(drop),
            host.set("k".into(), VALUE.into()),
            host.delete("k".into()),
        ];
        for result in results {
            match result {
                Err(Error::Denied(_)) => assert!(denied),
                Err(Error::Failed(_)) => assert!(!denied),
                other => panic!("{other:?}"),
            }
        }
    }
}

/// Collects what `log` records, in one place for the whole test process.
struct Capture(Mutex<String>);

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        // Format before locking: on aarch64 cranelift logs from inside a `Debug` impl that
        // `record.args()` formats lazily, which would re-enter this logger with the lock held.
        let line = format!("{}\n", record.args());
        self.0.lock().unwrap().push_str(&line);
    }
    fn flush(&self) {}
}

#[test]
fn values_never_appear_in_debug_logs_or_the_wasi_environment() {
    static CAPTURE: Capture = Capture(Mutex::new(String::new()));
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    let store = Arc::new(MemoryStore::new());
    let index = index_path("redaction");
    let secrets = plugin(&store, "a", &index);
    let mut host = host(secrets.clone());
    host.set("key".into(), VALUE.into()).unwrap();
    let read = host.get("key".into()).unwrap();
    assert_eq!(read.as_deref(), Some(VALUE));

    let failing = PluginSecrets::new(
        Arc::new(Failing(SecretError::Other("boom".into()))),
        SERVICE,
        "a",
        &index,
    );
    let error = failing.set("key", VALUE).unwrap_err();

    let debugs = [
        format!("{secrets:?}"),
        format!("{:?}", host.secrets),
        format!("{store:?}"),
        format!("{:?}", store.get(SERVICE, "ext.a.key")),
        format!("{error:?}"),
        fs::read_to_string(&index).unwrap(),
        CAPTURE.0.lock().unwrap().clone(),
    ];
    for text in debugs {
        assert!(!text.contains(VALUE), "{text}");
    }

    // The environment a plugin gets from the production WASI context stays empty after secret
    // operations, whatever the grants.
    let grants: Grants = ["fs.write:workspace", "net:*"]
        .iter()
        .map(|c| Capability::parse(c).unwrap())
        .collect();
    let mut guest = Guest::load(wasi_context(&grants, &Workspace::default()));
    assert_eq!(
        guest.call::<Vec<(String, String)>>("environment"),
        Vec::<(String, String)>::new()
    );
}

/// The `fs` test guest, which reports its WASI environment.
struct Guest {
    store: Store<Ctx>,
    instance: Instance,
}

struct Ctx {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl WasiView for Ctx {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl Guest {
    fn load(wasi: WasiCtx) -> Self {
        let mut config = Config::new();
        config.cranelift_opt_level(OptLevel::None);
        let engine = Engine::new(&config).unwrap();
        let component = Component::from_file(&engine, "tests/guests/fs/fs.wasm").unwrap();
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker).unwrap();
        let mut store = Store::new(
            &engine,
            Ctx {
                wasi,
                table: ResourceTable::new(),
            },
        );
        let instance = linker.instantiate(&mut store, &component).unwrap();
        Self { store, instance }
    }

    fn call<R: Lift + 'static>(&mut self, name: &str) -> R {
        let func = self
            .instance
            .get_typed_func::<(), (R,)>(&mut self.store, name)
            .unwrap();
        func.call(&mut self.store, ()).unwrap().0
    }
}

/// A store whose `set` waits to be released, and which records the order of its calls.
struct Gated {
    inner: MemoryStore,
    events: Mutex<Vec<&'static str>>,
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl SecretStore for Gated {
    fn get(&self, service: &str, account: &str) -> Result<Option<Secret>, SecretError> {
        self.inner.get(service, account)
    }
    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), SecretError> {
        self.events.lock().unwrap().push("set-start");
        self.entered.send(()).unwrap();
        self.release.lock().unwrap().recv().unwrap();
        self.inner.set(service, account, secret)?;
        self.events.lock().unwrap().push("set-end");
        Ok(())
    }
    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError> {
        self.events.lock().unwrap().push("delete");
        self.inner.delete(service, account)
    }
}

#[test]
fn a_delete_waits_for_a_set_in_flight() {
    for all in [false, true] {
        let (entered_tx, entered) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let store = Arc::new(Gated {
            inner: MemoryStore::new(),
            events: Mutex::new(Vec::new()),
            entered: entered_tx,
            release: Mutex::new(release_rx),
        });
        let index = index_path(if all { "race-all" } else { "race" });
        let setter = PluginSecrets::new(store.clone(), SERVICE, "a", &index);
        let deleter = setter.clone();
        let set = thread::spawn(move || setter.set("k", VALUE));
        entered.recv().unwrap();
        let delete = thread::spawn(move || {
            if all {
                deleter.delete_all()
            } else {
                deleter.delete("k")
            }
        });
        thread::sleep(Duration::from_millis(50));
        release.send(()).unwrap();
        set.join().unwrap().unwrap();
        delete.join().unwrap().unwrap();
        let events = store.events.lock().unwrap().clone();
        assert_eq!(events, ["set-start", "set-end", "delete"], "all: {all}");
        let secrets = PluginSecrets::new(store.clone(), SERVICE, "a", &index);
        assert_eq!(secrets.get("k"), Ok(None));
        assert_eq!(secrets.names().unwrap(), Vec::<String>::new());
    }
}
