//! The `fastedit:editor/buffer` imports, run by the guest in `tests/guests/editor/` against
//! buffers held here: what a plugin reads, how its edits land and undo, and what it is refused.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use fe_buffer::{Buffer, BufferEvent, Origin, Version};
use fe_plugin::editor::{BufferAccess, BufferText, Proposal, State};
use fe_plugin::fastedit::editor::buffer::Host as _;
use fe_plugin::{EditorExtension, HostState};
use kiln_command::capability::{Capability, Grants};
use kiln_plugin::bindings::Error;
use kiln_plugin::limits::Limits;
use kiln_plugin::paths::{self, Rules};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};

const PLUGIN: &str = "editor-test";

fn host() -> Origin {
    Origin::Host(PLUGIN.into())
}

/// An open buffer, with the versions plugins have been shown for it.
struct Open {
    buffer: Buffer,
    seen: Vec<Version>,
    /// Its path, as the host spells it.
    path: Option<String>,
}

impl Open {
    /// The version plugins see: one more for each buffer version seen since the last.
    fn version(&mut self) -> u64 {
        if self.seen.last() != Some(&self.buffer.version()) {
            self.seen.push(self.buffer.version());
        }
        self.seen.len() as u64
    }
}

/// The buffers the plugin reaches, and what it proposed.
#[derive(Default)]
struct Buffers {
    open: HashMap<u64, Open>,
    proposals: Vec<(u64, Proposal)>,
}

impl Buffers {
    fn with(id: u64, text: &str) -> Arc<Mutex<Self>> {
        let mut buffers = Self::default();
        buffers.open.insert(
            id,
            Open {
                buffer: Buffer::new(text),
                seen: Vec::new(),
                path: Some(format!("/work/{id}.txt")),
            },
        );
        Arc::new(Mutex::new(buffers))
    }
}

impl BufferAccess for Buffers {
    fn snapshot(&mut self, buffer: u64) -> Option<BufferText> {
        let open = self.open.get_mut(&buffer)?;
        Some(BufferText {
            version: open.version(),
            snapshot: open.buffer.snapshot(),
            path: open.path.clone(),
            language: Some("text".into()),
        })
    }

    fn edit(
        &mut self,
        buffer: u64,
        version: u64,
        edits: Vec<(std::ops::Range<usize>, String)>,
        origin: Origin,
    ) -> Result<u64, Error> {
        let open = self
            .open
            .get_mut(&buffer)
            .ok_or_else(|| Error::NotFound(buffer.to_string()))?;
        if open.version() != version {
            return Err(Error::Conflict(format!("not at {version}")));
        }
        let base = open.buffer.snapshot();
        open.buffer
            .edit_from(&base, edits, origin)
            .map_err(|error| Error::Conflict(error.to_string()))?;
        Ok(open.version())
    }

    fn propose(&mut self, buffer: u64, proposal: Proposal) -> Result<(), Error> {
        self.proposals.push((buffer, proposal));
        Ok(())
    }
}

/// The editor guest, loaded with `grants` over `buffers`.
struct Plugin {
    store: Store<HostState>,
    extension: EditorExtension,
    logged: Arc<Mutex<Vec<String>>>,
}

impl Plugin {
    fn new(grants: &[&str], buffers: &Arc<Mutex<Buffers>>) -> Self {
        Self::with_path_rules(grants, buffers, Rules::HOST)
    }

