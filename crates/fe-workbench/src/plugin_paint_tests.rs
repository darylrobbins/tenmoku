//! What plugins draw, read where the editor and the bar read it: a set of
//! decorations in every editor of its buffer, mapped through the edits
//! since its version, and status items on every window's bar, a click
//! running the item's command as its plugin. The plugins run as loaded;
//! the tests call their `decorations` and `status` imports as a plugin's
//! store does, on a slice made as the host makes one, and ask for the
//! repaint its thread asks for once a call returns.

// Decorations are compared as ranges, often one.
#![allow(clippy::single_range_in_vec_init)]

use std::{
    cell::RefCell,
    fs,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{Actor, ContextFrame, ContextStack, Invocation, Source, capability::Capability};
use fe_editor::decorations::{DecorationKind, Tint};
use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use fe_plugin::{
    HostState,
    fastedit::editor::{
        decorations::{Decoration, DecorationStyle, Host as _, Range as WitRange},
        status::{Alignment, Host as _, Item},
    },
};
use gpui::{AppContext as _, Entity, Focusable as _, TestAppContext, WindowHandle};
use kiln::core::{
    ApiVersion,
    extension::{ExtensionApi, load_manifest},
};
use kiln_plugin::{
    bindings::Error,
    grants::GrantStore,
    manifest::{ExtensionOrigin, PluginManifest},
};
use kiln_shell::StatusSide;

use super::{Paint, paint};
use crate::{
    Launch, Workbench,
    plugin_bridge::buffer_id,
    plugin_host::{
        Found, GRANTS_FILE, PluginHost, PluginState, bridge_buffers, plugin_states,
        set_plugin_dirs, set_plugins, set_preview, unload_plugin,
    },
};

/// Registers `acme.tool.hello`, which logs its id and args.
const COMMANDS: &str = include_str!("../../../kiln/crates/kiln-plugin/tests/fixtures/commands.wat");

/// Does nothing.
const EMPTY: &str = include_str!("../../../kiln/crates/kiln-plugin/tests/fixtures/empty.wat");

const TOOL: &str = "acme.tool";
const BARE: &str = "acme.bare";

/// A test's folder: `a.txt`, and the plugins' state, cache and extensions.
struct Dir(tempfile::TempDir);

impl Dir {
    fn file(&self, name: &str) -> PathBuf {
        self.0.path().join(name)
    }

    /// Plugin `id`, its component `wat`, granted the `capabilities` it
    /// asks for.
    fn install(&self, id: &str, wat: &str, capabilities: &[&str]) -> Found {
        let dir = self.file("extensions").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plugin.wasm"), wat).unwrap();
        let asked: Vec<String> = (capabilities.iter())
            .map(|capability| format!("{capability:?}"))
            .collect();
        fs::write(
            dir.join("extension.toml"),
            format!(
                "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
                 [engine]\ntenmoku = \"1.1\"\n\n\
                 [contributes.plugin]\nwasm = \"plugin.wasm\"\ncapabilities = [{}]\n",
                asked.join(", ")
            ),
        )
        .unwrap();
        GrantStore::new(self.file("state").join(GRANTS_FILE))
            .grant(id, (1, 0, 0), &grants(capabilities), &[], false)
            .unwrap();
        found(&dir)
    }
}

fn grants(capabilities: &[&str]) -> fe_command::Grants {
    (capabilities.iter())
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

fn found(dir: &Path) -> Found {
    let api = ExtensionApi {
        app: fe_config::paths::app_id().short,
        version: ApiVersion { major: 1, minor: 1 },
        reserved: Vec::new(),
        kinds: vec![crate::plugin_host::plugin_kind()],
        allowed_licenses: None,
        only_kinds: None,
    };
    let (manifest, diagnostics) = load_manifest(dir, &api);
    let manifest = manifest.unwrap_or_else(|| panic!("{diagnostics:#?}"));
    let plugin = PluginManifest::from_extension(&manifest, ExtensionOrigin::User)
        .unwrap()
        .unwrap();
    Found::new(&manifest, Ok(plugin))
}

/// The workbench's commands, a folder with `a.txt` holding `text`, and the
/// plugin host keeping its files there, with the preview on and a call
/// deadline a loaded machine meets.
fn start(cx: &mut TestAppContext, text: &str) -> Dir {
    // Plugins run on threads of their own.
    cx.executor().allow_parking();
    let dir = Dir(tempfile::tempdir().unwrap());
    fs::write(dir.file("a.txt"), text).unwrap();
    let (state, cache) = (dir.file("state"), dir.file("cache"));
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_plugin_dirs(cx, state, cache);
        set_preview(cx, true);
        cx.global_mut::<PluginHost>().limits.call_deadline = Duration::from_secs(30);
    });
    dir
}

