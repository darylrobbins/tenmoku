//! The `fastedit:editor/decorations` imports, called as a plugin's store calls them, against
//! buffers held here: what the host keeps, how it maps sets through edits, and that the editor's
//! provider reads the host's copy whatever the plugin is doing.

// Decorations are compared as ranges, often one.
#![allow(clippy::single_range_in_vec_init)]

use std::collections::HashMap;
use std::ops::Range;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use fe_buffer::{Buffer, BufferSnapshot, Origin};
use fe_editor::decorations::{Decoration as Painted, DecorationKind, DecorationProvider, Tint};
use fe_plugin::decorations::{
    BufferVersions, DecorationStore, MAX_ENTRIES_PER_BUFFER, PluginDecorations, PluginGutterEntry,
    State,
};
use fe_plugin::fastedit::editor::decorations::{
    Decoration, DecorationStyle, GutterEntry, Host as _, Range as WitRange,
};
use fe_plugin::{EditorExtension, HostState};
use kiln_command::capability::{Capability, Grants};
use kiln_plugin::bindings::Error;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};

const PLUGIN: &str = "deco-test";

/// An open buffer, with each snapshot plugins were shown of it.
struct Open {
    buffer: Buffer,
    shown: Vec<BufferSnapshot>,
}

/// The buffers plugins reach. A plugin sees version `n` for the `n`th snapshot shown.
#[derive(Default)]
struct Buffers {
    open: HashMap<u64, Open>,
}

impl Buffers {
    fn with(id: u64, text: &str) -> Arc<Mutex<Self>> {
        let buffers = Arc::new(Mutex::new(Self::default()));
        open(&buffers, id, text);
        buffers
    }
}

impl BufferVersions for Buffers {
    fn now(&mut self, buffer: u64) -> Option<BufferSnapshot> {
        Some(self.open.get(&buffer)?.buffer.snapshot())
    }

    fn at(&mut self, buffer: u64, version: u64) -> Option<BufferSnapshot> {
        let open = self.open.get(&buffer)?;
        let index = usize::try_from(version.checked_sub(1)?).ok()?;
        open.shown.get(index).cloned()
    }
}

fn open(buffers: &Arc<Mutex<Buffers>>, id: u64, text: &str) {
    buffers.lock().unwrap().open.insert(
        id,
        Open {
            buffer: Buffer::new(text),
            shown: Vec::new(),
        },
    );
}

/// The version of buffer `id` a plugin sees now, as a read would show it.
fn shown(buffers: &Arc<Mutex<Buffers>>, id: u64) -> u64 {
    let mut buffers = buffers.lock().unwrap();
    let open = buffers.open.get_mut(&id).unwrap();
    let now = open.buffer.snapshot();
    if open.shown.last().map(BufferSnapshot::version) != Some(now.version()) {
        open.shown.push(now);
    }
    open.shown.len() as u64
}

fn snapshot(buffers: &Arc<Mutex<Buffers>>, id: u64) -> BufferSnapshot {
    buffers.lock().unwrap().open[&id].buffer.snapshot()
}

/// Replaces `range` of buffer `id` with `text`, as the user.
fn type_in(buffers: &Arc<Mutex<Buffers>>, id: u64, range: Range<usize>, text: &str) {
    let mut buffers = buffers.lock().unwrap();
    let buffer = &mut buffers.open.get_mut(&id).unwrap().buffer;
    buffer.edit([(range, text)], Origin::User).unwrap();
}

/// Plugin `plugin`'s store state, holding `grants`, on `store` over `buffers`.
fn plugin(
    plugin: &str,
    grants: &[&str],
    store: &DecorationStore,
    buffers: &Arc<Mutex<Buffers>>,
) -> HostState {
    let grants: Grants = grants
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect();
    HostState {
        decorations: State::new(plugin, &grants, store, Box::new(Arc::clone(buffers))),
        ..HostState::default()
    }
}

fn decoration(range: Range<u64>, style: DecorationStyle) -> Decoration {
    Decoration {
        range: WitRange {
            start: range.start,
            end: range.end,
        },
        style,
        color: None,
        hover: None,
    }
}