    /// [`Plugin::new`], over buffers whose paths `rules` spell.
    fn with_path_rules(grants: &[&str], buffers: &Arc<Mutex<Buffers>>, rules: Rules) -> Self {
        // Compiled once for every plugin a test loads.
        static COMPILED: OnceLock<(Engine, Component, Linker<HostState>)> = OnceLock::new();
        let (engine, component, linker) = COMPILED.get_or_init(|| {
            let mut config = Config::new();
            config.wasm_component_model(true);
            let engine = Engine::new(&config).unwrap();
            let component =
                Component::from_file(&engine, "tests/guests/editor/editor.wasm").unwrap();
            let mut linker = Linker::new(&engine);
            fe_plugin::add_to_linker(&mut linker).unwrap();
            // The guest's std imports WASI, which it never calls.
            linker.define_unknown_imports_as_traps(&component).unwrap();
            (engine, component, linker)
        });

        let grants: Grants = grants
            .iter()
            .map(|capability| Capability::parse(capability).unwrap())
            .collect();
        let logged = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&logged);
        let state = HostState {
            kiln: kiln_plugin::bindings::HostState::new(Box::new(move |_, message| {
                sink.lock().unwrap().push(message);
            })),
            editor: State::new(
                PLUGIN,
                &grants,
                &Limits::default(),
                Box::new(Arc::clone(buffers)),
            )
            .with_path_rules(rules),
            ..HostState::default()
        };
        let mut store = Store::new(engine, state);
        let extension = EditorExtension::instantiate(&mut store, component, linker).unwrap();
        assert_eq!(extension.call_init(&mut store).unwrap(), Ok(()));
        Self {
            store,
            extension,
            logged,
        }
    }

    /// Runs the guest's `command` with `args`: what it logged, or the import's error.
    fn run(&mut self, command: &str, args: &str) -> Result<String, Error> {
        self.logged.lock().unwrap().clear();
        self.extension
            .kiln_extension_command_handler()
            .call_run_command(&mut self.store, &command.to_owned(), args)
            .unwrap()?;
        Ok(self.logged.lock().unwrap().join("\n"))
    }

    /// The guest's read of `args`, as its version and text.
    fn read(&mut self, args: &str) -> Result<(u64, String), Error> {
        let logged = self.run("read", args)?;
        let (version, text) = logged.split_once(' ').unwrap();
        Ok((version.parse().unwrap(), text.to_owned()))
    }
}

/// The path the plugin's read of buffer `id` names, calling the import as the guest would.
fn read_path(plugin: &mut Plugin, id: u64) -> Option<String> {
    plugin.store.data_mut().read(id, None).unwrap().path
}

fn set_path(buffers: &Arc<Mutex<Buffers>>, id: u64, path: &str) {
    buffers.lock().unwrap().open.get_mut(&id).unwrap().path = Some(path.to_owned());
}

fn text(buffers: &Arc<Mutex<Buffers>>, id: u64) -> String {
    buffers.lock().unwrap().open[&id].buffer.text().to_string()
}

/// Edits buffer `id` as the user.
fn type_in(buffers: &Arc<Mutex<Buffers>>, id: u64, at: usize, text: &str) {
    let mut buffers = buffers.lock().unwrap();
    let buffer = &mut buffers.open.get_mut(&id).unwrap().buffer;
    buffer.edit([(at..at, text)], Origin::User).unwrap();
}

#[test]
fn read_returns_text_and_version_with_buffer_read() {
    let buffers = Buffers::with(7, "héllo wörld");
    let mut plugin = Plugin::new(&["buffer.read"], &buffers);
    let (version, read) = plugin.read("7").unwrap();
    assert_eq!(read, "héllo wörld");
    // A range's text, in UTF-8 bytes, at the same version.
    assert_eq!(
        plugin.read("7 7 13").unwrap(),
        (version, "wörld".to_owned())
    );
    assert_eq!(plugin.read("7 0 0").unwrap(), (version, String::new()));

    type_in(&buffers, 7, 0, "¡");
    let (after, read) = plugin.read("7").unwrap();
    assert!(after > version, "{after} after {version}");
    assert_eq!(read, "¡héllo wörld");

    let missing = plugin.read("8");
    assert!(matches!(missing, Err(Error::NotFound(_))), "{missing:?}");
}

