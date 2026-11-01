//! `[extensions] disabled` in the user's settings.toml turns an installed
//! extension off with no restart (extensions record §10): its language
//! server, running in a trusted workbench, shuts down and isn't launched
//! again while it's disabled, its grammar is withdrawn and its snippets
//! aren't offered; enabled again they come back, and its server launches
//! afresh under the consent the user gave, which is kept.

#[allow(dead_code)]
#[path = "../src/config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use fe_config::SettingsStore;
use fe_gpui::{EditorSettings, EditorTheme};
use fe_lsp::fake::{FakeServer, Script};
use fe_workbench::{Launch, Workbench};
use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    platform::config::{BatchSender, batch_channel, diagnostics},
    ui::ThemeMode,
};

/// The grammar's language, unique to this test in the process-wide
/// registry.
const LANGUAGE: &str = "tomlx_disabled";

/// The extension's language server.
const SERVER: &str = "tomlx";

/// The TOML extension under `extensions`: a language server for TOML, a
/// grammar and a snippet; and another with a snippet of its own.
fn install(extensions: &Path) {
    let dir = extensions.join("tomlx");
    fs::create_dir_all(&dir).unwrap();
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let fixture = Path::new(&manifest).join("../fe-syntax/tests/fixtures/tree-sitter-json.wasm");
    fs::copy(&fixture, dir.join("tree-sitter-json.wasm")).unwrap();
    // An absolute command: no login shell to look it up in. The fake
    // launcher runs nothing.
    fs::write(
        dir.join("extension.toml"),
        format!(
            "id = \"acme.tomlx\"\nname = \"TOMLX\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\ntenmoku = \"1.1\"\n\n\
             [contributes.lsp.{SERVER}]\ncommand = \"/usr/bin/true\"\nlanguages = [\"toml\"]\n\n\
             [[contributes.grammars]]\nlanguage = \"{LANGUAGE}\"\n\
             wasm = \"tree-sitter-json.wasm\"\nextensions = [\"{LANGUAGE}\"]\n\n\
             [[contributes.snippets]]\nprefix = \"table\"\nbody = \"[$0]\"\n"
        ),
    )
    .unwrap();
    let other = extensions.join("other");
    fs::create_dir_all(&other).unwrap();
    fs::write(
        other.join("extension.toml"),
        "id = \"acme.other\"\nname = \"Other\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
         [engine]\ntenmoku = \"1.1\"\n\n\
         [[contributes.snippets]]\nprefix = \"todo\"\nbody = \"TODO: $0\"\n",
    )
    .unwrap();
}

