//! Plugin diagnostics shown as a language server's: in every editor of their
//! buffer and in the Problems panel, beside the fake server of
//! `tests/lsp.rs`, and gone once the plugin unloads or reloads or the buffer
//! closes, from an editor made from a copy of another's model too. The
//! plugin runs as loaded; the tests publish through a diagnostics slice made
//! as the host makes one, on a thread of their own as the plugin's would.

use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

use fe_command::capability::Capability;
use fe_gpui::EditorView;
use fe_plugin::{HostState, fastedit::editor::diagnostics::Host as _};
use gpui::{Entity, TestAppContext};
use kiln::core::{
    ApiVersion,
    extension::{ExtensionApi, load_manifest},
};
use kiln_plugin::{
    bindings::kiln::extension::types::{Diagnostic, Range, Severity},
    grants::GrantStore,
    manifest::{ExtensionOrigin, PluginManifest},
};
use lsp_types::{
    DiagnosticSeverity, Position, PublishDiagnosticsParams, notification::PublishDiagnostics,
};

use super::diagnostics;
use crate::{
    dock::problems_panel::ShownRow,
    plugin_bridge,
    plugin_host::{
        Found, GRANTS_FILE, PluginHost, PluginState, bridge_buffers, plugin_states,
        set_plugin_dirs, set_plugins, set_preview, unload_plugin,
    },
    plugin_paint,
    tests::lsp::Lsp,
};

/// Does nothing: the tests publish for it.
const EMPTY: &str = include_str!("../../../kiln/crates/kiln-plugin/tests/fixtures/empty.wat");

const LINT: &str = "acme.lint";
const NAME: &str = "Acme Lint";

/// `src/main.rs`: `x` at offset 20, `y` at 31.
const MAIN: &str = "fn main() {\n    let x = 1;\n    y();\n}\n";
const X: std::ops::Range<usize> = 20..21;
const Y: std::ops::Range<usize> = 31..32;

/// A diagnostic as an editor has it: range, message and source.
type Shown = (std::ops::Range<usize>, String, Option<String>);

/// The fake server, `src/main.rs` open in it, and plugin [`LINT`] running,
/// its files in `plugins`.
struct Setup {
    s: Lsp,
    plugins: tempfile::TempDir,
}

fn setup(cx: &mut TestAppContext) -> Setup {
    let mut s = Lsp::new(cx);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    let plugins = tempfile::tempdir().unwrap();
    let (state, cache) = (plugins.path().join("state"), plugins.path().join("cache"));
    s.cx.cx.update(|cx| {
        set_plugin_dirs(cx, state, cache);
        set_preview(cx, true);
        cx.global_mut::<PluginHost>().limits.call_deadline = Duration::from_secs(30);
    });
    s.open("src/main.rs");
    let uri = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    let mut setup = Setup { s, plugins };
    setup.install("");
    setup
}

impl Setup {
    /// Hands the host [`LINT`] with `extra` after its component, so another
    /// `extra` is a rebuilt one, which reloads it; runs until it runs.
    fn install(&mut self, extra: &str) {
        let dir = self.plugins.path().join("extensions").join(LINT);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plugin.wasm"), format!("{EMPTY}\n{extra}")).unwrap();
        fs::write(
            dir.join("extension.toml"),
            format!(
                "id = \"{LINT}\"\nname = \"{NAME}\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
                 [engine]\ntenmoku = \"1.1\"\n\n\
                 [contributes.plugin]\nwasm = \"plugin.wasm\"\ncapabilities = [\"decorations\"]\n"
            ),
        )
        .unwrap();
        GrantStore::new(self.plugins.path().join("state").join(GRANTS_FILE))
            .grant(LINT, (1, 0, 0), &grants(), &[], false)
            .unwrap();
        let found = found(&dir);
        self.s.cx.cx.update(|cx| set_plugins(cx, vec![found]));
        self.s
            .until("the plugin to run", |s| running(s) == Some(true));
    }

    /// A diagnostics slice of [`LINT`]'s, made as the host makes one: a new
    /// load of it.
    fn slice(&mut self) -> HostState {
        self.s.cx.cx.update(|cx| {
            let buffers = bridge_buffers(cx).expect("a plugin was loaded");
            HostState {
                diagnostics: plugin_paint::paint(cx).diagnostics(LINT, &grants(), buffers),
                ..HostState::default()
            }
        })
    }