fn highlight(range: Range<u64>) -> Decoration {
    decoration(range, DecorationStyle::Highlight)
}

fn gutter(line: u32, symbol: &str) -> GutterEntry {
    GutterEntry {
        line,
        symbol: symbol.into(),
        color: None,
        tooltip: None,
    }
}

/// What `provider` gives for all of `snapshot`.
fn provided(provider: &PluginDecorations, snapshot: &BufferSnapshot) -> Vec<Painted> {
    let mut out = Vec::new();
    provider.decorations(snapshot, 0..snapshot.len(), &mut out);
    out
}

fn ranges(painted: &[Painted]) -> Vec<Range<usize>> {
    painted
        .iter()
        .map(|painted| painted.range.clone())
        .collect()
}

/// The lines and symbols of every gutter entry on buffer `id` now.
fn gutter_lines(
    store: &DecorationStore,
    buffers: &Arc<Mutex<Buffers>>,
    id: u64,
) -> Vec<(u32, String)> {
    let entries: Vec<PluginGutterEntry> = store.gutter_entries(id, &snapshot(buffers, id), 0..100);
    entries
        .into_iter()
        .map(|entry| (entry.line, entry.symbol))
        .collect()
}

#[test]
fn a_set_at_the_current_version_is_provided() {
    let buffers = Buffers::with(1, "hello world\nsecond line\n");
    let store = DecorationStore::new();
    let mut state = plugin(PLUGIN, &["decorations"], &store, &buffers);
    let version = shown(&buffers, 1);
    let mut squiggle = decoration(6..11, DecorationStyle::Squiggle);
    squiggle.color = Some("diagnostic.warning".into());
    squiggle.hover = Some("did you mean `word`?".into());
    state
        .set_decorations(1, version, "lint".into(), vec![highlight(0..5), squiggle])
        .unwrap();
    let mut marked = gutter(1, "dot");
    marked.tooltip = Some("changed".into());
    state
        .set_gutter(1, version, "lint".into(), vec![marked])
        .unwrap();

    let provider = store.provider(1);
    let now = snapshot(&buffers, 1);
    assert_eq!(
        provided(&provider, &now),
        [
            Painted::new(0..5, DecorationKind::Highlight, Tint::Accent),
            Painted::new(
                6..11,
                DecorationKind::Underline { wavy: true },
                Tint::Warning
            ),
        ]
    );
    // Only those overlapping the range asked for.
    let mut out = Vec::new();
    provider.decorations(&now, 7..9, &mut out);
    assert_eq!(ranges(&out), [6..11]);

    // The whole record, for hovers.
    let read = store.decorations(1, &now, 0..now.len());
    assert_eq!(read.len(), 2);
    assert_eq!(read[1].plugin, PLUGIN);
    assert_eq!(read[1].set, "lint");
    assert_eq!(read[1].hover.as_deref(), Some("did you mean `word`?"));
    let entries = store.gutter_entries(1, &now, 0..2);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        (entries[0].line, entries[0].tooltip.as_deref()),
        (1, Some("changed"))
    );
    assert!(store.gutter_entries(1, &now, 0..1).is_empty());

    // Kept anchored: they follow the user's typing.
    type_in(&buffers, 1, 0..0, "¡x\n");
    let now = snapshot(&buffers, 1);
    assert_eq!(ranges(&provided(&provider, &now)), [4..9, 10..15]);
    assert_eq!(gutter_lines(&store, &buffers, 1), [(2, "dot".to_owned())]);

    // Setting a set again replaces it; clearing it removes both kinds.
    let version = shown(&buffers, 1);
    state
        .set_decorations(1, version, "lint".into(), vec![highlight(0..2)])
        .unwrap();
    assert_eq!(ranges(&provided(&provider, &now)), [0..2]);
    state.clear(1, "lint".into()).unwrap();
    assert!(provided(&provider, &now).is_empty());
    assert!(gutter_lines(&store, &buffers, 1).is_empty());
    // Another buffer's provider sees none of it.
    assert!(provided(&store.provider(2), &now).is_empty());
}