/// The app over `state`: fastedit's commands and config with the extensions
/// in `state/extensions`, the user's settings in `state/settings.toml`, the
/// consent and trust files in `state`, and `fake` as every language
/// server. `root` is trusted.
fn load(cx: &mut TestAppContext, state: &Path, root: &Path, fake: &FakeServer) -> BatchSender {
    let user = state.join("settings.toml");
    let (sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .commands(config::register_commands)
        .settings_store(SettingsStore::load(Some(user), None).into_inner())
        .theme_mode(ThemeMode::Dark)
        .watch_with(batches);
    let config = config::with_extensions(config, state.join("extensions"));
    let consent = state.join(fe_workbench::CONSENT_FILE);
    let trust = state.join("trust.toml");
    let launcher = fake.launcher();
    cx.update(|cx| {
        fe_workbench::set_server_launcher(cx, launcher);
        fe_workbench::set_extension_consent_file(cx, consent);
        fe_workbench::set_trust_file(cx, trust);
        kiln::init(cx, config);
        kiln::shell::trust::decide(root, true, cx);
    });
    cx.run_until_parked();
    sender
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

/// Draws until `done`: language servers run on threads of their own.
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        draw(cx);
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn open(workbench: &Entity<Workbench>, path: PathBuf, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

/// The launches of the extension's server, from 1, as the fake counts
/// every server's.
fn launches(fake: &FakeServer) -> Vec<usize> {
    (fake.launched().iter().enumerate())
        .filter(|(_, (spec, _))| spec.name == SERVER)
        .map(|(ix, _)| ix + 1)
        .collect()
}

/// Whether launch `launch` was told to exit, or killed.
fn stopped(fake: &FakeServer, launch: usize) -> bool {
    let exited = (fake.records_of(launch).iter()).any(|record| record.is_notification("exit"));
    exited || fake.kills() > 0
}

/// Writes the user's settings and sends the batch the watcher would.
fn save_settings(state: &Path, text: &str, sender: &BatchSender, cx: &mut VisualTestContext) {
    let user = state.join("settings.toml");
    fs::write(&user, text).unwrap();
    sender.send(vec![user]);
    draw(cx);
}

fn snippets(cx: &mut VisualTestContext) -> Vec<String> {
    let mut prefixes: Vec<String> = (cx.update(|_, cx| fe_workbench::extension_snippets(cx)))
        .into_iter()
        .map(|snippet| snippet.prefix)
        .collect();
    prefixes.sort();
    prefixes
}

fn grammar(registered: bool) -> impl FnMut(&mut VisualTestContext) -> bool {
    move |_| fe_syntax::language_by_name(LANGUAGE).is_some() == registered
}

#[gpui::test]
fn the_setting_reaches_the_app(cx: &mut TestAppContext) {
    // Server threads wake the event loop.
    cx.executor().allow_parking();
    let state = tempfile::tempdir().unwrap();
    install(&state.path().join("extensions"));
    fs::write(state.path().join("settings.toml"), "").unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path().canonicalize().unwrap();
    for name in ["a.toml", "b.toml", "c.toml"] {
        fs::write(root.join(name), "a = 1\n").unwrap();
    }
    let fake = FakeServer::new(Script::default());
    let sender = load(cx, state.path(), &root, &fake);
    let (workbench, mut cx) = window(cx, &root);

    // Enabled and granted, its server runs for a TOML file.
    let pending = cx.update(|_, cx| fe_workbench::pending_extension_consents(cx));
    assert_eq!(pending.len(), 1, "{pending:#?}");
    cx.update(|_, cx| fe_workbench::grant_extension_server(cx, &pending[0]))
        .unwrap();
    open(&workbench, root.join("a.toml"), &mut cx);
    until(&mut cx, "its server to launch", |_| {
        launches(&fake).len() == 1
    });
    let first = launches(&fake)[0];
    until(&mut cx, "its grammar", grammar(true));
    assert_eq!(snippets(&mut cx), ["table", "todo"]);
    let consent_file = state.path().join(fe_workbench::CONSENT_FILE);
    let consent = fs::read_to_string(&consent_file).unwrap();

    // Disabled, beside an ID nothing has: the server stops, and none of it
    // is offered; the other extension's snippet stays.
    save_settings(
        state.path(),
        "[extensions]\ndisabled = [\"acme.tomlx\", \"acme.uninstalled\"]\n",
        &sender,
        &mut cx,
    );
    until(&mut cx, "its server to stop", |_| stopped(&fake, first));
    assert_eq!(
        cx.update(|_, cx| fe_workbench::pending_extension_consents(cx)),
        []
    );
    until(&mut cx, "its grammar to go", grammar(false));
    assert_eq!(snippets(&mut cx), ["todo"]);
    let loaded: Vec<String> = (cx.update(|_, cx| kiln::extensions::loaded(cx)).into_iter())
        .map(|manifest| manifest.id)
        .collect();
    assert_eq!(loaded, ["acme.other", "acme.tomlx"]);
    // The unknown ID is no problem, and the consent stays recorded.
    assert_eq!(cx.update(|_, cx| diagnostics(cx).to_vec()), []);
    assert_eq!(fs::read_to_string(&consent_file).unwrap(), consent);

    // Another TOML file opened while it's disabled launches nothing.
    open(&workbench, root.join("b.toml"), &mut cx);
    std::thread::sleep(Duration::from_millis(100));
    draw(&mut cx);
    assert_eq!(launches(&fake), [first]);

    // Enabled again: it launches afresh without asking, and all is back.
    save_settings(state.path(), "", &sender, &mut cx);
    assert_eq!(
        cx.update(|_, cx| fe_workbench::pending_extension_consents(cx)),
        []
    );
    open(&workbench, root.join("c.toml"), &mut cx);
    until(&mut cx, "its server to launch again", |_| {
        launches(&fake).len() == 2
    });
    until(&mut cx, "its grammar again", grammar(true));
    assert_eq!(snippets(&mut cx), ["table", "todo"]);
}
