// SPDX-License-Identifier: LicenseRef-Proprietary

//! A plugin's commands: it registers them in its own namespace during `init`, the host runs
//! them on the plugin's thread under its deadline, and what the plugin invokes reaches the
//! app's registry as `Host` from `Script`, authorized by the plugin's grants and observed like
//! a user's. Calls that could stall run under [`bounded`].

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc, Barrier, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use kiln_command::{
    Actor, Capability, CommandId, CommandTarget, ContextPredicate, Dispatched, Effect, Grants,
    Invocation, PluginAccess, PluginPolicy, Registry, Source,
};
use kiln_plugin::{
    bindings::{self, Error, Extension, HostState, Level},
    commands::{AttachError, CommandHost, Dispatcher, PluginCommand, PluginHandle, Request, State},
    engine::PluginEngine,
    limits::Limits,
    runner::{LoadError, Plugin},
};
use serde_json::{Value, json};
use wasmtime::{
    Engine, Store,
    component::{Component, Instance, Linker},
};

const FIXTURE: &str = "tests/fixtures/commands.wat";

/// The fixture's plugin id: its commands are `acme.tool.*`.
const PLUGIN: &str = "acme.tool";

/// The app's commands: `test.read` open to any plugin with `commands`, `test.write` to one
/// with `buffer.write` too, `test.secret` to none, and `acme.tool.builtin`, a built-in in the
/// fixture's namespace.
const BUILTIN: [&str; 4] = [
    "test.read",
    "test.write",
    "test.secret",
    "acme.tool.builtin",
];

/// Commands that log what ran, and as whom.
struct Logged;

impl CommandTarget for Logged {
    type Tx = ();
    type Target<'a> = Vec<String>;
}

fn registry() -> Registry<Logged> {
    let mut registry = Registry::new();
    for id in BUILTIN {
        registry
            .register(id, id, move |log: &mut Vec<String>, actor: &Actor| {
                log.push(format!("{actor:?} ran {id}"));
                Ok(Dispatched::new(Effect::Changed))
            })
            .unwrap();
    }
    registry
        .allow_plugins("test.read", PluginAccess::default())
        .unwrap();
    registry
        .allow_plugins(
            "test.write",
            PluginAccess {
                capabilities: vec![Capability::BufferWrite],
                path_args: Vec::new(),
            },
        )
        .unwrap();
    registry
}

/// Hands each request to the app's thread.
struct ToApp(Mutex<Sender<Request>>);

impl Dispatcher for ToApp {
    fn dispatch(&self, request: Request) {
        self.0.lock().unwrap().send(request).unwrap();
    }
}

/// Keeps every request and answers none.
#[derive(Default)]
struct Withheld(Mutex<Vec<Request>>);

impl Dispatcher for Withheld {
    fn dispatch(&self, request: Request) {
        self.0.lock().unwrap().push(request);
    }
}

impl Withheld {
    fn cancelled(&self) -> Vec<bool> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(Request::is_cancelled)
            .collect()
    }
}

/// A host whose app never answers, and what it holds.
fn withholding() -> (CommandHost, Arc<Withheld>) {
    let withheld = Arc::new(Withheld::default());
    let builtin = BUILTIN.map(|id| CommandId::parse(id).unwrap());
    let host = CommandHost::new(builtin, Arc::clone(&withheld) as Arc<dyn Dispatcher>);
    (host, withheld)
}

/// What the app's thread saw: each invocation its registry's observer was told of, and what
/// each command that ran logged.
#[derive(Clone, Default)]
struct Seen {
    observed: Arc<Mutex<Vec<Invocation>>>,
    ran: Arc<Mutex<Vec<String>>>,
}

impl Seen {
    fn observed(&self) -> Vec<Invocation> {
        self.observed.lock().unwrap().clone()
    }

    fn ran(&self) -> Vec<String> {
        self.ran.lock().unwrap().clone()
    }
}