#[test]
fn a_set_for_an_older_version_is_mapped_through_edits() {
    let buffers = Buffers::with(1, "alpha beta gamma\nline two\nline three\n");
    let store = DecorationStore::new();
    let mut state = plugin(PLUGIN, &["decorations"], &store, &buffers);
    let old = shown(&buffers, 1);

    // The user edits while the plugin works: an insert before every range, then two deletions.
    type_in(&buffers, 1, 0..0, "XX ");
    type_in(&buffers, 1, 14..19, "");
    type_in(&buffers, 1, 15..24, "");
    let now = snapshot(&buffers, 1);
    assert_eq!(now.text().to_string(), "XX alpha beta \nline three\n");

    state
        .set_decorations(
            1,
            old,
            "words".into(),
            vec![
                // beta, shifted by the insert.
                highlight(6..10),
                // gamma, deleted.
                highlight(11..16),
                // Empty: inside gamma, and at its start.
                highlight(13..13),
                highlight(11..11),
            ],
        )
        .unwrap();
    state
        .set_gutter(
            1,
            old,
            "lines".into(),
            vec![gutter(1, "deleted"), gutter(2, "moved")],
        )
        .unwrap();

    let provider = store.provider(1);
    assert_eq!(ranges(&provided(&provider, &now)), [9..13, 14..14]);
    assert_eq!(&now.text().to_string()[9..13], "beta");
    assert_eq!(gutter_lines(&store, &buffers, 1), [(1, "moved".to_owned())]);

    // Ranges are checked against the text at the version named: 31..36 was there, not now.
    state
        .set_decorations(1, old, "late".into(), vec![highlight(31..36)])
        .unwrap();
    let late = store.decorations(1, &now, 0..now.len());
    let late: Vec<_> = late.iter().filter(|read| read.set == "late").collect();
    assert_eq!(late.len(), 1);
    assert_eq!(&now.text().to_string()[late[0].range.clone()], "three");
    let past = state.set_decorations(1, old, "late".into(), vec![highlight(31..38)]);
    assert!(matches!(past, Err(Error::InvalidArgument(_))), "{past:?}");

    // A version plugins were never shown, and a buffer that isn't open, can't be mapped.
    let future = state.set_decorations(1, old + 5, "words".into(), vec![highlight(0..1)]);
    assert!(matches!(future, Err(Error::Conflict(_))), "{future:?}");
    let missing = state.set_gutter(9, old, "lines".into(), vec![gutter(0, "x")]);
    assert!(matches!(missing, Err(Error::NotFound(_))), "{missing:?}");
    // A failed set leaves the set as it was.
    assert_eq!(gutter_lines(&store, &buffers, 1), [(1, "moved".to_owned())]);
}

#[test]
fn the_provider_never_calls_the_plugin() {
    /// kiln-plugin's fixture that imports every kiln:extension interface and logs from `init`.
    const STRINGS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../kiln/crates/kiln-plugin/tests/fixtures/strings.wat"
    );

    let buffers = Buffers::with(1, "stuck plugin");
    let store = DecorationStore::new();
    let version = shown(&buffers, 1);

    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::from_file(&engine, STRINGS).unwrap();
    let mut linker = Linker::new(&engine);
    fe_plugin::add_to_linker(&mut linker).unwrap();

    // The plugin's `init` logs, and its log sink holds the call until the test lets it go: at
    // least 100 ms, at most 1.5 s.
    let (entered, in_call) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let released = Mutex::new(released);
    let mut state = plugin(PLUGIN, &["decorations"], &store, &buffers);
    state.kiln = kiln_plugin::bindings::HostState::new(Box::new(move |_, _| {
        entered.send(()).unwrap();
        thread::sleep(Duration::from_millis(100));
        let _ = released
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_millis(1_400));
    }));
    state
        .set_decorations(1, version, "set".into(), vec![highlight(0..5)])
        .unwrap();
    let mut wasm = Store::new(&engine, state);
    let extension = EditorExtension::instantiate(&mut wasm, &component, &linker).unwrap();

    let call = thread::spawn(move || {
        assert_eq!(extension.call_init(&mut wasm).unwrap(), Ok(()));
        wasm
    });
    in_call.recv_timeout(Duration::from_millis(1_500)).unwrap();
    // The plugin is inside its call, and the provider answers from the host's copy.
    let provider = store.provider(1);
    let now = snapshot(&buffers, 1);
    assert_eq!(ranges(&provided(&provider, &now)), [0..5]);
    assert!(!call.is_finished(), "the plugin's call is still running");
    release.send(()).unwrap();
    let wasm = call.join().unwrap();

    // With the plugin's instance gone, the host still holds its sets, until it is unloaded.
    drop(wasm);
    assert_eq!(ranges(&provided(&provider, &now)), [0..5]);
    store.unload(PLUGIN);
    assert!(provided(&provider, &now).is_empty());
}