#[test]
fn reads_and_edits_without_the_capability_are_refused() {
    let buffers = Buffers::with(1, "text");
    let mut plugin = Plugin::new(&[], &buffers);
    let read = plugin.read("1");
    assert!(
        matches!(&read, Err(Error::Denied(why)) if why.contains("buffer.read")),
        "{read:?}"
    );

    let mut plugin = Plugin::new(&["buffer.read"], &buffers);
    let (version, _) = plugin.read("1").unwrap();
    let edit = plugin.run("edit", &format!("1 {version}\n0 4 edited"));
    assert!(
        matches!(&edit, Err(Error::Denied(why)) if why.contains("buffer.write")),
        "{edit:?}"
    );
    let propose = plugin.run("propose", &format!("1 {version} Edit\n0 4 edited"));
    assert!(matches!(propose, Err(Error::Denied(_))), "{propose:?}");
    assert_eq!(text(&buffers, 1), "text");
    assert!(buffers.lock().unwrap().proposals.is_empty());

    // Writing doesn't grant reading.
    let mut plugin = Plugin::new(&["buffer.write"], &buffers);
    assert!(matches!(plugin.read("1"), Err(Error::Denied(_))));
}

#[test]
fn a_read_over_the_max_is_an_error_not_a_truncation() {
    let max = Limits::default().max_read_bytes;
    assert_eq!(max, 16 << 20);
    let buffers = Buffers::with(1, &"a".repeat(max + 1));
    let mut plugin = Plugin::new(&["buffer.read"], &buffers);
    let read = plugin.run("read", "1");
    assert!(
        matches!(&read, Err(Error::Failed(why)) if why.contains("limit")),
        "{read:?}"
    );
    let read = plugin.run("read", &format!("1 0 {}", max + 1));
    assert!(matches!(read, Err(Error::Failed(_))), "{read:?}");
    // A range within the limit reads.
    assert_eq!(plugin.read("1 5 8").unwrap().1, "aaa");
}

#[test]
fn edits_apply_as_one_host_origin_transaction() {
    let buffers = Buffers::with(1, "one two three");
    let events = buffers
        .lock()
        .unwrap()
        .open
        .get_mut(&1)
        .unwrap()
        .buffer
        .subscribe();
    let mut plugin = Plugin::new(&["buffer.read", "buffer.write"], &buffers);
    let (version, _) = plugin.read("1").unwrap();
    let logged = plugin
        .run("edit", &format!("1 {version}\n0 3 1\n4 7 2\n8 13 3"))
        .unwrap();
    assert_eq!(text(&buffers, 1), "1 2 3");
    let (now, _) = plugin.read("1").unwrap();
    assert_eq!(logged, now.to_string());
    assert!(now > version);

    let events = events.drain();
    assert_eq!(events.len(), 1, "{events:?}");
    let BufferEvent::Edited { origin, edits, .. } = &events[0] else {
        panic!("{events:?}");
    };
    assert_eq!(*origin, host());
    assert_eq!(edits.len(), 3);

    // One undo of the plugin's origin reverts all three.
    let mut buffers = buffers.lock().unwrap();
    let buffer = &mut buffers.open.get_mut(&1).unwrap().buffer;
    assert!(buffer.undo(|origin| *origin == host()).unwrap().is_some());
    assert_eq!(buffer.text().to_string(), "one two three");
    assert_eq!(buffer.undo(|origin| *origin == host()).unwrap(), None);
}

#[test]
fn a_stale_version_applies_nothing() {
    let buffers = Buffers::with(1, "alpha beta");
    let mut plugin = Plugin::new(&["buffer.read", "buffer.write"], &buffers);
    let (stale, _) = plugin.read("1").unwrap();
    // The user's edit is apart from the plugin's, and the version still refuses it.
    type_in(&buffers, 1, 10, "!");
    let edit = plugin.run("edit", &format!("1 {stale}\n0 5 ALPHA"));
    assert!(matches!(edit, Err(Error::Conflict(_))), "{edit:?}");
    let propose = plugin.run("propose", &format!("1 {stale} Shout\n0 5 ALPHA"));
    assert!(matches!(propose, Err(Error::Conflict(_))), "{propose:?}");
    let future = plugin.run("edit", &format!("1 {}\n0 5 ALPHA", stale + 5));
    assert!(matches!(future, Err(Error::Conflict(_))), "{future:?}");
    assert_eq!(text(&buffers, 1), "alpha beta!");
    assert!(buffers.lock().unwrap().proposals.is_empty());

    // Read again, it applies.
    let (version, _) = plugin.read("1").unwrap();
    plugin
        .run("edit", &format!("1 {version}\n0 5 ALPHA"))
        .unwrap();
    assert_eq!(text(&buffers, 1), "ALPHA beta!");
}

