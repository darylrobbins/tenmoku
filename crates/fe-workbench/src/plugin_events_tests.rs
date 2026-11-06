//! The editor's events reaching plugins: a buffer's life, the focus, the
//! selection and reloads, each only to a plugin subscribed to its kind, on
//! the plugin's own thread, and a plugin that falls behind never holding
//! up typing. The plugin subscribes with the `subscribe` import in its
//! `init`; what it was handed is recorded by the host as it delivers.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, Origin};
use fe_command::capability::Grants;
use fe_config::SettingsStore;
use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use gpui::{AppContext as _, Entity, Focusable as _, TestAppContext, WindowHandle};
use kiln::{
    Config,
    core::{
        ApiVersion,
        extension::{ExtensionApi, ExtensionSet, load_manifest},
    },
    platform::config::{BatchSender, batch_channel},
};
use kiln_plugin::{
    bindings::kiln::extension::{
        events::{BufferChanged, BufferOpened, BufferSaved, Event, SelectionChanged},
        types::Range,
    },
    events::{Change, Edit, HostEvent},
    grants::GrantStore,
    manifest::{ExtensionOrigin, PluginManifest},
};

use super::Inbox;
use crate::{
    Launch, Workbench,
    plugin_bridge::{buffer_id, snapshot},
    plugin_host::{
        Found, GRANTS_FILE, PluginHost, PluginState, plugin_inbox, plugin_states, set_plugin_dirs,
        set_plugin_extensions, set_plugins, set_preview,
    },
};

// The `event-kinds` flags, as the guest lowers them.
const OPENED: u32 = 1;
const CHANGED: u32 = 1 << 1;
const SAVED: u32 = 1 << 2;
const CLOSED: u32 = 1 << 3;
const FOCUS: u32 = 1 << 4;
const SELECTION: u32 = 1 << 5;
const CONFIG: u32 = 1 << 6;

/// The listening plugin's id.
const LISTENER: &str = "acme.listener";

