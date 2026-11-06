//! The `fastedit:editor/formatting` imports, run by the guest in `tests/guests/editor/`: what a
//! plugin may register, and how a format request runs its command and checks its answer.

use std::sync::{Arc, Mutex, OnceLock};

use fe_buffer::{Buffer, BufferSnapshot};
use fe_plugin::decorations::BufferVersions;
use fe_plugin::editor::{self, BufferAccess, BufferText, Proposal};
use fe_plugin::fastedit::editor::buffer::TextEdit;
use fe_plugin::formatting::{FormatError, Formatters, State};
use fe_plugin::{EditorExtension, HostState};
use kiln_command::CommandId;
use kiln_command::capability::{Capability, Grants};
use kiln_plugin::bindings::Error;
use kiln_plugin::commands::{CallPlugin, CommandHost, Dispatcher, PluginHandle, Request};
use kiln_plugin::limits::Limits;
use kiln_plugin::runner::{Call, CallError, Diagnostic, Reason, Reply};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};

const PLUGIN: &str = "fmt";
const TEXT: &str = "héllo wörld";

/// The one open buffer, buffer 1, which plugins see at version 1.
struct Buffers(Buffer);

impl BufferAccess for Buffers {
    fn snapshot(&mut self, buffer: u64) -> Option<BufferText> {
        (buffer == 1).then(|| BufferText {
            snapshot: self.0.snapshot(),
            version: 1,
            path: None,
            language: Some("text".into()),
        })
    }

    fn edit(
        &mut self,
        _: u64,
        _: u64,
        _: Vec<(std::ops::Range<usize>, String)>,
        _: fe_buffer::Origin,
    ) -> Result<u64, Error> {
        unreachable!()
    }

    fn propose(&mut self, _: u64, _: Proposal) -> Result<(), Error> {
        unreachable!()
    }
}

impl BufferVersions for Buffers {
    fn now(&mut self, buffer: u64) -> Option<BufferSnapshot> {
        (buffer == 1).then(|| self.0.snapshot())
    }

    fn at(&mut self, buffer: u64, version: u64) -> Option<BufferSnapshot> {
        (version == 1).then(|| self.now(buffer))?
    }
}

struct Dropped;

impl Dispatcher for Dropped {
    fn dispatch(&self, _: Request) {}
}

/// The guest, which runs calls on this thread; `fmt.slow` stands in for a command that overruns
/// the plugin's call deadline.
struct Guest {
    store: Store<HostState>,
    extension: EditorExtension,
    /// Runs when the next call starts, as the app does something while a command runs.
    during: Option<Box<dyn FnOnce() + Send>>,
}

impl CallPlugin for Guest {
    fn call(&mut self, call: Call) -> Result<Reply, CallError> {
        let Call::RunCommand { id, args } = call else {
            unreachable!()
        };
        if let Some(during) = self.during.take() {
            during();
        }
        if id == "fmt.reject_slow" {
            drop(self.run(&id, &args));
        }
        if id == "fmt.slow" || id == "fmt.reject_slow" {
            return Err(CallError::Failed(Diagnostic {
                plugin: PLUGIN.into(),
                reason: Reason::Timeout,
                message: "a call ran past its deadline".into(),
            }));
        }
        Ok(Reply::RunCommand(self.run(&id, &args)))
    }
}

impl Guest {
    fn run(&mut self, command: &str, args: &str) -> Result<(), Error> {
        let handler = self.extension.kiln_extension_command_handler();
        handler
            .call_run_command(&mut self.store, &command.to_owned(), args)
            .unwrap()
    }
}

struct Setup {
    formatters: Formatters,
    guest: Arc<Mutex<Guest>>,
}

fn setup(grants: &[&str]) -> Setup {
    setup_on(grants, &Formatters::new())
}

/// The plugin, loaded again or anew on `formatters`.
fn setup_on(grants: &[&str], formatters: &Formatters) -> Setup {
    let setup = load_on(grants, formatters);
    setup.bind();
    setup
}