#[test]
fn pushing_needs_the_decorations_capability() {
    let buffers = Buffers::with(1, "text");
    let store = DecorationStore::new();
    let mut state = plugin(PLUGIN, &["buffer.read", "status"], &store, &buffers);
    let version = shown(&buffers, 1);
    let set = state.set_decorations(1, version, "set".into(), vec![highlight(0..4)]);
    assert!(
        matches!(&set, Err(Error::Denied(why)) if why.contains("decorations")),
        "{set:?}"
    );
    let gutter = state.set_gutter(1, version, "set".into(), vec![gutter(0, "x")]);
    assert!(
        matches!(&gutter, Err(Error::Denied(why)) if why.contains("decorations")),
        "{gutter:?}"
    );
    let clear = state.clear(1, "set".into());
    assert!(matches!(clear, Err(Error::Denied(_))), "{clear:?}");
    assert!(provided(&store.provider(1), &snapshot(&buffers, 1)).is_empty());
    assert!(gutter_lines(&store, &buffers, 1).is_empty());

    // A store with no decorations slice doesn't implement them.
    let set = HostState::default().set_decorations(1, version, "set".into(), Vec::new());
    assert!(matches!(set, Err(Error::Unimplemented(_))), "{set:?}");
}

#[test]
fn entries_are_capped_per_buffer() {
    assert_eq!(MAX_ENTRIES_PER_BUFFER, 10_000);
    let buffers = Buffers::with(1, &"a\n".repeat(MAX_ENTRIES_PER_BUFFER + 1));
    open(&buffers, 2, "b");
    let store = DecorationStore::new();
    let mut state = plugin(PLUGIN, &["decorations"], &store, &buffers);
    let version = shown(&buffers, 1);
    let each = |count: u64| {
        (0..count)
            .map(|line| highlight(line * 2..line * 2 + 1))
            .collect()
    };

    let over = state.set_decorations(1, version, "all".into(), each(10_001));
    assert!(
        matches!(&over, Err(Error::Failed(why)) if why.contains("10000")),
        "{over:?}"
    );
    let provider = store.provider(1);
    let now = snapshot(&buffers, 1);
    assert!(provided(&provider, &now).is_empty());

    state
        .set_decorations(1, version, "all".into(), each(10_000))
        .unwrap();
    assert_eq!(provided(&provider, &now).len(), 10_000);
    // The cap is over all the plugin's sets on the buffer, gutter entries included.
    let more = state.set_decorations(1, version, "more".into(), each(1));
    assert!(matches!(more, Err(Error::Failed(_))), "{more:?}");
    let gutter = state.set_gutter(1, version, "more".into(), vec![gutter(0, "x")]);
    assert!(matches!(gutter, Err(Error::Failed(_))), "{gutter:?}");
    // Replacing the full set counts it once.
    state
        .set_decorations(1, version, "all".into(), each(10_000))
        .unwrap();
    // Another buffer has its own.
    let other = shown(&buffers, 2);
    state
        .set_decorations(2, other, "all".into(), vec![highlight(0..1)])
        .unwrap();
    // As has another plugin.
    let mut second = plugin("second", &["decorations"], &store, &buffers);
    second
        .set_decorations(1, version, "all".into(), each(1))
        .unwrap();
    assert_eq!(provided(&provider, &now).len(), 10_001);
}

