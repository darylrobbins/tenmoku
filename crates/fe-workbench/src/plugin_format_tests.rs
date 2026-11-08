//! Format Document and format on save with a plugin's formatter: fe-plugin's
//! editor guest, whose `fmt.format` upper-cases the text, beside the fake
//! language server and a fake external formatter. The plugin runs on its own
//! thread while the main thread takes its jobs.

use std::{
    cell::Cell,
    fs,
    future::{self, Future},
    io,
    path::Path,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use fe_command::{
    ContextFrame, ContextStack, Invocation,
    capability::{Capability, Grants},
};
use fe_config::{Settings, SettingsEvent, SettingsStore};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use kiln::core::{
    ApiVersion,
    extension::{ExtensionApi, load_manifest},
};
use kiln_platform::config::{self, BatchSender, ConfigService};
use kiln_plugin::{
    grants::GrantStore,
    manifest::{ExtensionOrigin, PluginManifest},
    runner::Call,
};
use lsp_types::{OneOf, PositionEncodingKind, ServerCapabilities};
use serde_json::json;

use super::PluginFormatting;
use crate::{
    external_format::{RunOutput, RunSpec, Runner},
    plugin_host::{
        Found, GRANTS_FILE, PluginHost, PluginState, plugin_handle, plugin_kind, plugin_states,
        set_plugin_dirs, set_plugins, set_preview,
    },
    tests::lsp::Lsp,
};

const EDITOR_GUEST: &[u8] = include_bytes!("../../fe-plugin/tests/guests/editor/editor.wasm");
/// The guest's id: its formatters are `fmt.<name>`, its own namespace.
const PLUGIN: &str = "fmt";
const TEXT: &str = "fn main() {\n\tf();\n}\n";
const UPPER: &str = "FN MAIN() {\n\tF();\n}\n";
/// What the fake external formatter gives.
const SPACED: &str = "fn main() {\n    f();\n}\n";
const PREVIEW: &str = "[extensions]\nplugins_preview = true\n";
const EXTERNAL: &str = "[formatters.rust]\ncommand = [\"rustfmt\"]\n\n\
                        [extensions]\nplugins_preview = true\n";
const FORMATTING: &str = "textDocument/formatting";

/// An external formatter that gives [`SPACED`], or isn't there.
struct FakeRunner {
    missing: Cell<bool>,
    calls: Cell<usize>,
}

impl Runner for FakeRunner {
    fn run(&self, _: RunSpec) -> Pin<Box<dyn Future<Output = io::Result<RunOutput>>>> {
        self.calls.set(self.calls.get() + 1);
        let answer = match self.missing.get() {
            true => Err(io::ErrorKind::NotFound.into()),
            false => Ok(RunOutput {
                code: Some(0),
                stdout: SPACED.into(),
                stderr: Vec::new(),
            }),
        };
        Box::pin(future::ready(answer))
    }
}

/// A rust server that formats documents.
fn server_formatter() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        document_formatting_provider: Some(OneOf::Left(true)),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

struct Wired {
    s: Lsp,
    editor: Entity<EditorView>,
    runner: Rc<FakeRunner>,
    _kept: (tempfile::TempDir, BatchSender, gpui::Task<()>),
}

/// The workbench over the fakes, `settings` in force, `src/main.rs` (as
/// TEXT) open and focused, and the editor guest loaded as plugin `fmt` with
/// `fmt.format` the formatter for rust.
fn setup(cx: &mut TestAppContext, rust: Script, settings: &str) -> Wired {
    let mut s = Lsp::with_scripts(cx, rust, Script::default());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    fs::write(&path, settings).unwrap();
    let store = SettingsStore::load(Some(path), None);
    let (sender, batches) = config::batch_channel();
    let service = s.cx.update(|_, cx| {
        ConfigService::new()
            .settings(store.into_inner())
            .start_with(batches, cx)
    });
    let runner = Rc::new(FakeRunner {
        missing: Cell::new(false),
        calls: Cell::new(0),
    });
    let installed = runner.clone();
    (s.workbench).update(&mut s.cx, |w, _| w.format_runner = installed);
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), TEXT).unwrap();
    s.open("src/main.rs");
    s.fake
        .wait_for_document(&s.uri("src/main.rs"), "didOpen", |r| r.is_some())
        .unwrap();
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));

    let found = install(dir.path());
    let (state, cache) = (dir.path().join("state"), dir.path().join("cache"));
    s.cx.update(|_, cx| {
        set_plugin_dirs(cx, state, cache);
        set_preview(cx, true);
        cx.global_mut::<PluginHost>().limits.call_deadline = Duration::from_secs(30);
        set_plugins(cx, vec![found]);
    });
    s.until("the plugin to run", |s| {
        let states = s.cx.update(|_, cx| plugin_states(cx));
        (states.iter()).any(|info| info.id == PLUGIN && info.state == PluginState::Running)
    });
    let mut wired = Wired {
        s,
        editor,
        runner,
        _kept: (dir, sender, service),
    };
    register(&mut wired.s, "fmt.format");
    wired
}