/// The plugin, loaded on `formatters` but not yet bound.
fn load_on(grants: &[&str], formatters: &Formatters) -> Setup {
    static COMPILED: OnceLock<(Engine, Component, Linker<HostState>)> = OnceLock::new();
    let (engine, component, linker) = COMPILED.get_or_init(|| {
        let mut config = Config::new();
        config.wasm_component_model(true);
        let engine = Engine::new(&config).unwrap();
        let component = Component::from_file(&engine, "tests/guests/editor/editor.wasm").unwrap();
        let mut linker = Linker::new(&engine);
        fe_plugin::add_to_linker(&mut linker).unwrap();
        linker.define_unknown_imports_as_traps(&component).unwrap();
        (engine, component, linker)
    });
    let grants: Grants = grants
        .iter()
        .map(|c| Capability::parse(c).unwrap())
        .collect();
    let buffers = Arc::new(Mutex::new(Buffers(Buffer::new(TEXT))));
    let host = CommandHost::new(Vec::<CommandId>::new(), Arc::new(Dropped));
    let formatters = formatters.clone();
    let kiln = kiln_plugin::bindings::HostState {
        commands: kiln_plugin::commands::State::new(PLUGIN, &grants, &host).unwrap(),
        ..Default::default()
    };
    let state = HostState {
        kiln,
        editor: editor::State::new(
            PLUGIN,
            &grants,
            &Limits::default(),
            Box::new(Arc::clone(&buffers)),
        ),
        formatting: State::new(PLUGIN, &grants, &formatters, &host, Box::new(buffers)),
        ..HostState::default()
    };
    let mut store = Store::new(engine, state);
    let extension = EditorExtension::instantiate(&mut store, component, linker).unwrap();
    store.data_mut().kiln.commands.begin_init();
    assert_eq!(extension.call_init(&mut store).unwrap(), Ok(()));
    store.data_mut().kiln.commands.end_init();
    let guest = Arc::new(Mutex::new(Guest {
        store,
        extension,
        during: None,
    }));
    Setup { formatters, guest }
}

impl Setup {
    fn bind(&self) {
        let handle: PluginHandle = self.guest.clone();
        self.guest
            .lock()
            .unwrap()
            .store
            .data()
            .formatting
            .bind(handle);
    }

    fn register(&self, command: &str, languages: &str) -> Result<(), Error> {
        let args = format!("{command} {languages}");
        self.guest.lock().unwrap().run("register-formatter", &args)
    }

    fn listed(&self) -> Vec<(String, String)> {
        let formatters = self.formatters.formatters();
        let listed = formatters.into_iter();
        listed.map(|f| (f.language, f.command)).collect()
    }
}

fn pair(language: &str, command: &str) -> (String, String) {
    (language.to_owned(), command.to_owned())
}

#[test]
fn register_names_languages_and_a_command() {
    let setup = setup(&["commands", "buffer.read", "buffer.write"]);
    setup.register("fmt.format", "rust go").unwrap();
    assert_eq!(
        setup.listed(),
        [pair("go", "fmt.format"), pair("rust", "fmt.format")]
    );

    // Registering again replaces the entry for those languages alone.
    setup.register("fmt.silent", "go").unwrap();
    assert_eq!(
        setup.listed(),
        [pair("go", "fmt.silent"), pair("rust", "fmt.format")]
    );
    let formatter = setup.formatters.for_language("go").unwrap();
    assert_eq!(
        (formatter.plugin.as_str(), formatter.command.as_str()),
        (PLUGIN, "fmt.silent")
    );

    // A command the plugin didn't register, such as another plugin's, is refused.
    let other = setup.register("other.format", "rust");
    assert!(matches!(other, Err(Error::InvalidArgument(_))), "{other:?}");
    assert_eq!(setup.listed().len(), 2);
}

#[test]
fn a_plugin_without_buffer_write_cannot_register() {
    for grants in [
        &["commands", "buffer.read"][..],
        &["commands", "buffer.write"][..],
    ] {
        let setup = setup(grants);
        let denied = setup.register("fmt.format", "rust");
        assert!(matches!(denied, Err(Error::Denied(_))), "{denied:?}");
        assert_eq!(setup.listed(), []);
    }
}

fn edit(start: u64, end: u64, text: &str) -> TextEdit {
    TextEdit {
        range: fe_plugin::fastedit::editor::buffer::Range { start, end },
        text: text.to_owned(),
    }
}

#[test]
fn a_format_request_returns_the_answered_edits() {
    let setup = setup(&["commands", "buffer.read", "buffer.write"]);
    setup.register("fmt.format", "text").unwrap();
    setup.register("fmt.silent", "quiet").unwrap();

    let edits = setup.formatters.request(1, 1, "text").unwrap();
    // The text is 13 bytes: `é` and `ö` take two.
    assert_eq!(edits, Some(vec![edit(0, 13, "HÉLLO WÖRLD")]));

    assert_eq!(setup.formatters.request(1, 1, "quiet"), Ok(None));
    assert_eq!(
        setup.formatters.request(1, 1, "go"),
        Err(FormatError::NoFormatter("go".into()))
    );
}

