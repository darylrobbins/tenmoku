//! A load that fails after its plugin's `init` drew something takes what
//! it drew with it: status items and decorations set before `init`
//! trapped are gone from the bar and the editors, and stay gone once the
//! plugin is unloaded.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::capability::Capability;
use fe_editor::decorations::{DecorationKind, Tint};
use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use gpui::{AppContext as _, Entity, Focusable as _, TestAppContext, WindowHandle};
use kiln::core::{
    ApiVersion,
    extension::{ExtensionApi, load_manifest},
};
use kiln_plugin::{
    grants::GrantStore,
    manifest::{ExtensionOrigin, PluginManifest},
};

use super::{
    Artifact, Found, GRANTS_FILE, LoadContext, LoadFailure, PluginHost, PluginState, StopReason,
    plugin_kind, plugin_states, record, set_plugin_dirs, set_plugins, set_preview, ticker,
    unload_plugin,
};
use crate::{Launch, Workbench, plugin_paint};

/// A plugin whose `init` sets the status item `x`, with `decorate` a
/// highlight over the first byte of buffer 1 at version 1 too, then traps.
fn publisher(decorate: bool) -> String {
    let decorate = if decorate {
        "(call $set-decorations (i64.const 1) (i64.const 1) (i32.const 256) (i32.const 1) \
         (i32.const 512) (i32.const 1) (i32.const 64))"
    } else {
        ""
    };
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

  (import "fastedit:editor/decorations@1.0.0" (instance $decorations
    (alias outer $C $range (type $range))
    (alias outer $C $error (type $error))
    (type $style (enum "highlight" "underline" "squiggle" "strikethrough" "faded"))
    (export "decoration-style" (type $style' (eq $style)))
    (type $decoration (record
      (field "range" $range)
      (field "style" $style')
      (field "color" (option string))
      (field "hover" (option string))))
    (export "decoration" (type $decoration' (eq $decoration)))
    (export "set-decorations" (func
      (param "buffer" u64) (param "version" u64) (param "set" string)
      (param "decorations" (list $decoration'))
      (result (result (error $error)))))))

  (import "fastedit:editor/status@1.0.0" (instance $status
    (alias outer $C $error (type $error))
    (type $alignment (enum "left" "right"))
    (export "alignment" (type $alignment' (eq $alignment)))
    (type $item (record
      (field "id" string)
      (field "text" string)
      (field "tooltip" (option string))
      (field "command" (option string))
      (field "alignment" $alignment')))
    (export "item" (type $item' (eq $item)))
    (export "set-item" (func (param "item" $item') (result (result (error $error)))))))

  (core module $libc
    (memory (export "memory") 1)
    ;; 64.. holds the imports' results; the heap starts at 1024.
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

  (alias export $status "set-item" (func $status-set-item))
  (core func $set-item
    (canon lower (func $status-set-item) (memory $memory) (realloc $realloc)))
  (alias export $decorations "set-decorations" (func $decorations-set))
  (core func $set-decorations
    (canon lower (func $decorations-set) (memory $memory) (realloc $realloc)))
  (core instance $host
    (export "set-item" (func $set-item))
    (export "set-decorations" (func $set-decorations)))

  (core module $main
    (import "libc" "memory" (memory 1))
    (import "host" "set-item"
      (func $set-item (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
    (import "host" "set-decorations"
      (func $set-decorations (param i64 i64 i32 i32 i32 i32 i32)))
    ;; The set's name, the item's id and its text.
    (data (i32.const 256) "s")
    (data (i32.const 260) "x")
    (data (i32.const 264) "X")
    ;; One decoration at 512: 0..1, a highlight, with no colour or hover.
    (data (i32.const 520) "\01")
    (func (export "init") (result i32)
      ;; Item `x`, text `X`, no tooltip or command, on the left.
      (call $set-item
        (i32.const 260) (i32.const 1) (i32.const 264) (i32.const 1)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 64))
      {decorate}
      unreachable)
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

const GRANTED: [&str; 2] = ["decorations", "status"];

fn grants() -> fe_command::Grants {
    (GRANTED.iter())
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

/// Plugin `id`, its component `wat`, granted [`GRANTED`], in `dir`.
fn install(dir: &Path, id: &str, wat: &str) -> Found {
    let extension = dir.join("extensions").join(id);
    fs::create_dir_all(&extension).unwrap();
    fs::write(extension.join("plugin.wasm"), wat).unwrap();
    fs::write(
        extension.join("extension.toml"),
        format!(
            "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\ntenmoku = \"1.1\"\n\n\
             [contributes.plugin]\nwasm = \"plugin.wasm\"\n\
             capabilities = [\"decorations\", \"status\"]\n"
        ),
    )
    .unwrap();
    GrantStore::new(dir.join("state").join(GRANTS_FILE))
        .grant(id, (1, 0, 0), &grants(), &[], false)
        .unwrap();
    let api = ExtensionApi {
        app: fe_config::paths::app_id().short,
        version: ApiVersion { major: 1, minor: 1 },
        reserved: Vec::new(),
        kinds: vec![plugin_kind()],
        allowed_licenses: None,
        only_kinds: None,
    };
    let (manifest, diagnostics) = load_manifest(&extension, &api);
    let manifest = manifest.unwrap_or_else(|| panic!("{diagnostics:#?}"));
    let plugin = PluginManifest::from_extension(&manifest, ExtensionOrigin::User)
        .unwrap()
        .unwrap();
    Found::new(&manifest, Ok(plugin))
}

fn draw(cx: &mut TestAppContext, window: WindowHandle<Workbench>) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
    }
}

/// Runs until `done`, failing after a while a loaded machine meets.
fn until(cx: &mut TestAppContext, what: &str, mut done: impl FnMut(&mut TestAppContext) -> bool) {
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

/// The plugins' items on `window`'s bar, by id.
fn bar(cx: &mut TestAppContext, window: WindowHandle<Workbench>) -> Vec<String> {
    draw(cx, window);
    window
        .update(cx, |workbench, _, cx| {
            (workbench.status_bar().read(cx).items(cx).into_iter())
                .map(|item| item.id.to_string())
                .filter(|id| id.starts_with("plugin:"))
                .collect()
        })
        .unwrap()
}

/// The plugin highlights `editor` paints.
fn highlights(cx: &mut TestAppContext, editor: &Entity<EditorView>) -> usize {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        (model.decorations(0..model.buffer().len()).into_iter())
            .filter(|decoration| {
                decoration.kind == DecorationKind::Highlight && decoration.tint == Tint::Accent
            })
            .count()
    })
}

#[gpui::test]
fn what_a_failed_init_drew_goes_with_it(cx: &mut TestAppContext) {
    const STATUS: &str = "acme.status";
    const DECORATOR: &str = "acme.decorator";
    // Plugins run on threads of their own.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let a: PathBuf = dir.path().join("a.txt");
    fs::write(&a, "alpha\n").unwrap();
    let (state, cache) = (dir.path().join("state"), dir.path().join("cache"));
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_plugin_dirs(cx, state, cache);
        set_preview(cx, true);
        cx.global_mut::<PluginHost>().limits.call_deadline = Duration::from_secs(30);
    });
    let launch = Launch {
        root: None,
        file: Some((a, Some(Buffer::new("alpha\n").into()))),
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
    let editor = window
        .update(cx, |workbench, _, cx| workbench.active_editor(cx).cloned())
        .unwrap()
        .unwrap();

    // Loaded by the host, its `init` sets an item and traps: it stops, and
    // the item goes, unloaded or not.
    let status = install(dir.path(), STATUS, &publisher(false));
    cx.update(|cx| set_plugins(cx, vec![status]));
    until(cx, "the plugin to stop", |cx| {
        let states = cx.update(|cx| plugin_states(cx));
        (states.iter()).any(|info| {
            info.id == STATUS
                && matches!(&info.state, PluginState::Stopped(why) if why.reason == StopReason::Load)
        })
    });
    assert_eq!(bar(cx, window), Vec::<String>::new());
    cx.update(|cx| unload_plugin(STATUS, cx));
    assert_eq!(bar(cx, window), Vec::<String>::new());

    // Its decorations go too. The load runs on a thread of the test's, so
    // the main thread takes the buffers' jobs `init` hands it meanwhile.
    let decorator = install(dir.path(), DECORATOR, &publisher(true));
    let context = cx.update(|cx| {
        let paint = plugin_paint::paint(cx);
        let host = cx.global::<PluginHost>();
        LoadContext {
            cache: None,
            commands: host.commands.clone().unwrap(),
            bridge: host.bridge.clone().unwrap(),
            limits: host.limits,
            secrets: Arc::new(kiln::core::secrets::MemoryStore::new()),
            secrets_index: None,
            store: None,
            workspace: kiln_plugin::fs::Workspace::default(),
            record: record(host),
            paint,
            ticker,
        }
    });
    let artifact = Artifact {
        manifest: decorator.plugin.clone().unwrap(),
        bytes: publisher(true).into_bytes(),
        record: Default::default(),
    };
    let loading =
        std::thread::spawn(
            move || match context.load(&artifact, &grants(), &Arc::default()) {
                Err(LoadFailure::Failed(message)) => message,
                Err(LoadFailure::Cancelled) => panic!("not cancelled"),
                Ok(_) => panic!("loaded though its init trapped"),
            },
        );
    until(cx, "the load to fail", |_| loading.is_finished());
    let message = loading.join().unwrap();
    assert!(message.contains("unreachable"), "{message}");
    draw(cx, window);
    assert_eq!(highlights(cx, &editor), 0);
    assert_eq!(bar(cx, window), Vec::<String>::new());
}