/// A host whose app thread owns [`registry`], with the host as its plugin policy.
fn app() -> (CommandHost, Seen) {
    let (sender, asked) = mpsc::channel::<Request>();
    let builtin = BUILTIN.map(|id| CommandId::parse(id).unwrap());
    let host = CommandHost::new(builtin, Arc::new(ToApp(Mutex::new(sender))));
    let policy = host.clone();
    let seen = Seen::default();
    let app = seen.clone();
    thread::spawn(move || {
        let mut registry = registry();
        registry.set_plugin_policy(Arc::new(policy));
        let observed = Rc::new(RefCell::new(Vec::new()));
        let observer = Rc::clone(&observed);
        registry.observe(move |invocation| observer.borrow_mut().push(invocation.clone()));
        for request in asked {
            // Recorded before the answer, which lets the plugin, and so the test, go on.
            request.run(|invocation| {
                let mut log = Vec::new();
                let result = registry.dispatch(&mut log, invocation).map(drop);
                app.ran.lock().unwrap().extend(log);
                app.observed
                    .lock()
                    .unwrap()
                    .extend(observed.borrow_mut().drain(..));
                result
            });
        }
    });
    (host, seen)
}

fn grants(capabilities: &[&str]) -> Grants {
    capabilities
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

/// Runs `f` on a thread of its own and returns what it returned, failing the test if that takes
/// two seconds.
fn bounded<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, returned) = mpsc::channel();
    thread::spawn(move || drop(sender.send(f())));
    returned
        .recv_timeout(Duration::from_secs(2))
        .expect("still running after 2 s")
}

/// The fixture loaded by the runner as `plugin` on `host`, under `deadline`, and what it logs,
/// each with the name of the thread it logged on.
fn load(
    host: &CommandHost,
    plugin: &str,
    capabilities: &[&str],
    deadline: Duration,
) -> (Result<Plugin, LoadError>, Receiver<(String, String)>) {
    let component = std::fs::read_to_string(FIXTURE).unwrap();
    load_text(host, plugin, capabilities, deadline, &component)
}

/// [`load`], of `component`'s text.
fn load_text(
    host: &CommandHost,
    plugin: &str,
    capabilities: &[&str],
    deadline: Duration,
    component: &str,
) -> (Result<Plugin, LoadError>, Receiver<(String, String)>) {
    let grants = grants(capabilities);
    let (sender, logged) = mpsc::channel();
    let mut state = HostState::new(Box::new(move |_, message| {
        let thread = thread::current().name().unwrap_or_default().to_owned();
        drop(sender.send((thread, message)));
    }));
    state.commands = State::new(plugin, &grants, host).unwrap();
    let limits = Limits {
        call_deadline: deadline,
        ..Limits::default()
    };
    let plugin = Plugin::load_hosted(
        PluginEngine::shared().unwrap(),
        None,
        component.as_bytes(),
        plugin,
        &grants,
        limits,
        state,
    );
    (plugin, logged)
}

/// The fixture loaded as [`PLUGIN`] with `commands`, and its commands by name.
fn loaded(host: &CommandHost, deadline: Duration) -> (PluginHandle, Receiver<(String, String)>) {
    let (plugin, logged) = load(host, PLUGIN, &["commands"], deadline);
    let handle: PluginHandle = Arc::new(Mutex::new(plugin.unwrap()));
    (handle, logged)
}

fn command(commands: &[PluginCommand], id: &str) -> PluginCommand {
    commands
        .iter()
        .find(|command| command.id().as_str() == id)
        .unwrap_or_else(|| panic!("no {id} in {commands:?}"))
        .clone()
}

fn ids(commands: &[PluginCommand]) -> Vec<&str> {
    commands
        .iter()
        .map(|command| command.id().as_str())
        .collect()
}