/// A workbench showing `file` if any, in a window of its own, active and
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

fn editors(cx: &mut TestAppContext, window: WindowHandle<Workbench>) -> Vec<Entity<EditorView>> {
    window
        .update(cx, |workbench, _, cx| workbench.open_editors(cx))
        .unwrap()
}

/// Hands the host `found`, plugins `ids`, and runs until each runs.
fn load(cx: &mut TestAppContext, ids: &[&str], found: Vec<Found>) {
    cx.update(|cx| set_plugins(cx, found));
    until(cx, "the plugins to load", |cx| {
        let states = cx.update(|cx| plugin_states(cx));
        (ids.iter()).all(|id| {
            (states.iter()).any(|info| info.id == *id && info.state == PluginState::Running)
        })
    });
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

/// Runs `call` on `state` on a thread of the test's, as the plugin's
/// thread would, the main thread taking the bridge's jobs meanwhile; then
/// asks for the repaint the plugin's thread asks for.
fn on_thread<R: Send + 'static>(
    cx: &mut TestAppContext,
    paint: &Paint,
    state: HostState,
    call: impl FnOnce(&mut HostState) -> R + Send + 'static,
) -> (HostState, R) {
    let thread = std::thread::spawn(move || {
        let mut state = state;
        let returned = call(&mut state);
        (state, returned)
    });
    until(cx, "the call to return", |_| thread.is_finished());
    paint.repaint();
    cx.run_until_parked();
    thread.join().unwrap()
}

fn highlight(range: Range<u64>) -> Decoration {
    Decoration {
        range: WitRange {
            start: range.start,
            end: range.end,
        },
        style: DecorationStyle::Highlight,
        color: None,
        hover: None,
    }
}

/// The plugin highlights `editor` paints, as of its last frame.
fn highlights(cx: &mut TestAppContext, editor: &Entity<EditorView>) -> Vec<Range<usize>> {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        (model.decorations(0..model.buffer().len()).into_iter())
            .filter(|decoration| {
                decoration.kind == DecorationKind::Highlight && decoration.tint == Tint::Accent
            })
            .map(|decoration| decoration.range)
            .collect()
    })
}