    /// Runs `call` on `state` on a thread of the test's, as the plugin's
    /// thread would, the main thread serving the bridge meanwhile.
    fn on_thread<R: Send + 'static>(
        &mut self,
        state: HostState,
        call: impl FnOnce(&mut HostState) -> R + Send + 'static,
    ) -> (HostState, R) {
        let thread = std::thread::spawn(move || {
            let mut state = state;
            let returned = call(&mut state);
            (state, returned)
        });
        self.s.until("the call to return", |_| thread.is_finished());
        self.s.draw();
        thread.join().unwrap()
    }

    /// Publishes `items` as set `set` of `state`'s on the editors' buffer, at
    /// its version now.
    fn publish(&mut self, state: HostState, set: &str, items: Vec<Diagnostic>) -> HostState {
        let editor = self.editors()[0].clone();
        let (buffer, version) = self.s.cx.cx.update(|cx| {
            let buffer = plugin_bridge::buffer_id(&editor, cx).unwrap();
            (buffer, plugin_bridge::snapshot(buffer, cx).unwrap().version)
        });
        let set = set.to_owned();
        let (state, published) = self.on_thread(state, move |state| {
            state.publish(buffer, version, set, items)
        });
        published.unwrap();
        state
    }

    fn clear(&mut self, state: HostState, set: &str) -> HostState {
        let editor = self.editors()[0].clone();
        let buffer = (self.s.cx.cx).update(|cx| plugin_bridge::buffer_id(&editor, cx).unwrap());
        let set = set.to_owned();
        let (state, cleared) = self.on_thread(state, move |state| state.clear(buffer, set));
        cleared.unwrap();
        state
    }

    /// Has the fake server publish `items` for `src/main.rs`.
    fn server_publishes(&mut self, items: Vec<lsp_types::Diagnostic>) {
        let uri = self.s.uri("src/main.rs");
        let version = self.s.fake.document(&uri).expect("it's open").version;
        self.s
            .fake
            .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                uri,
                diagnostics: items,
                version: Some(version),
            });
    }

    /// A copy of the first editor's model, its diagnostics with it, as a
    /// split or a diff's retained source holds one.
    fn copy(&mut self) -> fe_editor::EditorModel {
        let editor = self.editors()[0].clone();
        editor.read_with(&self.s.cx, |view, _| view.model().split())
    }

    /// Opens `model` in a tab of its own in the active pane.
    fn open_copy(&mut self, model: fe_editor::EditorModel) {
        let pane = self
            .s
            .workbench
            .read_with(&self.s.cx, |w, _| w.pane().clone());
        let before = self.editors().len();
        self.s
            .cx
            .update(|_, cx| pane.update(cx, |pane, cx| pane.add(model, cx)));
        self.s.draw();
        assert_eq!(self.editors().len(), before + 1);
    }

    fn editors(&mut self) -> Vec<Entity<EditorView>> {
        let workbench = self.s.workbench.clone();
        workbench.read_with(&self.s.cx, |w, cx| w.open_editors(cx))
    }

    /// Each editor's diagnostics: range, message and source.
    fn shown(&mut self) -> Vec<Vec<Shown>> {
        let editors = self.editors();
        (editors.iter())
            .map(|editor| {
                editor.read_with(&self.s.cx, |view, _| {
                    (view.model().diagnostics().into_iter())
                        .map(|entry| {
                            (
                                entry.range.clone(),
                                entry.message.clone(),
                                entry.source.clone(),
                            )
                        })
                        .collect()
                })
            })
            .collect()
    }

    fn rows(&mut self) -> Vec<ShownRow> {
        let workbench = self.s.workbench.clone();
        let panel = workbench.read_with(&self.s.cx, |w, cx| w.dock().read(cx).problems().clone());
        panel.read_with(&self.s.cx, |panel, _| panel.shown_rows())
    }

    /// Draws until each editor shows `want` and the panel lists `rows`.
    fn until_shown(
        &mut self,
        what: &str,
        want: &[(std::ops::Range<usize>, &str, Option<&str>)],
        rows: &[ShownRow],
    ) {
        let want: Vec<_> = (want.iter())
            .map(|(range, message, source)| {
                (
                    range.clone(),
                    message.to_string(),
                    source.map(str::to_owned),
                )
            })
            .collect();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            self.s.draw();
            let shown = self.shown();
            let listed = self.rows();
            if shown.iter().all(|shown| *shown == want) && listed == rows {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "waited for {what}: editors {shown:?}, rows {listed:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn grants() -> fe_command::Grants {
    [Capability::parse("decorations").unwrap()]
        .into_iter()
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

/// Whether [`LINT`] runs, if it's found.
fn running(s: &mut Lsp) -> Option<bool> {
    let states = s.cx.cx.update(|cx| plugin_states(cx));
    let info = states.into_iter().find(|info| info.id == LINT)?;
    Some(info.state == PluginState::Running)
}

fn plugin_diagnostic(
    range: std::ops::Range<usize>,
    severity: Severity,
    message: &str,
    source: Option<&str>,
) -> Diagnostic {
    Diagnostic {
        range: Range {
            start: range.start as u64,
            end: range.end as u64,
        },
        severity,
        message: message.into(),
        source: source.map(str::to_owned),
        code: None,
    }
}

/// The fake server's warning on `x`.
fn server_warning() -> lsp_types::Diagnostic {
    lsp_types::Diagnostic {
        range: lsp_types::Range::new(Position::new(1, 8), Position::new(1, 9)),
        severity: Some(DiagnosticSeverity::WARNING),
        source: Some("rustc".into()),
        message: "unused variable: `x`".into(),
        ..Default::default()
    }
}

fn file_row(count: usize) -> ShownRow {
    ShownRow::File("src/main.rs".into(), count)
}

fn row(severity: &'static str, message: &str, source: &str, at: &str) -> ShownRow {
    ShownRow::Problem(severity, message.into(), Some(source.into()), at.into())
}

#[gpui::test]
fn published_diagnostics_show_in_the_editor_and_the_problems_panel(cx: &mut TestAppContext) {
    let mut t = setup(cx);
    // Every editor of the buffer: a split too.
    t.s.run("pane.split_right");
    assert_eq!(t.editors().len(), 2);
    let state = t.slice();
    let _state = t.publish(
        state,
        "lint",
        vec![
            plugin_diagnostic(Y, Severity::Error, "cannot find `y`", None),
            plugin_diagnostic(X, Severity::Warning, "`x` is unused", Some("clippy")),
        ],
    );
    // The one without a source shows the plugin's name.
    t.until_shown(
        "the plugin's diagnostics",
        &[
            (X, "`x` is unused", Some("clippy")),
            (Y, "cannot find `y`", Some(NAME)),
        ],
        &[
            file_row(2),
            row("W", "`x` is unused", "clippy", "2:9"),
            row("E", "cannot find `y`", NAME, "3:5"),
        ],
    );
    // Next and previous diagnostic reach them, as the squiggles and inline
    // diagnostics, which draw the editor's diagnostics, do.
    for editor in t.editors() {
        editor.read_with(&t.s.cx, |view, _| {
            let model = view.model();
            assert_eq!(model.next_diagnostic(0, true), Some(X));
            assert_eq!(model.next_diagnostic(X.start, true), Some(Y));
            assert_eq!(model.next_diagnostic(Y.start, false), Some(X));
        });
    }
}

#[gpui::test]
fn server_and_plugin_diagnostics_coexist(cx: &mut TestAppContext) {
    let mut t = setup(cx);
    let both: [(std::ops::Range<usize>, &str, Option<&str>); 2] = [
        (X, "unused variable: `x`", Some("rustc")),
        (Y, "cannot find `y`", Some("lint")),
    ];
    let both_rows = [
        file_row(2),
        row("W", "unused variable: `x`", "rustc", "2:9"),
        row("E", "cannot find `y`", "lint", "3:5"),
    ];
    let error = || plugin_diagnostic(Y, Severity::Error, "cannot find `y`", Some("lint"));
    t.server_publishes(vec![server_warning()]);
    let state = t.slice();
    let state = t.publish(state, "lint", vec![error()]);
    t.until_shown("both", &both, &both_rows);

    // The server clears its own: the plugin's stays.
    t.server_publishes(Vec::new());
    t.until_shown(
        "the plugin's alone",
        &[(Y, "cannot find `y`", Some("lint"))],
        &[file_row(1), row("E", "cannot find `y`", "lint", "3:5")],
    );

    // The plugin clears its own: the server's stays.
    t.server_publishes(vec![server_warning()]);
    t.until_shown("both again", &both, &both_rows);
    let state = t.clear(state, "lint");
    t.until_shown(
        "the server's alone",
        &[(X, "unused variable: `x`", Some("rustc"))],
        &[
            file_row(1),
            row("W", "unused variable: `x`", "rustc", "2:9"),
        ],
    );

    // Published again by the same load, it shows again.
    let _state = t.publish(state, "lint", vec![error()]);
    t.until_shown("both once more", &both, &both_rows);
}

#[gpui::test]
fn unloading_reloading_or_closing_clears_them(cx: &mut TestAppContext) {
    let mut t = setup(cx);
    let error = || plugin_diagnostic(Y, Severity::Error, "cannot find `y`", Some("lint"));
    let shown = [(Y, "cannot find `y`", Some("lint"))];
    let shown_rows = [file_row(1), row("E", "cannot find `y`", "lint", "3:5")];

    // Unloaded.
    let state = t.slice();
    let _state = t.publish(state, "lint", vec![error()]);
    t.until_shown("the diagnostics", &shown, &shown_rows);
    t.s.cx.cx.update(|cx| unload_plugin(LINT, cx));
    t.s.until("the plugin to unload", |s| running(s) == Some(false));
    t.until_shown("none after the unload", &[], &[]);

    // Reloaded: loaded again, then rebuilt.
    t.install(";; again");
    let state = t.slice();
    let _state = t.publish(state, "lint", vec![error()]);
    t.until_shown("the diagnostics of the new load", &shown, &shown_rows);
    t.install(";; rebuilt");
    t.until_shown("none after the reload", &[], &[]);

    // The buffer closed: they leave the store too.
    let state = t.slice();
    let _state = t.publish(state, "lint", vec![error()]);
    t.until_shown("the diagnostics before the close", &shown, &shown_rows);
    let editor = t.editors()[0].clone();
    let (buffer, snapshot) = t.s.cx.cx.update(|cx| {
        let buffer = plugin_bridge::buffer_id(&editor, cx).unwrap();
        (buffer, editor.read(cx).model().buffer().snapshot())
    });
    assert_eq!(
        t.s.cx
            .cx
            .update(|cx| diagnostics(cx).kept(buffer, &snapshot))
            .len(),
        1
    );
    let pane = t.s.workbench.read_with(&t.s.cx, |w, _| w.pane().clone());
    t.s.cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.close_matching(true, |_, _| true, window, cx)
        })
    });
    t.s.draw();
    assert!(t.editors().is_empty());
    let rows = t.rows();
    assert!(rows.is_empty(), "{rows:?}");
    let kept =
        t.s.cx
            .cx
            .update(|cx| diagnostics(cx).kept(buffer, &snapshot));
    assert!(kept.is_empty(), "{kept:?}");
}