/// The fixture instantiated on this thread with `state`, its `init` not called.
fn instantiate_with(state: HostState) -> Guest {
    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::from_file(&engine, FIXTURE).unwrap();
    let mut linker = Linker::<HostState>::new(&engine);
    bindings::add_to_linker(&mut linker, |state| state).unwrap();
    let mut store = Store::new(&engine, state);
    let instance = linker.instantiate(&mut store, &component).unwrap();
    Extension::new(&mut store, &instance).unwrap();
    Guest { store, instance }
}

/// The fixture instantiated on this thread as `plugin` with `capabilities` on `host`, still
/// registering: as if in its `init`.
fn instantiate(host: &CommandHost, plugin: &str, capabilities: &[&str]) -> Guest {
    let mut guest = instantiate_with(HostState {
        commands: State::new(plugin, &grants(capabilities), host).unwrap(),
        ..HostState::default()
    });
    guest.store.data_mut().commands.begin_init();
    guest
}

struct Guest {
    store: Store<HostState>,
    instance: Instance,
}

impl Guest {
    fn register(&mut self, id: &str) -> Result<(), Error> {
        let call = self
            .instance
            .get_typed_func::<(&str,), (Result<(), Error>,)>(&mut self.store, "try-register")
            .unwrap();
        call.call(&mut self.store, (id,)).unwrap().0
    }

    fn invoke(&mut self, id: &str, args: &str) -> Result<(), Error> {
        let call = self
            .instance
            .get_typed_func::<(&str, &str), (Result<(), Error>,)>(&mut self.store, "try-invoke")
            .unwrap();
        call.call(&mut self.store, (id, args)).unwrap().0
    }
}

fn denied(result: Result<(), Error>) -> String {
    match result {
        Err(Error::Denied(message)) => message,
        other => panic!("expected denied, got {other:?}"),
    }
}

#[test]
fn a_plugin_registers_a_namespaced_command() {
    let (host, _) = app();
    let (handle, _) = loaded(&host, Duration::from_secs(1));
    let commands = host.commands(PLUGIN, &handle);
    assert_eq!(
        ids(&commands),
        ["acme.tool.hello", "acme.tool.spin", "acme.tool.invoke"]
    );
    let hello = command(&commands, "acme.tool.hello");
    assert_eq!(hello.plugin(), PLUGIN);
    let meta = hello.meta();
    assert_eq!(meta.title, "Hello");
    assert_eq!(meta.args_schema, Some(json!({"type": "object"})));
    assert_eq!(
        meta.context,
        Some(ContextPredicate::parse("editor && !read_only").unwrap())
    );
    assert!(!meta.mutates_buffer);
    let invoke = command(&commands, "acme.tool.invoke");
    assert_eq!(invoke.meta().args_schema, None);
    assert_eq!(invoke.meta().context, None);
    assert!(invoke.meta().mutates_buffer);
    assert_eq!(
        host.grants(PLUGIN),
        Some(grants(&["commands"])),
        "the host is the registry's policy for the running plugin"
    );
}