#[test]
fn unloading_clears_the_plugins_sets() {
    let buffers = Buffers::with(1, "one two\nthree\n");
    let store = DecorationStore::new();
    let mut first = plugin(PLUGIN, &["decorations"], &store, &buffers);
    let mut second = plugin("second", &["decorations"], &store, &buffers);
    let version = shown(&buffers, 1);
    first
        .set_decorations(1, version, "set".into(), vec![highlight(0..3)])
        .unwrap();
    first
        .set_gutter(1, version, "set".into(), vec![gutter(0, "first")])
        .unwrap();
    second
        .set_decorations(1, version, "set".into(), vec![highlight(4..7)])
        .unwrap();
    second
        .set_gutter(1, version, "set".into(), vec![gutter(1, "second")])
        .unwrap();
    let provider = store.provider(1);
    let now = snapshot(&buffers, 1);
    assert_eq!(ranges(&provided(&provider, &now)), [0..3, 4..7]);

    store.unload(PLUGIN);
    assert_eq!(ranges(&provided(&provider, &now)), [4..7]);
    let read = store.decorations(1, &now, 0..now.len());
    assert!(read.iter().all(|read| read.plugin == "second"), "{read:?}");
    assert_eq!(
        gutter_lines(&store, &buffers, 1),
        [(1, "second".to_owned())]
    );

    // What the unloaded plugin's store still holds sets nothing.
    let after = first.set_decorations(1, version, "set".into(), vec![highlight(0..3)]);
    assert!(matches!(after, Err(Error::Denied(_))), "{after:?}");
    assert_eq!(ranges(&provided(&provider, &now)), [4..7]);

    // Loaded again, it starts with no sets, and the old load still sets nothing.
    let mut again = plugin(PLUGIN, &["decorations"], &store, &buffers);
    assert_eq!(ranges(&provided(&provider, &now)), [4..7]);
    again
        .set_decorations(1, version, "set".into(), vec![highlight(8..13)])
        .unwrap();
    assert_eq!(ranges(&provided(&provider, &now)), [8..13, 4..7]);
    let stale = first.clear(1, "set".into());
    assert!(matches!(stale, Err(Error::Denied(_))), "{stale:?}");
}

/// Undoes or redoes buffer `id`'s last change, as the user.
fn history(buffers: &Arc<Mutex<Buffers>>, id: u64, redo: bool) {
    let mut buffers = buffers.lock().unwrap();
    let buffer = &mut buffers.open.get_mut(&id).unwrap().buffer;
    let user = |origin: &Origin| *origin == Origin::User;
    let done = if redo {
        buffer.redo(user)
    } else {
        buffer.undo(user)
    };
    assert!(done.unwrap().is_some());
}

/// Plugin `plugin`'s decorations on buffer `id` now, by start.
fn ranges_of(
    store: &DecorationStore,
    buffers: &Arc<Mutex<Buffers>>,
    id: u64,
    plugin: &str,
) -> Vec<Range<usize>> {
    let now = snapshot(buffers, id);
    let read = store.decorations(id, &now, 0..now.len());
    let mut ranges: Vec<_> = read
        .into_iter()
        .filter(|read| read.plugin == plugin)
        .map(|read| read.range)
        .collect();
    ranges.sort_by_key(|range| (range.start, range.end));
    ranges
}

/// Plugin `plugin`'s gutter lines on buffer `id` now.
fn lines_of(
    store: &DecorationStore,
    buffers: &Arc<Mutex<Buffers>>,
    id: u64,
    plugin: &str,
) -> Vec<u32> {
    let entries = store.gutter_entries(id, &snapshot(buffers, id), 0..100);
    entries
        .into_iter()
        .filter(|entry| entry.plugin == plugin)
        .map(|entry| entry.line)
        .collect()
}

