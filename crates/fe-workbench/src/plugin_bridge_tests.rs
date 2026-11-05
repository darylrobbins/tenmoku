//! The bridge from plugins to the editor: buffer ids across splits and
//! windows, the editor guest reading, editing and proposing through it, and
//! a plugin invoking the editor's commands. The plugins run on their own
//! threads; each test calls into one from a thread of its own while the
//! main thread takes the plugin's jobs.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, Origin};
use fe_command::{
    Actor, ContextFrame, ContextStack, Invocation, Source,
    capability::{Capability, Grants},
};
use fe_editor::EditorModel;
use fe_gpui::{DiffView, EditorSettings, EditorTheme, EditorView};
use gpui::{AppContext as _, Entity, Focusable as _, TestAppContext, WindowHandle};
use kiln::core::{
    ApiVersion,
    extension::{ExtensionApi, load_manifest},
};
use kiln_plugin::{
    bindings::Error,
    grants::GrantStore,
    manifest::{ExtensionOrigin, PluginManifest},
    runner::{Call, Reply},
};

use super::{Handoff, buffer_id, snapshot};
use crate::{
    Launch, Workbench,
    plugin_host::{
        Found, GRANTS_FILE, PluginHost, PluginState, plugin_handle, plugin_states, set_plugin_dirs,
        set_plugins, set_preview,
    },
    tests::find_bar::{draw as draw_in, editor, opened, pane, run},
};

/// fe-plugin's editor guest: `read`, `edit` and `propose` call the buffer
/// imports and log what came back.
const EDITOR_GUEST: &[u8] = include_bytes!("../../fe-plugin/tests/guests/editor/editor.wasm");