#[test]
fn bad_or_taken_names_are_refused() {
    let (host, _) = app();
    host.reserve("app");
    let mut tool = instantiate(&host, PLUGIN, &["commands"]);
    assert_eq!(tool.register("acme.tool.hello"), Ok(()));

    let outside = denied(tool.register("other.hello"));
    assert!(outside.contains("namespace"), "{outside}");
    let bare = denied(tool.register("acme.tool"));
    assert!(bare.contains("namespace"), "{bare}");
    let neighbour = denied(tool.register("acme.toolkit.hello"));
    assert!(neighbour.contains("namespace"), "{neighbour}");
    assert!(
        matches!(
            tool.register("acme.tool.Hello"),
            Err(Error::InvalidArgument(_))
        ),
        "not a CommandId"
    );
    assert!(matches!(
        tool.register("acme.tool..hello"),
        Err(Error::InvalidArgument(_))
    ));
    let builtin = denied(tool.register("acme.tool.builtin"));
    assert!(builtin.contains("built-in"), "{builtin}");
    let again = denied(tool.register("acme.tool.hello"));
    assert!(again.contains("plugin acme.tool"), "{again}");

    // Plugin `acme` may name commands under `acme.tool`, but not one `acme.tool` has.
    let mut acme = instantiate(&host, "acme", &["commands"]);
    let taken = denied(acme.register("acme.tool.hello"));
    assert!(
        taken.contains("already registered by plugin acme.tool"),
        "{taken}"
    );
    assert_eq!(acme.register("acme.tool.other"), Ok(()));

    let mut app = instantiate(&host, "app.thing", &["commands"]);
    let reserved = denied(app.register("app.thing.hello"));
    assert!(reserved.contains("app namespace is reserved"), "{reserved}");

    // Only during init.
    tool.store.data_mut().commands.end_init();
    let late = denied(tool.register("acme.tool.later"));
    assert!(late.contains("only during init"), "{late}");

    let registered = |plugin: &str| {
        host.registered(plugin)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(registered(PLUGIN), ["acme.tool.hello"]);
    assert_eq!(registered("acme"), ["acme.tool.other"]);
    assert!(registered("app.thing").is_empty());
}

#[test]
fn register_and_invoke_need_the_commands_capability() {
    let (host, seen) = app();
    let mut guest = instantiate(&host, PLUGIN, &["buffer.read", "buffer.write"]);
    let register = denied(guest.register("acme.tool.hello"));
    assert!(register.contains("commands capability"), "{register}");
    let invoke = denied(guest.invoke("test.read", "{}"));
    assert!(invoke.contains("commands capability"), "{invoke}");
    assert!(seen.observed().is_empty(), "nothing reached the registry");
    assert!(seen.ran().is_empty());
    drop(guest);

    // Loaded by the runner, its `init` fails on the refusal, and the host forgets it.
    let (plugin, _) = load(&host, PLUGIN, &["buffer.read"], Duration::from_secs(1));
    match plugin {
        Err(LoadError::Init {
            error: Error::Denied(message),
            ..
        }) => assert!(message.contains("commands capability"), "{message}"),
        Err(other) => panic!("expected init to fail, got {other:?}"),
        Ok(_) => panic!("expected init to fail, but it loaded"),
    }
    assert_eq!(host.grants(PLUGIN), None);
    assert!(host.unload(PLUGIN).is_empty());
}

#[test]
fn running_a_plugin_command_calls_run_command_under_the_deadline() {
    let (host, _) = app();
    let deadline = Duration::from_millis(200);
    let (handle, logged) = loaded(&host, deadline);
    let commands = host.commands(PLUGIN, &handle);
    let hello = command(&commands, "acme.tool.hello");
    let spin = command(&commands, "acme.tool.spin");

    assert_eq!(hello.run(&json!({"name": "x"})), Ok(Effect::Notify));
    assert_eq!(hello.run(&Value::Null), Ok(Effect::Notify));
    let on_plugin_thread = |message: &str| ("plugin acme.tool".to_owned(), message.to_owned());
    let logs: Vec<_> = logged.try_iter().collect();
    assert_eq!(
        logs,
        [
            on_plugin_thread("acme.tool.hello"),
            on_plugin_thread(r#"{"name":"x"}"#),
            on_plugin_thread("acme.tool.hello"),
            on_plugin_thread("{}"),
        ]
    );

    let (error, took) = bounded(move || {
        let started = Instant::now();
        (spin.run(&Value::Null).unwrap_err(), started.elapsed())
    });
    assert!(error.0.contains("timed out"), "{error}");
    assert!(took >= deadline, "stopped after {took:?}");
    let after = hello.run(&Value::Null).unwrap_err();
    assert!(after.0.contains("unloaded"), "{after}");

    // An invoke the app never answers ends at the deadline too, and its request is cancelled.
    let (host, withheld) = withholding();
    let (handle, _) = loaded(&host, deadline);
    let invoke = command(&host.commands(PLUGIN, &handle), "acme.tool.invoke");
    let (error, took) = bounded(move || {
        let started = Instant::now();
        (invoke.run(&Value::Null).unwrap_err(), started.elapsed())
    });
    assert!(error.0.contains("timed out"), "{error}");
    assert!(took >= deadline, "stopped after {took:?}");
    assert_eq!(
        withheld.cancelled(),
        [true],
        "the app holds a cancelled request"
    );
}

#[test]
fn plugin_invocations_are_host_script_and_observed() {
    let (host, seen) = app();
    let (handle, _) = loaded(&host, Duration::from_secs(1));
    let invoke = command(&host.commands(PLUGIN, &handle), "acme.tool.invoke");
    let effect = bounded(move || invoke.run(&Value::Null));
    assert_eq!(effect, Ok(Effect::Changed));
    let expected = Invocation::of("test.read")
        .unwrap()
        .args(json!({}))
        .actor(Actor::Host(PLUGIN.into()))
        .source(Source::Script);
    assert_eq!(seen.observed(), [expected]);
    assert_eq!(seen.ran(), [r#"Host("acme.tool") ran test.read"#]);

    // The same from a store on this thread, with JSON args.
    let mut guest = instantiate(&host, "acme.other", &["commands", "buffer.write"]);
    assert_eq!(
        guest.invoke("test.write", r#"{"count": 2}"#),
        Err(Error::InvalidArgument(
            "bad args for test.write: takes no args".into()
        ))
    );
    assert_eq!(guest.invoke("test.write", ""), Ok(()));
    let observed = seen.observed();
    let last = observed.last().unwrap();
    assert_eq!(last.actor, Actor::Host("acme.other".into()));
    assert_eq!(last.source, Source::Script);
    assert_eq!(last.args, Value::Null);
    assert_eq!(observed.len(), 2, "a refused invocation is not observed");
}

#[test]
fn an_undeclared_command_is_refused_to_the_plugin() {
    let (host, seen) = app();
    let mut guest = instantiate(&host, PLUGIN, &["commands"]);
    let undeclared = denied(guest.invoke("test.secret", "{}"));
    assert!(undeclared.contains("isn't open to plugins"), "{undeclared}");
    let missing = denied(guest.invoke("test.write", "{}"));
    assert!(missing.contains("needs buffer.write"), "{missing}");
    assert!(matches!(
        guest.invoke("test.nothing", "{}"),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        guest.invoke("test.read", "{not json"),
        Err(Error::InvalidArgument(_))
    ));
    assert!(seen.observed().is_empty());
    assert!(seen.ran().is_empty(), "nothing ran: {:?}", seen.ran());
}

#[test]
fn unloading_returns_the_plugins_command_ids() {
    let (host, _) = app();
    let (handle, _) = loaded(&host, Duration::from_secs(1));
    assert_eq!(
        State::new(PLUGIN, &grants(&["commands"]), &host).err(),
        Some(AttachError::AlreadyLoaded(PLUGIN.into())),
        "one plugin per id"
    );
    let removed: Vec<String> = host
        .unload(PLUGIN)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        removed,
        ["acme.tool.hello", "acme.tool.spin", "acme.tool.invoke"]
    );
    assert!(host.commands(PLUGIN, &handle).is_empty());
    assert_eq!(host.grants(PLUGIN), None, "the registry refuses it now");
    assert!(host.unload(PLUGIN).is_empty());
    drop(handle);

    // Its ids are free again: loading it once more registers them anew.
    let (handle, _) = loaded(&host, Duration::from_secs(1));
    let commands = host.commands(PLUGIN, &handle);
    assert_eq!(
        ids(&commands),
        ["acme.tool.hello", "acme.tool.spin", "acme.tool.invoke"]
    );
    host.unload(PLUGIN);
    let unloaded = command(&commands, "acme.tool.hello").run(&Value::Null);
    assert!(unloaded.unwrap_err().0.contains("unloaded"));
    drop(handle);

    // A load that fails while its plugin is unloaded and loaded again leaves the new load be,
    // however the two interleave: the two start together each round.
    let caps = grants(&["commands"]);
    for _ in 0..2000 {
        let failing = State::new(PLUGIN, &caps, &host).unwrap();
        let together = Arc::new(Barrier::new(2));
        let (other, start, reload_caps) = (host.clone(), Arc::clone(&together), caps.clone());
        let reloading = thread::spawn(move || {
            start.wait();
            other.unload(PLUGIN);
            State::new(PLUGIN, &reload_caps, &other).ok()
        });
        together.wait();
        drop(failing);
        if let Some(mut replacement) = reloading.join().unwrap() {
            assert_eq!(host.grants(PLUGIN), Some(caps.clone()), "the reload stays");
            replacement.end_init();
        }
        host.unload(PLUGIN);
    }
}

#[test]
fn an_unloaded_plugin_keeps_nothing_across_a_reload() {
    let (host, seen) = app();
    let mut old = instantiate(&host, PLUGIN, &["commands"]);
    assert_eq!(old.register("acme.tool.hello"), Ok(()));
    assert_eq!(host.unload(PLUGIN).len(), 1);
    let mut new = instantiate(&host, PLUGIN, &["commands", "buffer.write"]);

    // The old store holds only its own revoked load, whatever the new one is granted.
    let invoke = denied(old.invoke("test.write", "{}"));
    assert!(invoke.contains("unloaded"), "{invoke}");
    let register = denied(old.register("acme.tool.old"));
    assert!(register.contains("unloaded"), "{register}");
    assert!(host.registered(PLUGIN).is_empty());
    assert!(seen.observed().is_empty());
    assert_eq!(new.invoke("test.write", "{}"), Ok(()));
    assert_eq!(seen.observed().len(), 1);
    drop((old, new));

    // A request still out holds the id: no reload until the app lets it go, and then it is
    // not run.
    host.unload(PLUGIN);
    let (host, withheld) = withholding();
    let mut old = instantiate(&host, PLUGIN, &["commands"]);
    old.store.data_mut().deadline = Some(Instant::now() + Duration::from_millis(50));
    let error = old.invoke("test.read", "{}").unwrap_err();
    assert!(matches!(error, Error::Failed(_)), "{error:?}");
    host.unload(PLUGIN);
    assert_eq!(withheld.cancelled(), [true]);
    assert_eq!(
        State::new(PLUGIN, &grants(&["commands"]), &host).err(),
        Some(AttachError::RequestsOut(PLUGIN.into()))
    );
    let request = withheld.0.lock().unwrap().pop().unwrap();
    let mut ran = false;
    request.run(|_| {
        ran = true;
        Ok(())
    });
    assert!(!ran, "a cancelled request does not run");
    assert!(State::new(PLUGIN, &grants(&["commands"]), &host).is_ok());
}

#[test]
fn a_registration_before_init_is_refused() {
    let (host, _) = app();
    let fixture = std::fs::read_to_string(FIXTURE).unwrap();
    assert!(fixture.contains(";; (start $early)"));
    let early = fixture.replace(";; (start $early)", "(start $early)");
    let (plugin, _) = load_text(&host, PLUGIN, &["commands"], Duration::from_secs(1), &early);
    plugin.unwrap();
    assert_eq!(
        host.registered(PLUGIN)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["acme.tool.hello", "acme.tool.spin", "acme.tool.invoke"],
        "the start function's acme.tool.early was refused, init's three were not"
    );
}

#[test]
fn a_store_without_a_host_leaves_both_imports_unimplemented() {
    let mut guest = instantiate_with(HostState::new(Box::new(|_: Level, _| {})));
    assert_eq!(
        guest.register("acme.tool.hello"),
        Err(Error::Unimplemented(
            "kiln:extension/commands.register".into()
        ))
    );
    assert_eq!(
        guest.invoke("test.read", "{}"),
        Err(Error::Unimplemented(
            "kiln:extension/commands.invoke".into()
        ))
    );
}