#[test]
fn held_sets_lose_what_edits_delete() {
    let buffers = Buffers::with(1, "abcdef");
    open(&buffers, 2, "one\ntwo\nthree\n");
    let store = DecorationStore::new();
    let mut held = plugin("held", &["decorations"], &store, &buffers);
    let mut late = plugin("late", &["decorations"], &store, &buffers);
    let (old, old_lines) = (shown(&buffers, 1), shown(&buffers, 2));
    let words = || {
        vec![
            highlight(1..3),
            highlight(3..3),
            highlight(0..0),
            highlight(4..6),
        ]
    };
    let lines = || vec![gutter(0, "0"), gutter(1, "1"), gutter(2, "2")];
    held.set_decorations(1, old, "set".into(), words()).unwrap();
    held.set_gutter(2, old_lines, "set".into(), lines())
        .unwrap();

    type_in(&buffers, 1, 1..5, "");
    type_in(&buffers, 2, 4..8, "");
    // Pushed before the edits or after them, against the same version, a set ends up the same:
    // "bc" and the empty decoration inside "bcde" are gone, and so is line "two".
    late.set_decorations(1, old, "set".into(), words()).unwrap();
    late.set_gutter(2, old_lines, "set".into(), lines())
        .unwrap();
    for plugin in ["held", "late"] {
        assert_eq!(
            ranges_of(&store, &buffers, 1, plugin),
            [0..0, 1..2],
            "{plugin}"
        );
        assert_eq!(lines_of(&store, &buffers, 2, plugin), [0, 1], "{plugin}");
    }
    // Refreshed, the held sets keep the same.
    store.provider(1).refresh(&snapshot(&buffers, 1));
    store.provider(2).refresh(&snapshot(&buffers, 2));
    assert_eq!(ranges_of(&store, &buffers, 1, "held"), [0..0, 1..2]);
    assert_eq!(lines_of(&store, &buffers, 2, "held"), [0, 1]);

    // What edits deleted no longer counts toward the cap.
    let buffers = Buffers::with(3, &"ab".repeat(MAX_ENTRIES_PER_BUFFER / 2));
    let version = shown(&buffers, 3);
    let mut full = plugin("full", &["decorations"], &store, &buffers);
    let each = (0..MAX_ENTRIES_PER_BUFFER as u64)
        .map(|at| highlight(at..at + 1))
        .collect();
    full.set_decorations(3, version, "all".into(), each)
        .unwrap();
    let more = full.set_decorations(3, version, "more".into(), vec![highlight(0..1)]);
    assert!(matches!(more, Err(Error::Failed(_))), "{more:?}");
    type_in(&buffers, 3, 0..MAX_ENTRIES_PER_BUFFER, "x");
    let version = shown(&buffers, 3);
    full.set_decorations(3, version, "more".into(), vec![highlight(0..1)])
        .unwrap();
    assert_eq!(ranges_of(&store, &buffers, 3, "full"), [0..1]);
}