#[gpui::test]
fn an_editor_from_a_copied_model_drops_what_was_cleared_or_ended(cx: &mut TestAppContext) {
    let mut t = setup(cx);
    let error = || plugin_diagnostic(Y, Severity::Error, "cannot find `y`", Some("lint"));
    let warning = || plugin_diagnostic(X, Severity::Warning, "`x` is unused", Some("lint"));
    let errors = [(Y, "cannot find `y`", Some("lint"))];
    let error_rows = [file_row(1), row("E", "cannot find `y`", "lint", "3:5")];

    // A copy taken before the set was published again shows it as it is now.
    let state = t.slice();
    let state = t.publish(state, "lint", vec![error()]);
    t.until_shown("the error", &errors, &error_rows);
    let copy = t.copy();
    let state = t.publish(state, "lint", vec![warning()]);
    t.open_copy(copy);
    t.until_shown(
        "the warning in the copy too",
        &[(X, "`x` is unused", Some("lint"))],
        &[file_row(1), row("W", "`x` is unused", "lint", "2:9")],
    );

    // A copy taken before the set was cleared.
    let copy = t.copy();
    let state = t.clear(state, "lint");
    t.until_shown("none after the clear", &[], &[]);
    t.open_copy(copy);
    t.until_shown("none in the copy after the clear", &[], &[]);

    // A copy taken before the plugin unloaded.
    let _state = t.publish(state, "lint", vec![error()]);
    t.until_shown("the error again", &errors, &error_rows);
    let copy = t.copy();
    t.s.cx.cx.update(|cx| unload_plugin(LINT, cx));
    t.s.until("the plugin to unload", |s| running(s) == Some(false));
    t.until_shown("none after the unload", &[], &[]);
    t.open_copy(copy);
    t.until_shown("none in the copy after the unload", &[], &[]);

    // A copy taken before the plugin reloaded.
    t.install(";; again");
    let state = t.slice();
    let _state = t.publish(state, "lint", vec![error()]);
    t.until_shown("the new load's error", &errors, &error_rows);
    let copy = t.copy();
    t.install(";; rebuilt");
    t.until_shown("none after the reload", &[], &[]);
    t.open_copy(copy);
    t.until_shown("none in the copy after the reload", &[], &[]);

    // The next load's still show in every editor.
    let state = t.slice();
    let _state = t.publish(state, "lint", vec![error()]);
    t.until_shown("the next load's error everywhere", &errors, &error_rows);
}