#[test]
fn a_bad_answer_or_an_overrun_fails_the_request() {
    let setup = setup(&["commands", "buffer.read", "buffer.write"]);
    setup.register("fmt.unordered", "unordered").unwrap();
    setup.register("fmt.split", "split").unwrap();
    setup.register("fmt.slow", "slow").unwrap();
    setup.register("fmt.format", "text").unwrap();

    for language in ["unordered", "split"] {
        let failed = setup.formatters.request(1, 1, language);
        assert!(
            matches!(failed, Err(FormatError::BadAnswer(_))),
            "{failed:?}"
        );
    }
    // A bad answer doesn't unload the plugin.
    assert!(setup.formatters.request(1, 1, "text").unwrap().is_some());

    let failed = setup.formatters.request(1, 1, "slow");
    assert!(matches!(failed, Err(FormatError::Timeout(_))), "{failed:?}");
    assert_eq!(setup.listed(), []);
    assert_eq!(
        setup.formatters.request(1, 1, "text"),
        Err(FormatError::NoFormatter("text".into()))
    );
}

const ALL: [&str; 3] = ["commands", "buffer.read", "buffer.write"];

#[test]
fn a_second_or_unavailable_answer_fails_even_if_the_guest_carries_on() {
    let setup = setup(&ALL);
    setup.register("fmt.catch", "catch").unwrap();
    for version in [1, 2] {
        let failed = setup.formatters.request(1, version, "catch");
        assert!(
            matches!(failed, Err(FormatError::BadAnswer(_))),
            "{failed:?}"
        );
    }
}

#[test]
fn an_overrun_after_a_bad_answer_is_still_a_timeout() {
    let setup = setup(&ALL);
    setup.register("fmt.reject_slow", "slow").unwrap();
    let failed = setup.formatters.request(1, 1, "slow");
    assert!(matches!(failed, Err(FormatError::Timeout(_))), "{failed:?}");
}

#[test]
fn an_unload_while_a_command_runs_fails_the_request() {
    let setup = setup(&ALL);
    setup.register("fmt.format", "text").unwrap();
    let formatters = setup.formatters.clone();
    setup.guest.lock().unwrap().during = Some(Box::new(move || formatters.unload(PLUGIN)));
    let failed = setup.formatters.request(1, 1, "text");
    assert!(failed.is_err(), "{failed:?}");
    assert_eq!(setup.listed(), []);
}

#[test]
fn an_old_loads_failure_leaves_the_new_load_alone() {
    let old = setup(&ALL);
    old.register("fmt.slow", "slow").unwrap();
    let formatters = old.formatters.clone();
    let loaded = Arc::new(Mutex::new(None));
    let kept = Arc::clone(&loaded);
    old.guest.lock().unwrap().during = Some(Box::new(move || {
        // The plugin is loaded again while the old load's command runs.
        let new = setup_on(&ALL, &formatters);
        new.register("fmt.format", "text").unwrap();
        *kept.lock().unwrap() = Some(new);
    }));
    let failed = old.formatters.request(1, 1, "slow");
    assert!(matches!(failed, Err(FormatError::Timeout(_))), "{failed:?}");

    let new = loaded.lock().unwrap().take().unwrap();
    assert_eq!(new.listed(), [pair("text", "fmt.format")]);
    assert!(new.formatters.request(1, 1, "text").unwrap().is_some());
    // The old load's slice can register nothing more.
    let denied = old.register("fmt.format", "rust");
    assert!(matches!(denied, Err(Error::Denied(_))), "{denied:?}");
}

#[test]
fn an_old_loads_late_bind_leaves_the_new_load_bound() {
    let formatters = Formatters::new();
    let old = load_on(&ALL, &formatters);
    old.register("fmt.slow", "slow").unwrap();
    // The plugin is loaded again and bound before the old load's bind completes.
    let new = setup_on(&ALL, &formatters);
    new.register("fmt.format", "text").unwrap();
    old.bind();

    // Requests still run on the new load, and the old load's bind didn't revive its formatters.
    assert!(formatters.request(1, 1, "text").unwrap().is_some());
    assert_eq!(new.listed(), [pair("text", "fmt.format")]);
    assert_eq!(
        formatters.request(1, 1, "slow"),
        Err(FormatError::NoFormatter("slow".into()))
    );
}