/// A plugin whose `run-command` invokes the command it's given, with the
/// args it's given, and returns what that returned.
const INVOKER: &str = r#"
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

  (import "kiln:extension/commands@1.0.0" (instance $commands
    (alias outer $C $error (type $error))
    (export "invoke" (func (param "id" string) (param "args" string) (result (result (error $error)))))))

  (import "kiln:extension/events@1.0.0" (instance $events
    (alias outer $C $range (type $range))
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
    (export "event" (type (eq $event)))))
  (alias export $events "event" (type $event))

  (core module $libc
    (memory (export "memory") 1)
    ;; Bytes 0..8 stay zero: a `result` that is `ok`. 64.. holds the import's result; the heap
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

  (alias export $commands "invoke" (func $commands-invoke))
  (core func $invoke (canon lower (func $commands-invoke) (memory $memory) (realloc $realloc)))
  (core instance $host (export "invoke" (func $invoke)))

  (core module $main
    (import "libc" "memory" (memory 1))
    (import "host" "invoke" (func $invoke (param i32 i32 i32 i32 i32)))
    (func (export "init") (result i32) (i32.const 0))
    (func (export "shutdown"))
    (func (export "run-command") (param $id i32) (param $id-len i32) (param $args i32) (param $args-len i32) (result i32)
      (call $invoke (local.get $id) (local.get $id-len) (local.get $args) (local.get $args-len) (i32.const 64))
      (i32.const 64))
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
"#;

/// The editor guest's id.
const EDITOR: &str = "acme.editor";

/// A test's folder, with the plugins' state, cache and extensions in it.
struct Dir(tempfile::TempDir);

impl Dir {
    fn path(&self) -> &Path {
        self.0.path()
    }

    /// Plugin `id`, its component `bytes`, asking for and granted
    /// `capabilities`.
    fn install(&self, id: &str, bytes: &[u8], capabilities: &[&str]) -> Found {
        let dir = self.path().join("extensions").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plugin.wasm"), bytes).unwrap();
        let quoted: Vec<String> = (capabilities.iter())
            .map(|capability| format!("{capability:?}"))
            .collect();
        fs::write(
            dir.join("extension.toml"),
            format!(
                "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
                 [engine]\ntenmoku = \"1.1\"\n\n\
                 [contributes.plugin]\nwasm = \"plugin.wasm\"\ncapabilities = [{}]\n",
                quoted.join(", ")
            ),
        )
        .unwrap();
        let grants: Grants = (capabilities.iter())
            .map(|capability| Capability::parse(capability).unwrap())
            .collect();
        GrantStore::new(self.path().join("state").join(GRANTS_FILE))
            .grant(id, (1, 0, 0), &grants, &[], false)
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

/// The workbench's commands, a folder with `a.txt` holding `text`, and,
/// with `plugins`, the plugin host keeping its files there, with the
/// preview on and a call deadline a loaded machine meets.
fn start(cx: &mut TestAppContext, text: &str, plugins: bool) -> Dir {
    // Plugins run on threads of their own.
    cx.executor().allow_parking();
    let dir = Dir(tempfile::tempdir().unwrap());
    fs::write(dir.path().join("a.txt"), text).unwrap();
    let (state, cache) = (dir.path().join("state"), dir.path().join("cache"));
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        if plugins {
            set_plugin_dirs(cx, state, cache);
            set_preview(cx, true);
            cx.global_mut::<PluginHost>().limits.call_deadline = Duration::from_secs(30);
        }
    });
    dir
}

/// A workbench showing `file` with `text` if any, in a window of its own,
/// active and drawn.
fn window(cx: &mut TestAppContext, file: Option<(PathBuf, &str)>) -> WindowHandle<Workbench> {
    let launch = Launch {
        root: None,
        file: file.map(|(path, text)| (path, Some(Buffer::new(text).into()))),
    };
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

fn editors(cx: &mut TestAppContext, window: WindowHandle<Workbench>) -> Vec<Entity<EditorView>> {
    window
        .update(cx, |workbench, _, cx| workbench.open_editors(cx))
        .unwrap()
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

fn active_id(cx: &mut TestAppContext, window: WindowHandle<Workbench>) -> u64 {
    let editor = active_editor(cx, window);
    id_of(cx, &editor)
}

fn text(cx: &mut TestAppContext, editor: &Entity<EditorView>) -> String {
    editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string())
}

/// Hands the host `found`, plugins `ids`, and runs until each of them runs.
fn load(cx: &mut TestAppContext, ids: &[&str], found: Vec<Found>) {
    cx.update(|cx| set_plugins(cx, found));
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        cx.run_until_parked();
        let running = cx.update(|cx| {
            let states = plugin_states(cx);
            (ids.iter()).all(|id| {
                (states.iter()).any(|info| info.id == *id && info.state == PluginState::Running)
            })
        });
        if running {
            return;
        }
        assert!(Instant::now() < deadline, "{ids:?} didn't load");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs plugin `plugin`'s `command` with `args` on a thread of the test's,
/// the main thread taking the plugin's jobs meanwhile: what it returned.
fn call(cx: &mut TestAppContext, plugin: &str, command: &str, args: &str) -> Result<(), Error> {
    let handle = cx.update(|cx| plugin_handle(plugin, cx)).unwrap();
    let call = Call::RunCommand {
        id: command.to_owned(),
        args: args.to_owned(),
    };
    let thread = std::thread::spawn(move || handle.lock().unwrap().call(call));
    let deadline = Instant::now() + Duration::from_secs(30);
    while !thread.is_finished() {
        assert!(Instant::now() < deadline, "{command} didn't return");
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(1));
    }
    match thread.join().unwrap() {
        Ok(Reply::RunCommand(result)) => result,
        other => panic!("{command}: {other:?}"),
    }
}

/// What plugin `plugin` logged since `since` lines of the host's record.
fn logged(cx: &mut TestAppContext, plugin: &str, since: usize) -> Vec<String> {
    let prefix = format!("log {plugin} on plugin {plugin}: ");
    cx.update(|cx| {
        let record = cx.global::<PluginHost>().record.lock().unwrap();
        (record.iter().skip(since))
            .filter_map(|line| line.strip_prefix(&prefix).map(str::to_owned))
            .collect()
    })
}

fn record_len(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| cx.global::<PluginHost>().record.lock().unwrap().len())
}

/// The editor guest's read of `args`: its version and text.
fn read(cx: &mut TestAppContext, args: &str) -> Result<(u64, String), Error> {
    let since = record_len(cx);
    call(cx, EDITOR, "read", args)?;
    let logged = logged(cx, EDITOR, since).concat();
    let (version, text) = logged.split_once(' ').unwrap();
    Ok((version.parse().unwrap(), text.to_owned()))
}

#[gpui::test]
fn buffer_ids_follow_open_buffers(cx: &mut TestAppContext) {
    let dir = start(cx, "alpha\n", false);
    let a = dir.path().join("a.txt");
    let first = window(cx, Some((a.clone(), "alpha\n")));
    let shown = active_editor(cx, first);
    // A split in the window, and a model of the buffer in another window.
    first
        .update(cx, |_, window, cx| {
            let invocation = Invocation::of("pane.split_right")
                .unwrap()
                .context(ContextStack::new().with(ContextFrame::new("workbench")));
            window.dispatch_action(Box::new(fe_gpui::commands::RunCommand(invocation)), cx);
        })
        .unwrap();
    draw(cx, first);
    assert_eq!(editors(cx, first).len(), 2);
    let second = window(cx, None);
    let split = shown.read_with(cx, |editor, _| editor.model().split());
    let pane = second
        .update(cx, |workbench, _, cx| {
            workbench.pane_group(cx).panes()[0].clone()
        })
        .unwrap();
    pane.update(cx, |pane, cx| {
        pane.add(split, cx);
        pane.add(EditorModel::new(Buffer::new("beta"), None), cx);
    });
    draw(cx, second);

    let id = id_of(cx, &shown);
    let mut showing = editors(cx, first);
    showing.extend(editors(cx, second));
    let ids: Vec<u64> = showing.iter().map(|editor| id_of(cx, editor)).collect();
    let beta = active_id(cx, second);
    assert_ne!(beta, id);
    assert_eq!(ids, [id, id, id, beta]);
    let read = cx.update(|cx| snapshot(id, cx)).unwrap();
    assert_eq!(read.snapshot.text().to_string(), "alpha\n");
    assert_eq!(read.path.as_deref(), Some(a.to_str().unwrap()));

    // Closed everywhere, the id reads nothing, and the file opened again
    // has another.
    cx.update_window(first.into(), |_, window, _| window.remove_window())
        .unwrap();
    second
        .update(cx, |_, window, cx| {
            pane.update(cx, |pane, cx| {
                pane.activate(0, window, cx);
                pane.close_active(window, cx);
            })
        })
        .unwrap();
    draw(cx, second);
    assert_eq!(editors(cx, second).len(), 1);
    assert!(cx.update(|cx| snapshot(id, cx)).is_none());
    assert!(cx.update(|cx| snapshot(beta, cx)).is_some());
    let again = window(cx, Some((a, "alpha\n")));
    let reopened = active_id(cx, again);
    assert!(reopened != id && reopened != beta, "{reopened}");
    assert_eq!(active_id(cx, second), beta);
}

#[gpui::test]
fn a_granted_plugin_reads_edits_and_proposes(cx: &mut TestAppContext) {
    let dir = start(cx, "alpha beta\n", true);
    let shown = window(cx, Some((dir.path().join("a.txt"), "alpha beta\n")));
    let found = dir.install(EDITOR, EDITOR_GUEST, &["buffer.read", "buffer.write"]);
    load(cx, &[EDITOR], vec![found]);
    let editor = active_editor(cx, shown);
    let id = id_of(cx, &editor);

    let (version, first) = read(cx, &id.to_string()).unwrap();
    assert_eq!(first, "alpha beta\n");
    let missing = read_missing(cx, id);
    assert!(matches!(missing, Err(Error::NotFound(_))), "{missing:?}");

    // An edit is one undo step of the plugin's origin.
    let since = record_len(cx);
    call(cx, EDITOR, "edit", &format!("{id} {version}\n0 5 ALPHA")).unwrap();
    let after: u64 = logged(cx, EDITOR, since).concat().parse().unwrap();
    assert!(after > version, "{after} after {version}");
    assert_eq!(text(cx, &editor), "ALPHA beta\n");
    assert_eq!(read(cx, &id.to_string()).unwrap().0, after);

    // At a version it has moved past, nothing applies.
    let stale = call(cx, EDITOR, "edit", &format!("{id} {version}\n6 10 BETA"));
    assert!(matches!(stale, Err(Error::Conflict(_))), "{stale:?}");
    assert_eq!(text(cx, &editor), "ALPHA beta\n");

    let host = Origin::Host(EDITOR.into());
    let undone = editor.update(cx, |editor, _| {
        let mut buffer = editor.model_mut().buffer_mut();
        let first = buffer.undo(|origin| *origin == host).unwrap();
        (
            first.is_some(),
            buffer.undo(|origin| *origin == host).unwrap(),
        )
    });
    assert_eq!(undone, (true, None));
    assert_eq!(text(cx, &editor), "alpha beta\n");

    // A proposal waits in the editor for the user.
    let (version, _) = read(cx, &id.to_string()).unwrap();
    call(
        cx,
        EDITOR,
        "propose",
        &format!("{id} {version} Shout\n0 5 ALPHA"),
    )
    .unwrap();
    let proposals = editor.read_with(cx, |editor, _| editor.proposals().to_vec());
    assert_eq!(proposals.len(), 1);
    assert_eq!(*proposals[0].borrow().origin(), host);
    assert_eq!(text(cx, &editor), "alpha beta\n");
}

/// The editor guest's read of a buffer that isn't open.
fn read_missing(cx: &mut TestAppContext, open: u64) -> Result<(u64, String), Error> {
    read(cx, &(open + 1000).to_string())
}

#[gpui::test]
fn a_plugin_invokes_an_allowlisted_editor_command(cx: &mut TestAppContext) {
    const WRITER: &str = "acme.writer";
    const MOVER: &str = "acme.mover";
    let dir = start(cx, "alpha\n", true);
    let shown = window(cx, Some((dir.path().join("a.txt"), "alpha\n")));
    let writer = dir.install(WRITER, INVOKER.as_bytes(), &["commands", "buffer.write"]);
    let mover = dir.install(MOVER, INVOKER.as_bytes(), &["commands"]);
    load(cx, &[WRITER, MOVER], vec![writer, mover]);
    let editor = active_editor(cx, shown);
    editor.update(cx, |editor, cx| editor.select_and_reveal(5..5, cx));
    let seen: Rc<RefCell<Vec<Invocation>>> = Rc::default();
    let observer = seen.clone();
    cx.update(|cx| {
        fe_gpui::commands::observe_invocations(cx, move |invocation| {
            observer.borrow_mut().push(invocation.clone());
        })
    });

    call(cx, WRITER, "editor.insert", r#"{"text":"!"}"#).unwrap();
    assert_eq!(text(cx, &editor), "alpha!\n");
    let inserted: Vec<_> = (seen.borrow().iter())
        .filter(|invocation| invocation.command.as_str() == "editor.insert")
        .map(|invocation| (invocation.actor.clone(), invocation.source))
        .collect();
    assert_eq!(inserted, [(Actor::Host(WRITER.into()), Source::Script)]);

    let denied = call(cx, MOVER, "editor.insert", r#"{"text":"?"}"#);
    assert!(
        matches!(&denied, Err(Error::Denied(why)) if why.contains("buffer.write")),
        "{denied:?}"
    );
    let saved = call(cx, WRITER, "editor.save", "");
    assert!(matches!(saved, Err(Error::Denied(_))), "{saved:?}");
    assert_eq!(text(cx, &editor), "alpha!\n");
}

#[gpui::test]
fn a_buffer_a_diff_view_shows_keeps_its_id(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "alpha\n")]);
    let tab = editor(&pane(&workbench, cx), cx);
    let id = TestAppContext::update(cx, |cx| buffer_id(&tab, cx)).unwrap();
    run("diff.compare_saved", cx);
    let right = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let diff = pane
            .active_item()
            .and_then(|item| item.downcast_ref::<DiffView>().cloned());
        diff.unwrap().read(cx).right().clone()
    });

    // The tab closed, the diff's working side still shows the buffer.
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| {
            pane.activate(0, window, cx);
            pane.close_active(window, cx);
        });
    });
    draw_in(cx);
    assert_eq!(
        pane(&workbench, cx).read_with(cx, |pane, _| pane.tabs().len()),
        1
    );
    assert_eq!(
        TestAppContext::update(cx, |cx| buffer_id(&right, cx)),
        Some(id)
    );
    let read = TestAppContext::update(cx, |cx| snapshot(id, cx)).unwrap();
    assert_eq!(read.snapshot.text().to_string(), "alpha\n");
}