/// A plugin that subscribes to `kinds` in its `init` and takes every event
/// it's handed.
fn listener(kinds: u32) -> String {
    format!(
        r#"
(component $C
  (import "kiln:extension/types@1.0.0" (instance $types
    (type $range (record (field "start" u64) (field "end" u64)))
    (export "range" (type (eq $range)))
    (type $error (variant
      (case "unimplemented" string)
      (case "denied" string)
      (case "not-found" string)
      (case "invalid-argument" string)
      (case "conflict" string)
      (case "failed" string)))
    (export "error" (type (eq $error)))))
  (alias export $types "range" (type $range))
  (alias export $types "error" (type $error))

  (import "kiln:extension/events@1.0.0" (instance $events
    (alias outer $C $range (type $range))
    (alias outer $C $error (type $error))
    (type $event-kinds (flags
      "buffer-opened" "buffer-changed" "buffer-saved" "buffer-closed"
      "focus-changed" "selection-changed" "config-reloaded"))
    (export "event-kinds" (type $event-kinds' (eq $event-kinds)))
    (type $buffer-opened (record
      (field "buffer" u64)
      (field "version" u64)
      (field "path" (option string))
      (field "language" (option string))))
    (export "buffer-opened" (type $buffer-opened' (eq $buffer-opened)))
    (type $buffer-changed (record
      (field "buffer" u64) (field "version" u64) (field "ranges" (list $range))))
    (export "buffer-changed" (type $buffer-changed' (eq $buffer-changed)))
    (type $buffer-saved (record
      (field "buffer" u64) (field "version" u64) (field "path" string)))
    (export "buffer-saved" (type $buffer-saved' (eq $buffer-saved)))
    (type $selection-changed (record
      (field "buffer" u64) (field "version" u64) (field "ranges" (list $range))))
    (export "selection-changed" (type $selection-changed' (eq $selection-changed)))
    (type $event (variant
      (case "buffer-opened" $buffer-opened')
      (case "buffer-changed" $buffer-changed')
      (case "buffer-saved" $buffer-saved')
      (case "buffer-closed" u64)
      (case "focus-changed" (option u64))
      (case "selection-changed" $selection-changed')
      (case "config-reloaded")
      (case "lagged" u64)))
    (export "event" (type (eq $event)))
    (export "subscribe" (func (param "kinds" $event-kinds') (result (result (error $error)))))))
  (alias export $events "event" (type $event))

  (core module $libc
    (memory (export "memory") 1)
    ;; 64.. holds `subscribe`'s result, which `init` returns; the heap
    ;; starts at 1024.
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32) (param $align i32) (param $size i32) (result i32)
      (local $p i32)
      (local.set $p
        (i32.and
          (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
          (i32.sub (i32.const 0) (local.get $align))))
      (global.set $heap (i32.add (local.get $p) (local.get $size)))
      (local.get $p)))
  (core instance $libc (instantiate $libc))
  (alias core export $libc "memory" (core memory $memory))
  (alias core export $libc "realloc" (core func $realloc))

  (alias export $events "subscribe" (func $events-subscribe))
  (core func $subscribe (canon lower (func $events-subscribe) (memory $memory) (realloc $realloc)))
  (core instance $host (export "subscribe" (func $subscribe)))

  (core module $main
    (import "libc" "memory" (memory 1))
    (import "host" "subscribe" (func $subscribe (param i32 i32)))
    (func (export "init") (result i32)
      (call $subscribe (i32.const {kinds}) (i32.const 64))
      (i32.const 64))
    (func (export "shutdown"))
    (func (export "run-command") (param i32 i32 i32 i32) (result i32) (i32.const 0))
    (func (export "handle-events") (param i32 i32)))
  (core instance $main (instantiate $main
    (with "libc" (instance $libc))
    (with "host" (instance $host))))

  (func (export "init") (result (result (error $error)))
    (canon lift (core func $main "init") (memory $memory)))
  (func (export "shutdown")
    (canon lift (core func $main "shutdown")))

  (func $run-command (param "id" string) (param "args" string) (result (result (error $error)))
    (canon lift (core func $main "run-command") (memory $memory) (realloc $realloc)))
  (instance $command-handler (export "run-command" (func $run-command)))
  (export "kiln:extension/command-handler@1.0.0" (instance $command-handler))

  (func $handle-events (param "events" (list $event))
    (canon lift (core func $main "handle-events") (memory $memory) (realloc $realloc)))
  (instance $event-handler (export "handle-events" (func $handle-events)))
  (export "kiln:extension/event-handler@1.0.0" (instance $event-handler)))
"#
    )
}

/// A test's folder: `a.txt` and `b.txt`, and the plugins' state, cache and
/// extensions.
struct Dir(tempfile::TempDir);

impl Dir {
    fn path(&self) -> &Path {
        self.0.path()
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path().join(name)
    }

    /// The listener, subscribing to `kinds`, granted all it asks: nothing.
    fn install(&self, kinds: u32) -> Found {
        let dir = self.path().join("extensions").join(LISTENER);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plugin.wasm"), listener(kinds)).unwrap();
        fs::write(
            dir.join("extension.toml"),
            format!(
                "id = \"{LISTENER}\"\nname = \"{LISTENER}\"\nversion = \"1.0.0\"\n\
                 license = \"MIT\"\n\n[engine]\ntenmoku = \"1.1\"\n\n\
                 [contributes.plugin]\nwasm = \"plugin.wasm\"\ncapabilities = []\n"
            ),
        )
        .unwrap();
        GrantStore::new(self.path().join("state").join(GRANTS_FILE))
            .grant(LISTENER, (1, 0, 0), &Grants::new(), &[], false)
            .unwrap();
        let api = ExtensionApi {
            app: fe_config::paths::app_id().short,
            version: ApiVersion { major: 1, minor: 1 },
            reserved: Vec::new(),
            kinds: vec![crate::plugin_host::plugin_kind()],
            allowed_licenses: None,
            only_kinds: None,
        };
        let (manifest, diagnostics) = load_manifest(&dir, &api);
        let manifest = manifest.unwrap_or_else(|| panic!("{diagnostics:#?}"));
        let plugin = PluginManifest::from_extension(&manifest, ExtensionOrigin::User)
            .unwrap()
            .unwrap();
        Found::new(&manifest, Ok(plugin))
    }
}

/// The user's settings and keymap files, in a test's folder, and kiln's
/// watcher of them.
struct Settings {
    path: PathBuf,
    keymap: PathBuf,
    sender: BatchSender,
}

impl Settings {
    /// Writes `toml` as the settings and has kiln reload them, as its
    /// watcher would.
    fn reload(&self, toml: &str, cx: &mut TestAppContext) {
        self.write(&self.path, toml, cx);
    }

    /// Writes `toml` as the keymap and has kiln reload it.
    fn reload_keymap(&self, toml: &str, cx: &mut TestAppContext) {
        self.write(&self.keymap, toml, cx);
    }

    fn write(&self, path: &Path, toml: &str, cx: &mut TestAppContext) {
        fs::write(path, toml).unwrap();
        self.sender.send(vec![path.to_owned()]);
        cx.run_until_parked();
    }
}

const PREVIEW: &str = "[extensions]\nplugins_preview = true\n";

/// The workbench's commands, a folder with `a.txt` and `b.txt`, kiln's
/// settings from the user's file there, and the plugin host keeping its
/// files there, with the preview on and a call deadline a loaded machine
/// meets.
fn start(cx: &mut TestAppContext) -> (Dir, Settings) {
    // Plugins run on threads of their own.
    cx.executor().allow_parking();
    let dir = Dir(tempfile::tempdir().unwrap());
    fs::write(dir.file("a.txt"), "alpha\n").unwrap();
    fs::write(dir.file("b.txt"), "beta\n").unwrap();
    let path = dir.file("settings.toml");
    fs::write(&path, PREVIEW).unwrap();
    let keymap = dir.file("keymap.toml");
    fs::write(&keymap, "").unwrap();
    let store = SettingsStore::load(Some(path.clone()), None);
    let (sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(store.into_inner())
        .user_keymap(keymap.clone())
        .watch_with(batches);
    let (state, cache) = (dir.path().join("state"), dir.path().join("cache"));
    cx.update(|cx| {
        assert_eq!(kiln::init(cx, config), []);
        fe_gpui::init(cx);
        crate::init(cx);
        set_plugin_dirs(cx, state, cache);
        set_preview(cx, true);
        cx.global_mut::<PluginHost>().limits.call_deadline = Duration::from_secs(30);
    });
    let settings = Settings {
        path,
        keymap,
        sender,
    };
    (dir, settings)
}

/// A workbench showing `file`, if any, in a window of its own, active and
/// drawn.
fn window(cx: &mut TestAppContext, file: Option<PathBuf>) -> WindowHandle<Workbench> {
    let file = file.map(|path| {
        let text = fs::read_to_string(&path).unwrap();
        (path, Some(Buffer::new(&text).into()))
    });
    let launch = Launch { root: None, file };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    window
        .update(cx, |workbench, window, cx| {
            window.activate_window();
            window.focus(&workbench.focus_handle(cx), cx);
        })
        .unwrap();
    draw(cx, window);
    window
}

fn draw(cx: &mut TestAppContext, window: WindowHandle<Workbench>) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
    }
}

/// Opens `path` in the window's pane, shown and focused.
fn open(cx: &mut TestAppContext, window: WindowHandle<Workbench>, path: PathBuf) {
    window
        .update(cx, |workbench, window, cx| {
            let pane = workbench.pane().clone();
            pane.update(cx, |pane, cx| pane.open(path, window, cx));
        })
        .unwrap();
    draw(cx, window);
    let editor = active_editor(cx, window);
    focus(cx, window, &editor);
}

fn focus(cx: &mut TestAppContext, window: WindowHandle<Workbench>, editor: &Entity<EditorView>) {
    window
        .update(cx, |_, window, cx| {
            window.focus(&editor.focus_handle(cx), cx)
        })
        .unwrap();
    draw(cx, window);
}

fn active_editor(cx: &mut TestAppContext, window: WindowHandle<Workbench>) -> Entity<EditorView> {
    window
        .update(cx, |workbench, _, cx| workbench.active_editor(cx).cloned())
        .unwrap()
        .unwrap()
}

fn id_of(cx: &mut TestAppContext, editor: &Entity<EditorView>) -> u64 {
    cx.update(|cx| buffer_id(editor, cx)).unwrap()
}

/// Puts `editor`'s caret, or selection, at `range`.
fn select(
    cx: &mut TestAppContext,
    window: WindowHandle<Workbench>,
    editor: &Entity<EditorView>,
    range: std::ops::Range<usize>,
) {
    editor.update(cx, |editor, cx| editor.select_and_reveal(range, cx));
    draw(cx, window);
}

/// Hands the host the listener subscribing to `kinds`, runs until it runs,
/// and returns its inbox.
fn load(cx: &mut TestAppContext, dir: &Dir, kinds: u32) -> Inbox {
    let found = dir.install(kinds);
    cx.update(|cx| set_plugins(cx, vec![found]));
    let running = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            (plugin_states(cx).iter())
                .any(|info| info.id == LISTENER && info.state == PluginState::Running)
        })
    };
    wait(cx, "the listener to load", running);
    cx.update(|cx| plugin_inbox(LISTENER, cx)).unwrap()
}

/// Runs until `done`, failing after a while that a loaded machine meets.
fn wait(cx: &mut TestAppContext, what: &str, mut done: impl FnMut(&mut TestAppContext) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "waited for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The events the listener was handed once it was handed `count`, each
/// checked to have come on its own thread.
fn delivered(cx: &mut TestAppContext, inbox: &Inbox, count: usize) -> Vec<Event> {
    wait(cx, "the deliveries", |_| inbox.deliveries().len() >= count);
    let thread = format!("plugin {LISTENER}");
    (inbox.deliveries().into_iter())
        .map(|(on, event)| {
            assert_eq!(on, thread, "{event:?}");
            event
        })
        .collect()
}

/// The changes pushed to `inbox`'s plugin.
fn changes(inbox: &Inbox) -> Vec<Change> {
    (inbox.pushed().into_iter())
        .filter_map(|event| match event {
            HostEvent::BufferChanged(change) => Some(change),
            _ => None,
        })
        .collect()
}

/// The versions of the changes and saves pushed to `inbox`'s plugin, in
/// order: `('c', version)` a change, `('s', version)` a save.
fn versions(inbox: &Inbox) -> Vec<(char, u64)> {
    (inbox.pushed().into_iter())
        .filter_map(|event| match event {
            HostEvent::BufferChanged(change) => Some(('c', change.version)),
            HostEvent::BufferSaved(saved) => Some(('s', saved.version)),
            _ => None,
        })
        .collect()
}

/// One range, `start..end`, as events list them.
fn one(start: u64, end: u64) -> Vec<Range> {
    vec![Range { start, end }]
}

#[gpui::test]
fn buffer_lifecycle_reaches_a_subscribed_plugin(cx: &mut TestAppContext) {
    let (dir, _settings) = start(cx);
    let window = window(cx, None);
    let inbox = load(cx, &dir, OPENED | CHANGED | SAVED | CLOSED);
    // Held, what's queued is what it subscribed to, each kept apart.
    inbox.hold(true);

    let a = dir.file("a.txt");
    open(cx, window, a.clone());
    let editor = active_editor(cx, window);
    let id = id_of(cx, &editor);
    // "alpha" replaced by what's typed over it: one edit.
    select(cx, window, &editor, 0..5);
    cx.simulate_input(window.into(), "x");
    draw(cx, window);
    let edit = Edit {
        replaced: Range { start: 0, end: 5 },
        inserted: 1,
    };
    let change = Change {
        buffer: id,
        version: 2,
        edits: vec![edit],
    };
    assert_eq!(changes(&inbox), [change]);
    let saving = window
        .update(cx, |_, window, cx| {
            editor.update(cx, |editor, cx| editor.save(window, cx))
        })
        .unwrap();
    draw(cx, window);
    drop(saving);
    assert_eq!(fs::read_to_string(&a).unwrap(), "x\n");
    window
        .update(cx, |workbench, window, cx| {
            let pane = workbench.pane().clone();
            pane.update(cx, |pane, cx| pane.close_active(window, cx));
        })
        .unwrap();
    draw(cx, window);
    assert!(cx.update(|cx| buffer_id(&editor, cx)).is_none(), "closed");
    // Not the focus or the selection, which moved too.
    assert_eq!(inbox.len(), 4);

    inbox.hold(false);
    let events = delivered(cx, &inbox, 4);
    let path = a.display().to_string();
    let [
        Event::BufferOpened(opened),
        Event::BufferChanged(changed),
        Event::BufferSaved(saved),
        Event::BufferClosed(closed),
    ] = events.as_slice()
    else {
        panic!("{events:#?}");
    };
    let BufferOpened {
        buffer,
        version,
        path: opened_path,
        ..
    } = opened;
    assert_eq!((*buffer, *version), (id, 1));
    assert_eq!(opened_path.as_deref(), Some(path.as_str()));
    // Replaced 0..5 with one byte: in the text after, 0..1.
    let expected = BufferChanged {
        buffer: id,
        version: 2,
        ranges: one(0, 1),
    };
    assert_eq!(*changed, expected);
    let expected = BufferSaved {
        buffer: id,
        version: 2,
        path,
    };
    assert_eq!(*saved, expected);
    assert_eq!(*closed, id);
}

#[gpui::test]
fn focus_selection_and_config_events_arrive(cx: &mut TestAppContext) {
    let (dir, settings) = start(cx);
    let window = window(cx, Some(dir.file("a.txt")));
    open(cx, window, dir.file("b.txt"));
    let b = active_editor(cx, window);
    let b_id = id_of(cx, &b);
    let inbox = load(cx, &dir, FOCUS | SELECTION | CONFIG);
    // Loaded, it hears where the focus is.
    assert_eq!(delivered(cx, &inbox, 1), [Event::FocusChanged(Some(b_id))]);
    inbox.hold(true);

    // Switching tabs moves the focus to `a.txt`, at its caret.
    window
        .update(cx, |workbench, window, cx| {
            let pane = workbench.pane().clone();
            pane.update(cx, |pane, cx| pane.cycle(true, window, cx));
        })
        .unwrap();
    draw(cx, window);
    let a = active_editor(cx, window);
    let a_id = id_of(cx, &a);
    assert_ne!(a_id, b_id);
    assert_eq!(inbox.len(), 2, "the focus and the selection");
    // The caret moving: one selection of the buffer waits, the latest.
    for at in 1..=3 {
        select(cx, window, &a, at..at);
    }
    assert_eq!(inbox.len(), 2);
    settings.reload(&format!("{PREVIEW}\n[editor]\ntab_size = 2\n"), cx);
    assert_eq!(inbox.len(), 3);

    inbox.hold(false);
    let events = delivered(cx, &inbox, 4);
    let selection = SelectionChanged {
        buffer: a_id,
        version: 1,
        ranges: one(3, 3),
    };
    assert_eq!(
        events[1..],
        [
            Event::FocusChanged(Some(a_id)),
            Event::SelectionChanged(selection),
            Event::ConfigReloaded,
        ]
    );
}

#[gpui::test]
fn a_plugin_that_falls_behind_never_slows_typing(cx: &mut TestAppContext) {
    let (dir, _settings) = start(cx);
    // A queue of one, so one event it hasn't taken fills it.
    cx.update(|cx| cx.global_mut::<PluginHost>().limits.event_queue = 1);
    let window = window(cx, Some(dir.file("a.txt")));
    let a = active_editor(cx, window);
    let a_id = id_of(cx, &a);
    let inbox = load(cx, &dir, OPENED | CHANGED);
    assert_eq!(delivered(cx, &inbox, 1).len(), 1, "`a.txt` opened");
    // It takes nothing from here.
    inbox.hold(true);
    open(cx, window, dir.file("b.txt"));
    let b = active_editor(cx, window);
    let b_id = id_of(cx, &b);
    assert_eq!(inbox.len(), 1, "`b.txt` opened");

    let typed = "x".repeat(1000);
    let started = Instant::now();
    cx.simulate_input(window.into(), &typed);
    let took = started.elapsed();
    draw(cx, window);
    assert_eq!(
        b.read_with(cx, |editor, _| editor.model().buffer().text().to_string()),
        format!("{typed}beta\n")
    );
    assert!(took < Duration::from_secs(20), "typing took {took:?}");
    assert_eq!(inbox.len(), 1, "bounded");

    inbox.hold(false);
    let events = delivered(cx, &inbox, 3);
    let [
        Event::BufferOpened(opened),
        Event::Lagged(lagged),
        Event::BufferChanged(changed),
    ] = events.as_slice()
    else {
        panic!("{events:#?}");
    };
    assert_eq!(opened.buffer, a_id);
    // `b.txt`'s open went to make room for its first change, which every
    // later one merged into.
    assert_eq!(*lagged, 1);
    let expected = BufferChanged {
        buffer: b_id,
        version: 1001,
        ranges: one(0, 1000),
    };
    assert_eq!(*changed, expected);
}

#[gpui::test]
fn a_save_names_the_version_it_wrote(cx: &mut TestAppContext) {
    let (dir, _settings) = start(cx);
    let a = dir.file("a.txt");
    let window = window(cx, Some(a.clone()));
    let editor = active_editor(cx, window);
    focus(cx, window, &editor);
    let id = id_of(cx, &editor);
    let inbox = load(cx, &dir, CHANGED | SAVED);

    cx.simulate_input(window.into(), "x");
    // Typed while the save writes what it took: version 2.
    let saving = window
        .update(cx, |_, window, cx| {
            let saving = editor.update(cx, |editor, cx| editor.save(window, cx));
            window.dispatch_keystroke(gpui::Keystroke::parse("y").unwrap(), cx);
            saving
        })
        .unwrap();
    draw(cx, window);
    drop(saving);
    assert_eq!(fs::read_to_string(&a).unwrap(), "xalpha\n");
    assert_eq!(versions(&inbox), [('c', 2), ('c', 3), ('s', 2)]);

    // Edits no editor told of yet go to plugins before the bridge reads
    // the buffer, so each keeps its own version.
    editor.update(cx, |editor, _| {
        let mut buffer = editor.model_mut().buffer_mut();
        buffer.edit([(0..0, "1")], Origin::User);
        buffer.edit([(0..0, "2")], Origin::User);
    });
    let read = cx.update(|cx| snapshot(id, cx)).unwrap();
    assert_eq!(read.version, 5);
    assert_eq!(&versions(&inbox)[3..], [('c', 4), ('c', 5)]);
}

#[gpui::test]
fn focus_leaving_every_buffer_is_sent(cx: &mut TestAppContext) {
    let (dir, _settings) = start(cx);
    let window = window(cx, Some(dir.file("a.txt")));
    let editor = active_editor(cx, window);
    focus(cx, window, &editor);
    let id = id_of(cx, &editor);
    let inbox = load(cx, &dir, FOCUS);
    assert_eq!(delivered(cx, &inbox, 1), [Event::FocusChanged(Some(id))]);

    // Focus elsewhere, then back.
    window.update(cx, |_, window, cx| window.blur(cx)).unwrap();
    draw(cx, window);
    assert_eq!(delivered(cx, &inbox, 2)[1], Event::FocusChanged(None));
    focus(cx, window, &editor);
    assert_eq!(delivered(cx, &inbox, 3)[2], Event::FocusChanged(Some(id)));
    // The window losing focus.
    gpui::VisualTestContext::from_window(window.into(), cx).deactivate_window();
    draw(cx, window);
    assert_eq!(delivered(cx, &inbox, 4)[3], Event::FocusChanged(None));
}

#[gpui::test]
fn closing_a_window_closes_its_buffers(cx: &mut TestAppContext) {
    let (dir, _settings) = start(cx);
    let first = window(cx, Some(dir.file("a.txt")));
    let second = window(cx, Some(dir.file("b.txt")));
    let b = active_editor(cx, second);
    let b_id = id_of(cx, &b);
    let inbox = load(cx, &dir, OPENED | CLOSED);
    assert_eq!(delivered(cx, &inbox, 2).len(), 2, "both opened");

    first
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    draw(cx, first);
    // The inactive one closes, its workbench still held.
    let held = second.update(cx, |_, _, cx| cx.entity()).unwrap();
    second
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(delivered(cx, &inbox, 3)[2], Event::BufferClosed(b_id));
    drop(held);
}

#[gpui::test]
fn a_focused_buffers_changes_go_before_its_selection(cx: &mut TestAppContext) {
    let (dir, _settings) = start(cx);
    let window = window(cx, Some(dir.file("a.txt")));
    let a = active_editor(cx, window);
    open(cx, window, dir.file("b.txt"));
    let b = active_editor(cx, window);
    let b_id = id_of(cx, &b);
    let inbox = load(cx, &dir, CHANGED | SELECTION);

    // Two edits to the focused buffer that no editor told of yet, then
    // another editor's notification.
    b.update(cx, |editor, _| {
        let mut buffer = editor.model_mut().buffer_mut();
        buffer.edit([(0..0, "1")], Origin::User);
        buffer.edit([(0..0, "2")], Origin::User);
    });
    a.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let changes: Vec<(u64, u64)> = (changes(&inbox).iter())
        .map(|change| (change.buffer, change.version))
        .collect();
    assert_eq!(changes, [(b_id, 2), (b_id, 3)]);
}

#[gpui::test]
fn a_save_finishing_256_changes_later_names_its_version(cx: &mut TestAppContext) {
    let (dir, _settings) = start(cx);
    let window = window(cx, Some(dir.file("a.txt")));
    let editor = active_editor(cx, window);
    let id = id_of(cx, &editor);
    let inbox = load(cx, &dir, SAVED);
    let saves = |inbox: &Inbox| {
        (inbox.pushed().into_iter())
            .filter_map(|event| match event {
                HostEvent::BufferSaved(saved) => Some(saved.version),
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    let mut sent = Vec::new();
    for later in [255, 256, 257] {
        let wrote = cx.update(|cx| snapshot(id, cx)).unwrap().version;
        // `later` changes made while the save writes.
        let saving = window
            .update(cx, |_, window, cx| {
                editor.update(cx, |editor, cx| {
                    let saving = editor.save(window, cx);
                    let mut buffer = editor.model_mut().buffer_mut();
                    for _ in 0..later {
                        buffer.edit([(0..0, "x")], Origin::User);
                    }
                    saving
                })
            })
            .unwrap();
        draw(cx, window);
        drop(saving);
        sent.push(wrote);
    }
    // The last named a version evicted: not sent.
    assert_eq!(saves(&inbox), sent[..2]);
}

#[gpui::test]
fn a_keymap_reload_sends_config_reloaded(cx: &mut TestAppContext) {
    let (dir, settings) = start(cx);
    let _window = window(cx, Some(dir.file("a.txt")));
    let inbox = load(cx, &dir, CONFIG);

    // Only the keymap's text changes: the settings stay as they were.
    settings.reload_keymap("# Nothing bound.\n", cx);
    assert_eq!(delivered(cx, &inbox, 1), [Event::ConfigReloaded]);
}

/// Extensions a workspace's watcher applies, which publishes no batch of
/// changed files, still tell a plugin the config reloaded.
#[gpui::test]
fn extensions_applied_without_changed_files_tell_of_a_reload(cx: &mut TestAppContext) {
    let (dir, _settings) = start(cx);
    let inbox = load(cx, &dir, CONFIG);
    inbox.hold(true);
    let api = ExtensionApi {
        app: fe_config::paths::app_id().short,
        version: ApiVersion { major: 1, minor: 1 },
        reserved: Vec::new(),
        kinds: vec![crate::plugin_host::plugin_kind()],
        allowed_licenses: None,
        only_kinds: None,
    };
    let set = ExtensionSet::discover(&dir.path().join("extensions"), &api);
    // As the watcher's callback hands them over.
    cx.update(|cx| set_plugin_extensions(cx, &set));
    cx.run_until_parked();
    assert_eq!(inbox.pushed(), [HostEvent::ConfigReloaded]);
    let running = cx.update(|cx| {
        (plugin_states(cx).iter())
            .any(|info| info.id == LISTENER && info.state == PluginState::Running)
    });
    assert!(running, "the same extensions keep it running");
}