/// The editor guest installed under `dir` and granted what a formatter
/// needs. Its manifest has the id `acme.fmt`, as extension ids have two
/// segments, but it's found as `fmt`, the namespace its commands are in.
fn install(dir: &Path) -> Found {
    let folder = dir.join("extensions").join("acme.fmt");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("plugin.wasm"), EDITOR_GUEST).unwrap();
    let capabilities = ["commands", "buffer.read", "buffer.write"];
    fs::write(
        folder.join("extension.toml"),
        "id = \"acme.fmt\"\nname = \"Shouter\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
         [engine]\ntenmoku = \"1.1\"\n\n[contributes.plugin]\nwasm = \"plugin.wasm\"\n\
         capabilities = [\"commands\", \"buffer.read\", \"buffer.write\"]\n",
    )
    .unwrap();
    let grants: Grants = (capabilities.iter())
        .map(|capability| Capability::parse(capability).unwrap())
        .collect();
    GrantStore::new(dir.join("state").join(GRANTS_FILE))
        .grant(PLUGIN, (1, 0, 0), &grants, &[], false)
        .unwrap();
    let api = ExtensionApi {
        app: fe_config::paths::app_id().short,
        version: ApiVersion { major: 1, minor: 1 },
        reserved: Vec::new(),
        kinds: vec![plugin_kind()],
        allowed_licenses: None,
        only_kinds: None,
    };
    let (manifest, diagnostics) = load_manifest(&folder, &api);
    let mut manifest = manifest.unwrap_or_else(|| panic!("{diagnostics:#?}"));
    let mut plugin = PluginManifest::from_extension(&manifest, ExtensionOrigin::User)
        .unwrap()
        .unwrap();
    manifest.id = PLUGIN.into();
    plugin.id = PLUGIN.into();
    Found::new(&manifest, Ok(plugin))
}

/// Makes the guest's `command` the formatter for rust, calling it on a
/// thread of the test's while the main thread takes its jobs.
fn register(s: &mut Lsp, command: &str) {
    let handle = s.cx.update(|_, cx| plugin_handle(PLUGIN, cx)).unwrap();
    let call = Call::RunCommand {
        id: "register-formatter".into(),
        args: format!("{command} rust"),
    };
    let thread = std::thread::spawn(move || handle.lock().unwrap().call(call));
    let deadline = Instant::now() + Duration::from_secs(30);
    while !thread.is_finished() {
        assert!(Instant::now() < deadline, "register didn't return");
        s.draw();
    }
    let reply = thread.join().unwrap();
    assert!(
        matches!(reply, Ok(kiln_plugin::runner::Reply::RunCommand(Ok(())))),
        "{reply:?}"
    );
}

/// What the plugin formatters' requests did.
fn record(s: &mut Lsp) -> Vec<String> {
    s.cx.update(|_, cx| {
        cx.global::<PluginFormatting>()
            .record
            .lock()
            .unwrap()
            .clone()
    })
}

fn asked(s: &mut Lsp) -> usize {
    (record(s).iter())
        .filter(|line| line.starts_with("asked"))
        .count()
}

fn answered(s: &mut Lsp) -> usize {
    (record(s).iter())
        .filter(|line| *line == "answered")
        .count()
}

fn text(s: &Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

fn on_disk(s: &Lsp) -> String {
    fs::read_to_string(s.path("src/main.rs")).unwrap()
}

fn run(s: &mut Lsp, command: &str) {
    let stack = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(fe_editor::commands::CONTEXT));
    let invocation = Invocation::of(command).unwrap().context(stack);
    s.cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    s.draw();
}

fn toasts(s: &Lsp) -> Vec<String> {
    let layer = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    layer.read_with(&s.cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

fn undo(s: &mut Lsp) {
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
}

#[gpui::test]
fn a_plugin_formats_a_document_nothing_else_covers(cx: &mut TestAppContext) {
    let Wired { mut s, editor, .. } = setup(cx, Script::default(), PREVIEW);
    run(&mut s, "lsp.format_document");
    s.until("the plugin's format", |s| text(s, &editor) == UPPER);
    assert_eq!(asked(&mut s), 1);
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));
    // One step to undo.
    undo(&mut s);
    assert_eq!(text(&s, &editor), TEXT);

    // A selection is never a plugin's.
    run(&mut s, "lsp.format_selection");
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["No formatter for Rust"]);
    assert_eq!(asked(&mut s), 1);

    // Its formatter failing, answering edits out of order, is one line
    // naming the plugin.
    register(&mut s, "fmt.unordered");
    run(&mut s, "lsp.format_document");
    s.until("the toast", |s| toasts(s).len() == 2);
    let failed = &toasts(&s)[1];
    assert!(
        failed.starts_with("Plugin Shouter's formatter failed: ") && !failed.contains('\n'),
        "{failed}"
    );
    assert_eq!(text(&s, &editor), TEXT);
}