#[test]
fn undone_edits_map_as_anchors_do() {
    let buffers = Buffers::with(1, "abcdef");
    open(&buffers, 2, "one\ntwo\nthree\n");
    let store = DecorationStore::new();
    let mut held = plugin("held", &["decorations"], &store, &buffers);
    let (old, old_lines) = (shown(&buffers, 1), shown(&buffers, 2));
    let words = || vec![highlight(2..5), highlight(1..2), highlight(3..3)];
    let lines = || vec![gutter(1, "1"), gutter(2, "2")];
    held.set_decorations(1, old, "set".into(), words()).unwrap();
    held.set_gutter(2, old_lines, "set".into(), lines())
        .unwrap();

    // Deleted and undone: the text is as it was, and so is everything on it.
    type_in(&buffers, 1, 0..3, "");
    history(&buffers, 1, false);
    type_in(&buffers, 2, 0..8, "");
    history(&buffers, 2, false);
    let mut late = plugin("late", &["decorations"], &store, &buffers);
    late.set_decorations(1, old, "set".into(), words()).unwrap();
    late.set_gutter(2, old_lines, "set".into(), lines())
        .unwrap();
    for plugin in ["held", "late"] {
        assert_eq!(
            ranges_of(&store, &buffers, 1, plugin),
            [1..2, 2..5, 3..3],
            "{plugin}"
        );
        assert_eq!(lines_of(&store, &buffers, 2, plugin), [1, 2], "{plugin}");
    }

    // Redone, the deletion takes "b" and leaves the rest where it would have.
    history(&buffers, 1, true);
    history(&buffers, 2, true);
    let mut redone = plugin("redone", &["decorations"], &store, &buffers);
    redone
        .set_decorations(1, old, "set".into(), words())
        .unwrap();
    redone
        .set_gutter(2, old_lines, "set".into(), lines())
        .unwrap();
    assert_eq!(snapshot(&buffers, 1).text().to_string(), "def");
    for plugin in ["held", "redone"] {
        assert_eq!(
            ranges_of(&store, &buffers, 1, plugin),
            [0..0, 0..2],
            "{plugin}"
        );
        assert_eq!(lines_of(&store, &buffers, 2, plugin), [0], "{plugin}");
    }
}

#[test]
fn replacing_what_an_entry_sits_on_takes_it() {
    let buffers = Buffers::with(1, "abcdef");
    open(&buffers, 2, "one\ntwo\nthree\n");
    let store = DecorationStore::new();
    let mut held = plugin("held", &["decorations"], &store, &buffers);
    let mut late = plugin("late", &["decorations"], &store, &buffers);
    let (old, old_lines) = (shown(&buffers, 1), shown(&buffers, 2));
    let words = || vec![highlight(3..3), highlight(2..4), highlight(0..1)];
    let lines = || vec![gutter(0, "0"), gutter(1, "1"), gutter(2, "2")];
    held.set_decorations(1, old, "set".into(), words()).unwrap();
    held.set_gutter(2, old_lines, "set".into(), lines())
        .unwrap();

    // "cd" is replaced, not deleted: the empty decoration between "c" and "d" and the one over
    // "cd" are gone, though the span they had left isn't empty. Line "two" is replaced whole.
    type_in(&buffers, 1, 2..4, "X");
    type_in(&buffers, 2, 4..8, "2\n");
    late.set_decorations(1, old, "set".into(), words()).unwrap();
    late.set_gutter(2, old_lines, "set".into(), lines())
        .unwrap();
    for plugin in ["held", "late"] {
        assert_eq!(ranges_of(&store, &buffers, 1, plugin), [0..1], "{plugin}");
        assert_eq!(lines_of(&store, &buffers, 2, plugin), [0, 2], "{plugin}");
    }
    store.provider(1).refresh(&snapshot(&buffers, 1));
    store.provider(2).refresh(&snapshot(&buffers, 2));
    assert_eq!(ranges_of(&store, &buffers, 1, "held"), [0..1]);
    assert_eq!(lines_of(&store, &buffers, 2, "held"), [0, 2]);
}

#[test]
fn an_empty_entry_that_moves_inside_the_text_is_taken_with_it() {
    let buffers = Buffers::with(1, "abcdef");
    let store = DecorationStore::new();
    let mut held = plugin("held", &["decorations"], &store, &buffers);
    let mut late = plugin("late", &["decorations"], &store, &buffers);
    let old = shown(&buffers, 1);
    held.set_decorations(1, old, "set".into(), vec![highlight(0..0)])
        .unwrap();
    // Text typed at the start moves it in; deleting that and everything after it takes it.
    type_in(&buffers, 1, 0..0, "X");
    assert_eq!(ranges_of(&store, &buffers, 1, "held"), [1..1]);
    type_in(&buffers, 1, 0..7, "");
    late.set_decorations(1, old, "set".into(), vec![highlight(0..0)])
        .unwrap();
    for plugin in ["held", "late"] {
        assert_eq!(ranges_of(&store, &buffers, 1, plugin), [], "{plugin}");
    }
}