#[test]
fn bad_ranges_are_refused() {
    let buffers = Buffers::with(1, "héllo world");
    let events = buffers
        .lock()
        .unwrap()
        .open
        .get_mut(&1)
        .unwrap()
        .buffer
        .subscribe();
    let mut plugin = Plugin::new(&["buffer.read", "buffer.write"], &buffers);
    let (version, _) = plugin.read("1").unwrap();
    for edits in [
        // Overlapping.
        "0 6 a\n4 8 b",
        // Unsorted.
        "7 12 a\n0 1 b",
        // Out of range, at the end and past u32.
        "0 13 a",
        "0 4294967296 a",
        // Ending and starting inside é.
        "2 3 a",
        "0 2 a",
        // Backwards.
        "5 3 a",
        // Two insertions at one offset.
        "3 3 a\n3 3 b",
    ] {
        let edit = plugin.run("edit", &format!("1 {version}\n{edits}"));
        assert!(
            matches!(edit, Err(Error::InvalidArgument(_))),
            "{edits:?}: {edit:?}"
        );
        let propose = plugin.run("propose", &format!("1 {version} Bad\n{edits}"));
        assert!(
            matches!(propose, Err(Error::InvalidArgument(_))),
            "{edits:?}: {propose:?}"
        );
    }
    for range in ["0 13", "2 3", "0 2", "5 3"] {
        let read = plugin.read(&format!("1 {range}"));
        assert!(
            matches!(read, Err(Error::InvalidArgument(_))),
            "{range:?}: {read:?}"
        );
    }
    assert_eq!(text(&buffers, 1), "héllo world");
    assert!(events.drain().is_empty());
    assert!(buffers.lock().unwrap().proposals.is_empty());

    // Touching ranges, an insertion beside a replacement, and the very end are fine.
    plugin
        .run(
            "edit",
            &format!("1 {version}\n0 3 H\n3 3 E\n3 6 LLO\n12 12 !"),
        )
        .unwrap();
    assert_eq!(text(&buffers, 1), "HELLO world!");
}

#[test]
fn undo_by_origin_reverts_only_the_plugins_edits() {
    let buffers = Buffers::with(1, "a b");
    let mut plugin = Plugin::new(&["buffer.read", "buffer.write"], &buffers);
    type_in(&buffers, 1, 0, "user1 ");
    let (version, _) = plugin.read("1").unwrap();
    plugin.run("edit", &format!("1 {version}\n6 7 A")).unwrap();
    type_in(&buffers, 1, 9, " user2");
    let (version, _) = plugin.read("1").unwrap();
    plugin.run("edit", &format!("1 {version}\n8 9 B")).unwrap();
    assert_eq!(text(&buffers, 1), "user1 A B user2");

    let mut buffers = buffers.lock().unwrap();
    let buffer = &mut buffers.open.get_mut(&1).unwrap().buffer;
    let plugins = |origin: &Origin| *origin == host();
    assert!(buffer.undo(plugins).unwrap().is_some());
    assert_eq!(buffer.text().to_string(), "user1 A b user2");
    assert!(buffer.undo(plugins).unwrap().is_some());
    assert_eq!(buffer.text().to_string(), "user1 a b user2");
    assert_eq!(buffer.undo(plugins).unwrap(), None);
    // The user's own undo still has both of theirs.
    assert!(
        buffer
            .undo(|origin| *origin == Origin::User)
            .unwrap()
            .is_some()
    );
    assert_eq!(buffer.text().to_string(), "user1 a b");
}