#[gpui::test]
fn external_and_server_formatters_come_first(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, server_formatter(), EXTERNAL);
    // The external formatter goes first.
    run(&mut s, "lsp.format_document");
    s.until("the external format", |s| text(s, &editor) == SPACED);
    assert_eq!(runner.calls.get(), 1);
    undo(&mut s);
    assert_eq!(text(&s, &editor), TEXT);

    // It isn't there: the server formats.
    runner.missing.set(true);
    let edit = json!({
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 2 } },
        "newText": "pub fn",
    });
    s.fake
        .set_reply(FORMATTING, Scripted::now(Reply::Result(json!([edit]))));
    run(&mut s, "lsp.format_document");
    let served = TEXT.replacen("fn", "pub fn", 1);
    s.until("the server's format", |s| text(s, &editor) == served);
    undo(&mut s);
    assert_eq!(text(&s, &editor), TEXT);
    assert_eq!(asked(&mut s), 0, "neither falls through to the plugin");

    // Both fall through, the server failing: the plugin formats, and the
    // server's failure shows.
    s.fake.set_reply(
        FORMATTING,
        Scripted::now(Reply::Error {
            code: -32603,
            message: "it broke".into(),
        }),
    );
    run(&mut s, "lsp.format_document");
    s.until("the plugin's format", |s| text(s, &editor) == UPPER);
    assert_eq!(asked(&mut s), 1);
    assert_eq!(
        runner.calls.get(),
        3,
        "the external formatter is tried each time"
    );
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s).len(), 1, "{:?}", toasts(&s));
    assert!(toasts(&s)[0].contains("it broke"), "{:?}", toasts(&s));
}

fn format_on_save(s: &mut Lsp) {
    let mut settings =
        s.cx.update(|_, cx| Settings::clone(&config::settings::<Settings>(cx)));
    settings.editor.format_on_save = true;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    (s.workbench).update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.draw();
}

#[gpui::test]
fn format_on_save_uses_the_plugin_formatter(cx: &mut TestAppContext) {
    let Wired { mut s, editor, .. } = setup(cx, Script::default(), PREVIEW);
    format_on_save(&mut s);
    s.cx.simulate_keystrokes("cmd-s");
    s.until("the formatted text written", |s| on_disk(s) == UPPER);
    assert_eq!(text(&s, &editor), UPPER);
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));

    // It fails, answering edits out of order: the text is saved as it is,
    // and nothing is shown.
    register(&mut s, "fmt.unordered");
    s.cx.simulate_input("x");
    let typed = text(&s, &editor);
    s.cx.simulate_keystrokes("cmd-s");
    s.until("the failed format's save", |s| on_disk(s) == typed);
    assert_eq!(answered(&mut s), 2);
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));

    // It doesn't answer within the plugin's call deadline, here held off:
    // likewise.
    register(&mut s, "fmt.format");
    s.cx.update(|_, cx| {
        cx.global_mut::<PluginHost>().limits.call_deadline = Duration::from_millis(100);
    });
    let handle = s.cx.update(|_, cx| plugin_handle(PLUGIN, cx)).unwrap();
    let held = handle.lock().unwrap();
    s.cx.simulate_input("y");
    let typed = text(&s, &editor);
    s.cx.simulate_keystrokes("cmd-s");
    s.until("the request", |s| asked(s) == 3);
    s.cx.executor().advance_clock(Duration::from_millis(100));
    s.until("the timed-out format's save", |s| on_disk(s) == typed);
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));
    // Its answer, once it comes, is dropped.
    drop(held);
    s.until("the late answer", |s| answered(s) == 3);
    s.draw();
    assert_eq!(text(&s, &editor), typed);
}

#[gpui::test]
fn a_stale_answer_is_dropped(cx: &mut TestAppContext) {
    let Wired { mut s, editor, .. } = setup(cx, Script::default(), PREVIEW);
    // Held off until the user has typed: it then reads the newer text and
    // answers for that, which doesn't fit the text it was asked about.
    let handle = s.cx.update(|_, cx| plugin_handle(PLUGIN, cx)).unwrap();
    let held = handle.lock().unwrap();
    run(&mut s, "lsp.format_document");
    s.until("the request", |s| asked(s) == 1);
    s.cx.simulate_input("x");
    let typed = text(&s, &editor);
    assert_ne!(typed, TEXT);
    drop(held);
    s.until("the answer", |s| answered(s) == 1);
    s.draw();
    assert_eq!(text(&s, &editor), typed);
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));

    // Not held off, it formats.
    run(&mut s, "lsp.format_document");
    s.until("the plugin's format", |s| {
        text(s, &editor) == typed.to_uppercase()
    });
}
