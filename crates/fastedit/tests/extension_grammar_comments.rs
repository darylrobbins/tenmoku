//! An extension's grammar declares its comment tokens and shebang
//! interpreters (`line_comment`, `block_comment`, `shebangs` in
//! `[[contributes.grammars]]`): a file with no extension and a `#!` line
//! naming one opens in its language, and `editor.toggle_comment` comments
//! with its token. Malformed values are refused, as is an interpreter a
//! built-in language claims.

#[allow(dead_code)]
#[path = "../src/config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;

use std::{
    fs,
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::SettingsStore;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use fe_workbench::{Launch, Workbench};
use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    core::Severity,
    platform::config::{batch_channel, diagnostics},
    ui::ThemeMode,
};

/// The tests share the process-wide grammar registration, where a newer
/// batch stops an older one, so they run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// An extension `id` under `extensions` with one grammar of the JSON
/// fixture called `language`, its manifest's grammar table ending in
/// `keys`.
fn install(extensions: &Path, id: &str, language: &str, keys: &str) {
    let dir = extensions.join(id);
    fs::create_dir_all(&dir).unwrap();
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let fixture = Path::new(&manifest).join("../fe-syntax/tests/fixtures/tree-sitter-json.wasm");
    fs::copy(&fixture, dir.join("tree-sitter-json.wasm")).unwrap();
    fs::write(
        dir.join("extension.toml"),
        format!(
            "id = \"acme.{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\ntenmoku = \"1.0\"\n\n\
             [[contributes.grammars]]\nlanguage = \"{language}\"\n\
             wasm = \"tree-sitter-json.wasm\"\n{keys}\n"
        ),
    )
    .unwrap();
}

/// The app with the extensions in `state/extensions`, and the error
/// diagnostics loading them gave.
fn load(cx: &mut TestAppContext, state: &Path) -> Vec<String> {
    let user = state.join("settings.toml");
    fs::write(&user, "").unwrap();
    let (_sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .commands(config::register_commands)
        .settings_store(SettingsStore::load(Some(user), None).into_inner())
        .theme_mode(ThemeMode::Dark)
        .watch_with(batches);
    let config = config::with_extensions(config, state.join("extensions"));
    cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
    (cx.update(|cx| diagnostics(cx).to_vec()).into_iter())
        .filter(|d| d.severity == Severity::Error)
        .map(|d| format!("{d:?}"))
        .collect()
}

/// Waits for `name` to be registered: registering compiles on a
/// background thread.
fn registered(cx: &mut TestAppContext, name: &str) {
    for _ in 0..600 {
        cx.run_until_parked();
        if fe_syntax::language_by_name(name).is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("{name} not registered");
}

/// A workbench over `root`, in a window of its own.
fn window(cx: &mut TestAppContext, root: &Path) -> (Entity<Workbench>, VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(*window, cx);
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

/// The editor of `path`, opened in the workbench, once its language has
/// loaded: the name of that language, or `None` for plain text.
fn open(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> (Entity<EditorView>, Option<&'static str>) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        draw(cx);
        let editor = pane.read_with(cx, |pane, _| pane.active_editor().cloned());
        if let Some(editor) = editor
            && !editor.read_with(cx, |editor, _| editor.is_loading_language())
        {
            let language = editor.read_with(cx, |editor, _| {
                editor.syntax().map(|syntax| syntax.language().name())
            });
            return (editor, language);
        }
        assert!(Instant::now() < deadline, "the file didn't open");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

/// Puts the caret at the start of `row` and runs `id` there, as its key
/// binding does.
fn run_at(editor: &Entity<EditorView>, row: u32, id: &str, cx: &mut VisualTestContext) {
    let at = editor.read_with(cx, |editor, _| editor.model().text().row_start(row));
    editor.update(cx, |editor, _| {
        editor.model_mut().select_range(at..at, false)
    });
    let context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor"));
    let invocation = Invocation::of(id).unwrap().context(context);
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    draw(cx);
}

#[gpui::test]
fn extension_grammar_comments_and_shebangs(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // The project's watcher thread wakes the event loop.
    cx.executor().allow_parking();
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    install(
        &extensions,
        "commented",
        "jsonc_t298",
        "line_comment = \"//\"\nblock_comment = [\"/*\", \"*/\"]\nshebangs = [\"jsont298\"]",
    );
    install(
        &extensions,
        "bare",
        "jsonb_t298",
        "extensions = [\"jsonb\"]",
    );
    assert_eq!(load(cx, state.path()), Vec::<String>::new());
    registered(cx, "jsonc_t298");
    registered(cx, "jsonb_t298");

    let project = tempfile::tempdir().unwrap();
    let root = project.path().canonicalize().unwrap();
    fs::write(root.join("tool"), "#!/usr/bin/env jsont298\n{}\n").unwrap();
    fs::write(root.join("versioned"), "#!/usr/bin/jsont2983\n{}\n").unwrap();
    fs::write(root.join("data.jsonb"), "{}\n").unwrap();
    let (workbench, mut cx) = window(cx, &root);
    let cx = &mut cx;

    let (tool, language) = open(&workbench, &root.join("tool"), cx);
    assert_eq!(language, Some("jsonc_t298"));
    run_at(&tool, 1, "editor.toggle_comment", cx);
    assert_eq!(text(&tool, cx), "#!/usr/bin/env jsont298\n// {}\n");
    run_at(&tool, 1, "editor.toggle_comment", cx);
    assert_eq!(text(&tool, cx), "#!/usr/bin/env jsont298\n{}\n");

    let (_, language) = open(&workbench, &root.join("versioned"), cx);
    assert_eq!(language, Some("jsonc_t298"));

    // A grammar without the keys behaves as before: no comment tokens.
    assert_eq!(fe_syntax::comment_tokens("jsonb_t298"), None);
    let (bare, language) = open(&workbench, &root.join("data.jsonb"), cx);
    assert_eq!(language, Some("jsonb_t298"));
    run_at(&bare, 0, "editor.toggle_comment", cx);
    assert_eq!(text(&bare, cx), "{}\n");
}

#[gpui::test]
fn malformed_comment_keys_and_claimed_shebangs_are_refused(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let malformed = [
        ("one_token", "block_comment = [\"/*\"]"),
        ("three_tokens", "block_comment = [\"/*\", \"*/\", \"//\"]"),
        ("empty_close", "block_comment = [\"/*\", \"\"]"),
        ("empty_line", "line_comment = \"\""),
        ("not_a_string", "line_comment = 1"),
        ("empty_shebang", "shebangs = [\"\"]"),
        ("shebang_path", "shebangs = [\"/usr/bin/foo\"]"),
        ("shebang_space", "shebangs = [\"foo bar\"]"),
    ];
    for (id, keys) in malformed {
        install(&extensions, id, &format!("{id}_t298"), keys);
    }
    install(
        &extensions,
        "claimed",
        "claimed_t298",
        "shebangs = [\"python3\"]",
    );
    install(
        &extensions,
        "fine",
        "fine_t298",
        "shebangs = [\"finet298\"]",
    );
    // kiln's manifest check stops at the array of tables: the grammar
    // loader is what refuses these, logging each and registering the rest.
    load(cx, state.path());
    registered(cx, "fine_t298");
    for (id, _) in malformed {
        assert!(
            fe_syntax::language_by_name(&format!("{id}_t298")).is_none(),
            "{id}"
        );
    }
    assert!(fe_syntax::language_by_name("claimed_t298").is_none());
    assert_eq!(
        fe_syntax::language_name_for_shebang("#!/usr/bin/env finet298"),
        Some("fine_t298")
    );
}