#[test]
fn a_proposed_change_waits_for_the_user() {
    let buffers = Buffers::with(1, "fn parse() {}\nparse();\n");
    let mut plugin = Plugin::new(&["buffer.read", "buffer.write"], &buffers);
    let (version, _) = plugin.read("1").unwrap();
    plugin
        .run(
            "propose",
            &format!("1 {version} Rename to parse_header\n3 8 parse_header\n14 19 parse_header"),
        )
        .unwrap();
    assert_eq!(text(&buffers, 1), "fn parse() {}\nparse();\n");
    // The user types meanwhile; the proposal follows the text.
    type_in(&buffers, 1, 0, "// x\n");

    let mut buffers = buffers.lock().unwrap();
    let (id, mut proposal) = buffers.proposals.pop().unwrap();
    assert_eq!(id, 1);
    assert_eq!(proposal.title, "Rename to parse_header");
    assert_eq!(*proposal.change.origin(), host());
    let buffer = &mut buffers.open.get_mut(&1).unwrap().buffer;
    assert_eq!(proposal.change.buffer(), buffer.id());
    assert_eq!(proposal.change.hunks(buffer.current()).len(), 2);
    assert_eq!(buffer.text().to_string(), "// x\nfn parse() {}\nparse();\n");

    let (applied, stale) = proposal.change.accept_all_as(buffer, Origin::User);
    assert!(applied.is_some());
    assert_eq!(stale, []);
    assert_eq!(
        buffer.text().to_string(),
        "// x\nfn parse_header() {}\nparse_header();\n"
    );
}

#[test]
fn a_snapshot_names_the_guest_path_of_a_windows_host_path() {
    let buffers = Buffers::with(1, "text");
    let mut plugin = Plugin::with_path_rules(&["buffer.read"], &buffers, Rules::Windows);
    for (host, guest) in [
        (r"C:\Users\d\work\a.rs", Some("/C:/Users/d/work/a.rs")),
        (r"c:\Users\d\work\a.rs", Some("/C:/Users/d/work/a.rs")),
        // A file over 260 characters drops its `\\?\`.
        (r"\\?\D:\work\b.rs", Some("/D:/work/b.rs")),
        // A network share's path never reaches the plugin as a host string, and its text still
        // does.
        (r"\\server\share\a.rs", None),
        (r"\\?\UNC\server\share\a.rs", None),
    ] {
        set_path(&buffers, 1, host);
        assert_eq!(read_path(&mut plugin, 1).as_deref(), guest, "{host}");
        assert_eq!(
            read_path(&mut plugin, 1),
            paths::to_guest(host, Rules::Windows).ok()
        );
    }
    assert_eq!(plugin.read("1").unwrap().1, "text");

    // Under Unix rules the path is the host's own.
    let buffers = Buffers::with(2, "text");
    let mut plugin = Plugin::with_path_rules(&["buffer.read"], &buffers, Rules::Unix);
    assert_eq!(read_path(&mut plugin, 2).as_deref(), Some("/work/2.txt"));
}

#[test]
fn an_edit_of_a_buffer_at_a_windows_host_path_applies() {
    let buffers = Buffers::with(1, "one two");
    set_path(&buffers, 1, r"C:\Users\d\work\a.rs");
    let mut plugin =
        Plugin::with_path_rules(&["buffer.read", "buffer.write"], &buffers, Rules::Windows);
    let (version, _) = plugin.read("1").unwrap();
    let after = plugin.run("edit", &format!("1 {version}\n0 3 1")).unwrap();
    assert_eq!(text(&buffers, 1), "1 two");
    let snapshot = plugin.store.data_mut().read(1, None).unwrap();
    assert_eq!(snapshot.version.to_string(), after);
    assert_eq!(snapshot.path.as_deref(), Some("/C:/Users/d/work/a.rs"));
    // The guest path maps back to the buffer's.
    assert_eq!(
        paths::to_host(snapshot.path.as_deref().unwrap(), Rules::Windows).as_deref(),
        Ok(r"C:\Users\d\work\a.rs")
    );

    // A buffer on a network share is edited too, its path withheld.
    set_path(&buffers, 1, r"\\server\share\a.rs");
    let (version, _) = plugin.read("1").unwrap();
    plugin.run("edit", &format!("1 {version}\n2 5 2")).unwrap();
    assert_eq!(text(&buffers, 1), "1 2");
    assert_eq!(read_path(&mut plugin, 1), None);
}