#[test]
fn a_hand_off_past_its_deadline_never_starts() {
    // The plugin waits until the deadline, then the job won't start.
    let handoff = Handoff::<u32>::new(Instant::now() + Duration::from_millis(20));
    let waited = Instant::now();
    assert!(matches!(handoff.wait(), Err(Error::Failed(_))));
    assert!(waited.elapsed() >= Duration::from_millis(20));
    assert!(!handoff.start());

    // Nor once the deadline has passed while the plugin's thread hasn't
    // looked yet.
    let handoff = Handoff::<u32>::new(Instant::now());
    assert!(!handoff.start());
    assert!(matches!(handoff.wait(), Err(Error::Failed(_))));
}

#[test]
fn a_hand_off_running_past_its_deadline_stops_the_wait() {
    let handoff = Handoff::new(Instant::now() + Duration::from_millis(50));
    assert!(handoff.start());
    let waited = Instant::now();
    let wait = handoff.wait();
    assert!(
        matches!(&wait, Err(Error::Failed(why)) if why.contains("finish")),
        "{wait:?}"
    );
    assert!(waited.elapsed() < Duration::from_secs(5));
    // Finishing late hands nobody anything.
    handoff.finish(1);

    // One finished in time is the plugin's.
    let handoff = Handoff::new(Instant::now() + Duration::from_secs(30));
    assert!(handoff.start());
    let theirs = handoff.clone();
    let main = std::thread::spawn(move || theirs.finish(7));
    assert_eq!(handoff.wait().unwrap(), 7);
    main.join().unwrap();
}