#[gpui::test]
fn pushed_decorations_reach_the_editor(cx: &mut TestAppContext) {
    const PAINTER: &str = "acme.painter";
    let dir = start(cx, "one two three\n");
    let a = dir.file("a.txt");
    let first = window(cx, Some(a.clone()));
    let painter = dir.install(PAINTER, EMPTY, &["decorations"]);
    load(cx, &[PAINTER], vec![painter]);
    let shown = editors(cx, first)[0].clone();
    // Another window's editor of the buffer, and a split, which carries
    // the provider of the editor it was made from.
    let second = window(cx, None);
    let model = shown.read_with(cx, |editor, _| editor.model().split());
    second
        .update(cx, |workbench, _, cx| {
            let pane = workbench.pane_group(cx).panes()[0].clone();
            pane.update(cx, |pane, cx| pane.add(model, cx));
        })
        .unwrap();
    draw(cx, second);
    first
        .update(cx, |_, window, cx| {
            let invocation = Invocation::of("pane.split_right")
                .unwrap()
                .context(ContextStack::new().with(ContextFrame::new("workbench")));
            window.dispatch_action(Box::new(fe_gpui::commands::RunCommand(invocation)), cx);
        })
        .unwrap();
    draw(cx, first);
    let mut all = editors(cx, first);
    all.extend(editors(cx, second));
    assert_eq!(all.len(), 3);
    let id = cx.update(|cx| buffer_id(&shown, cx)).unwrap();

    // The plugin was shown version 1; the user types before its set lands.
    let paint = cx.update(paint);
    let buffers = cx.update(|cx| bridge_buffers(cx)).unwrap();
    let decorations = paint.decorations(PAINTER, &grants(&["decorations"]), buffers);
    let state = HostState {
        decorations,
        ..HostState::default()
    };
    shown.update(cx, |editor, cx| {
        editor.model_mut().set_cursor(0);
        editor.model_mut().insert("zero ");
        cx.notify();
    });
    draw(cx, first);
    draw(cx, second);
    let (state, set) = on_thread(cx, &paint, state, move |state| {
        state.set_decorations(id, 1, "lint".into(), vec![highlight(4..7)])
    });
    set.unwrap();
    draw(cx, first);
    draw(cx, second);
    // "two", moved on by "zero ", in every editor, once.
    for editor in &all {
        assert_eq!(highlights(cx, editor), [9..12]);
    }
    // A version it was never shown can't be mapped.
    let (state, stale) = on_thread(cx, &paint, state, move |state| {
        state.set_decorations(id, 99, "lint".into(), vec![highlight(0..1)])
    });
    assert!(matches!(stale, Err(Error::Conflict(_))), "{stale:?}");

    // Cleared, it's gone.
    let (state, cleared) = on_thread(cx, &paint, state, move |state| {
        state.clear(id, "lint".into())
    });
    cleared.unwrap();
    draw(cx, first);
    for editor in &all {
        assert_eq!(highlights(cx, editor), Vec::<Range<usize>>::new());
    }

    // The buffer closed everywhere, its sets go with it.
    let (state, set) = on_thread(cx, &paint, state, move |state| {
        state.set_decorations(id, 2, "lint".into(), vec![highlight(0..4)])
    });
    set.unwrap();
    let snapshot = shown.read_with(cx, |editor, _| editor.model().buffer().snapshot());
    let kept = |paint: &Paint| {
        paint
            .decorations
            .decorations(id, &snapshot, 0..snapshot.len())
    };
    assert_eq!(kept(&paint).len(), 1);
    cx.update_window(first.into(), |_, window, _| window.remove_window())
        .unwrap();
    // Its changes discarded.
    second
        .update(cx, |workbench, window, cx| {
            let pane = workbench.pane().clone();
            pane.update(cx, |pane, cx| {
                pane.close_matching(true, |_, _| true, window, cx)
            });
        })
        .unwrap();
    draw(cx, second);
    assert!(kept(&paint).is_empty());

    // Opened again, the file is another buffer; unloading the plugin takes
    // its sets off it.
    second
        .update(cx, |workbench, window, cx| {
            let pane = workbench.pane().clone();
            pane.update(cx, |pane, cx| pane.open(a, window, cx));
        })
        .unwrap();
    draw(cx, second);
    let reopened = editors(cx, second)[0].clone();
    let again = cx.update(|cx| buffer_id(&reopened, cx)).unwrap();
    assert_ne!(again, id);
    let (_state, set) = on_thread(cx, &paint, state, move |state| {
        state.set_decorations(again, 1, "lint".into(), vec![highlight(0..4)])
    });
    set.unwrap();
    draw(cx, second);
    assert_eq!(highlights(cx, &reopened), [0..4]);
    cx.update(|cx| unload_plugin(PAINTER, cx));
    until(cx, "the plugin to unload", |cx| {
        let states = cx.update(|cx| plugin_states(cx));
        (states.iter()).any(|info| info.id == PAINTER && info.state != PluginState::Running)
    });
    draw(cx, second);
    assert_eq!(highlights(cx, &reopened), Vec::<Range<usize>>::new());
}

/// The plugins' items on `window`'s bar: side, text and tooltip, by id.
fn bar(
    cx: &mut TestAppContext,
    window: WindowHandle<Workbench>,
) -> Vec<(String, StatusSide, String, Option<String>)> {
    draw(cx, window);
    window
        .update(cx, |workbench, _, cx| {
            let items = workbench.status_bar().read(cx).items(cx);
            (items.into_iter())
                .filter(|item| item.id.starts_with("plugin:"))
                .map(|item| {
                    let tooltip = item.tooltip.map(|tooltip| tooltip.to_string());
                    (
                        item.id.to_string(),
                        item.side,
                        item.text.to_string(),
                        tooltip,
                    )
                })
                .collect()
        })
        .unwrap()
}

/// Clicks item `id` on `window`'s bar: dispatches its command as the bar
/// does.
fn click(cx: &mut TestAppContext, window: WindowHandle<Workbench>, id: &str) {
    let items = window
        .update(cx, |workbench, _, cx| {
            workbench.status_bar().read(cx).items(cx)
        })
        .unwrap();
    let item = (items.into_iter()).find(|item| item.id == id).unwrap();
    let mut command = item.command.unwrap();
    // Not while the workbench is leased: its handler takes the click.
    cx.update_window(window.into(), |_, window, cx| {
        command.context = kiln_platform::keys::context_stack(&window.context_stack(), cx);
        kiln_platform::command_actions::dispatch_invocation(command, window, cx).unwrap();
    })
    .unwrap();
    draw(cx, window);
}

fn item(id: &str, text: &str, command: &str, alignment: Alignment) -> Item {
    Item {
        id: id.into(),
        text: text.into(),
        tooltip: None,
        command: Some(command.into()),
        alignment,
    }
}

#[gpui::test]
fn status_items_show_and_run_their_command(cx: &mut TestAppContext) {
    let dir = start(cx, "alpha\n");
    let first = window(cx, Some(dir.file("a.txt")));
    let second = window(cx, None);
    let tool = dir.install(TOOL, COMMANDS, &["commands", "status"]);
    let bare = dir.install(BARE, EMPTY, &["status"]);
    load(cx, &[TOOL, BARE], vec![tool, bare]);
    let paint = cx.update(paint);
    let status = |plugin: &str, granted: &[&str]| HostState {
        status: paint.status(plugin, &grants(granted)),
        ..HostState::default()
    };
    let mut tool = status(TOOL, &["commands", "status"]);
    let mut bare = status(BARE, &["status"]);
    let mut all = item("all", "Select All", "editor.select_all", Alignment::Right);
    all.tooltip = Some("Selects the whole buffer".into());
    tool.set_item(all).unwrap();
    let hello = item("hello", "Hello", "acme.tool.hello", Alignment::Left);
    tool.set_item(hello).unwrap();
    let ungranted = item("all", "All", "editor.select_all", Alignment::Left);
    bare.set_item(ungranted).unwrap();
    paint.repaint();

    // Every window's bar shows them, on their sides, after the built-in
    // items.
    let tool_all = ("plugin:acme.tool:all".to_owned(), StatusSide::Right);
    let shown = vec![
        (
            "plugin:acme.bare:all".into(),
            StatusSide::Left,
            "All".into(),
            None,
        ),
        (
            "plugin:acme.tool:hello".into(),
            StatusSide::Left,
            "Hello".into(),
            None,
        ),
        (
            tool_all.0.clone(),
            tool_all.1,
            "Select All".into(),
            Some("Selects the whole buffer".into()),
        ),
    ];
    assert_eq!(bar(cx, first), shown);
    assert_eq!(bar(cx, second), shown);

    // A click runs the item's command as its plugin, from the mouse, on
    // the window's editor; one its plugin's grants don't allow is refused.
    let editor = editors(cx, first)[0].clone();
    let selected = |cx: &mut TestAppContext| {
        editor.read_with(cx, |editor, _| editor.model().primary_selection().range)
    };
    let seen: Rc<RefCell<Vec<Invocation>>> = Rc::default();
    let observer = seen.clone();
    cx.update(|cx| {
        fe_gpui::commands::observe_invocations(cx, move |invocation| {
            observer.borrow_mut().push(invocation.clone());
        })
    });
    click(cx, first, "plugin:acme.bare:all");
    assert_eq!(selected(cx), 0..0, "refused: no `commands` grant");
    click(cx, first, &tool_all.0);
    assert_eq!(selected(cx), 0..6);
    let ran: Vec<_> = (seen.borrow().iter())
        .filter(|invocation| invocation.command.as_str() == "editor.select_all")
        .map(|invocation| (invocation.actor.clone(), invocation.source))
        .collect();
    assert_eq!(ran, [(Actor::Host(TOOL.into()), Source::Mouse)]);
    // One of the plugin's own runs on its thread.
    click(cx, first, "plugin:acme.tool:hello");
    let hello = format!("log {TOOL} on plugin {TOOL}: acme.tool.hello");
    until(cx, "the plugin's command to run", |cx| {
        cx.update(|cx| {
            let record = cx.global::<PluginHost>().record.lock().unwrap();
            record.contains(&hello)
        })
    });

    // Removed, the item leaves every bar; unloaded, its plugin's do.
    tool.remove_item("all".into()).unwrap();
    paint.repaint();
    assert_eq!(bar(cx, first), shown[..2]);
    assert_eq!(bar(cx, second), shown[..2]);
    cx.update(|cx| unload_plugin(TOOL, cx));
    until(cx, "the plugin to unload", |cx| {
        let states = cx.update(|cx| plugin_states(cx));
        (states.iter()).any(|info| info.id == TOOL && info.state != PluginState::Running)
    });
    assert_eq!(bar(cx, first), shown[..1]);
    assert_eq!(bar(cx, second), shown[..1]);
}
