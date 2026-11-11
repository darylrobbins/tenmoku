//! The workbench's commands a plugin may invoke, run through the bridge's
//! [`Served`] as a plugin's invocation is, against a policy granting what
//! each test says: each runs with its grants and is refused without them,
//! `workbench.open_path` stays inside the workspace, `editor.save` saves
//! only a file inside it, and no plugin runs a plugin's command.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use fe_buffer::{Buffer, Origin};
use fe_command::{
    Actor, Capability, DispatchError, Grants, Invocation, PluginDenied, PluginPolicy, Source,
};
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use serde_json::{Value, json};
use std::{cell::Cell, rc::Rc};

use crate::{Launch, Workbench, plugin_bridge::Served};

/// The invoking plugin.
const PLUGIN: &str = "acme.tool";

/// Another plugin, loaded beside it.
const OTHER: &str = "acme.other";

/// What [`PLUGIN`] is granted, which a test changes as it goes, and the
/// workspace.
struct Policy {
    grants: Mutex<Grants>,
    roots: Vec<PathBuf>,
}

impl Policy {
    fn grant(&self, capabilities: &[&str]) {
        *self.grants.lock().unwrap() = (capabilities.iter())
            .map(|capability| Capability::parse(capability).unwrap())
            .collect();
    }
}

impl PluginPolicy for Policy {
    fn grants(&self, plugin: &str) -> Option<Grants> {
        match plugin {
            PLUGIN => Some(self.grants.lock().unwrap().clone()),
            OTHER => Some(Grants::default()),
            _ => None,
        }
    }

    fn workspace_roots(&self) -> Vec<PathBuf> {
        self.roots.clone()
    }
}

struct Setup<'a> {
    /// The workspace root, with `a.txt` and `b.txt` in it.
    root: tempfile::TempDir,
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
    policy: Arc<Policy>,
    served: Served,
}

const A: &str = "alpha\nbeta\n";
const B: &str = "one\ntwo\nthree\n";

/// A window on a root with `a.txt` showing, active and drawn, and plugins'
/// invocations served against a [`Policy`] granting nothing yet.
fn setup(cx: &mut TestAppContext) -> Setup<'_> {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a.txt");
    fs::write(&a, A).unwrap();
    fs::write(root.path().join("b.txt"), B).unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.path().to_owned()),
        file: Some((a, Some(Buffer::new(A).into()))),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        let editor = workbench.read(cx).active_editor(cx).unwrap().clone();
        window.focus(&editor.focus_handle(cx), cx);
    });
    draw(cx);
    let policy = Arc::new(Policy {
        grants: Mutex::default(),
        roots: vec![root.path().to_owned()],
    });
    let served = cx.update(|_, cx| Served::new(policy.clone(), cx));
    Setup {
        root,
        workbench,
        cx,
        policy,
        served,
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

impl Setup<'_> {
    /// Runs [`PLUGIN`]'s invocation of `command` with `args`, as the bridge
    /// does, then lets what it started finish.
    fn invoke(&mut self, command: &str, args: Value) -> Result<(), DispatchError> {
        self.invoke_as(PLUGIN, command, args)
    }

    fn invoke_as(&mut self, plugin: &str, command: &str, args: Value) -> Result<(), DispatchError> {
        let invocation = Invocation::of(command)
            .unwrap()
            .args(args)
            .actor(Actor::Host(plugin.into()))
            .source(Source::Script);
        let served = &self.served;
        let result = self.cx.cx.update(|cx| served.run(&invocation, cx));
        draw(self.cx);
        result
    }

    /// The commands the workbench ran for [`PLUGIN`], in order.
    fn ran(&mut self) -> Vec<String> {
        self.workbench.read_with(self.cx, |workbench, _| {
            (workbench.commands_run.iter())
                .filter(|run| run.actor == Actor::Host(PLUGIN.into()))
                .map(|run| run.command.to_string())
                .collect()
        })
    }

    fn active_editor(&mut self) -> Entity<EditorView> {
        self.workbench
            .read_with(self.cx, |workbench, cx| {
                workbench.active_editor(cx).cloned()
            })
            .expect("an editor is active")
    }

    fn active_path(&mut self) -> Option<PathBuf> {
        let editor = self.active_editor();
        editor.read_with(self.cx, |editor, _| {
            editor.model().path().map(Path::to_owned)
        })
    }

    /// Types `text` at the start of the active editor's buffer, unsaved.
    fn edit(&mut self, text: &str) {
        let editor = self.active_editor();
        editor.update(self.cx, |editor, cx| {
            editor
                .model_mut()
                .buffer_mut()
                .edit([(0..0, text)], Origin::User);
            cx.notify();
        });
    }

    /// Shows an editor of `model` in a new tab of the active pane, focused.
    fn add_tab(&mut self, model: EditorModel) {
        let pane = self
            .workbench
            .read_with(self.cx, |workbench, _| workbench.pane().clone());
        self.cx.update(|window, cx| {
            pane.update(cx, |pane, cx| {
                pane.add(model, cx);
                pane.activate(pane.tabs().len() - 1, window, cx);
            })
        });
        let editor = self.active_editor();
        self.cx
            .update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
        draw(self.cx);
    }
}

/// The capability a refusal names as missing.
fn missing(result: Result<(), DispatchError>) -> Capability {
    match result {
        Err(DispatchError::PluginDenied(PluginDenied::MissingCapability {
            capability, ..
        })) => capability,
        other => panic!("expected a denial for a capability, got {other:?}"),
    }
}

#[gpui::test]
fn each_new_command_runs_with_its_grant_and_is_refused_without(cx: &mut TestAppContext) {
    let mut setup = setup(cx);
    let a = setup.root.path().join("a.txt");
    let b = setup.root.path().join("b.txt");
    // Each command, its args, and what it needs beyond `commands`.
    let rows: [(&str, Value, &[&str]); 8] = [
        (
            "editor.save",
            Value::Null,
            &["buffer.write", "fs.write:workspace"],
        ),
        ("nav.back", Value::Null, &[]),
        ("nav.forward", Value::Null, &[]),
        ("diagnostics.next", Value::Null, &[]),
        ("diagnostics.previous", Value::Null, &[]),
        ("project_panel.reveal_active", Value::Null, &[]),
        ("problems.show", Value::Null, &[]),
        // Last, as it leaves `b.txt` showing.
        (
            "workbench.open_path",
            json!({ "path": b.display().to_string() }),
            &["fs.read:workspace"],
        ),
    ];
    for (command, args, needs) in rows {
        let saves = command == "editor.save";
        if saves {
            setup.edit("saved ");
        }
        let ran = setup.ran().len();
        let on_disk = fs::read_to_string(&a).unwrap();
        let refusals: Vec<(Vec<&str>, Capability)> = if needs.is_empty() {
            vec![(Vec::new(), Capability::Commands)]
        } else {
            (needs.iter())
                .map(|need| {
                    let others = needs.iter().filter(|other| *other != need);
                    let grants = std::iter::once(&"commands").chain(others).copied();
                    (grants.collect(), Capability::parse(need).unwrap())
                })
                .collect()
        };
        for (grants, needed) in refusals {
            setup.policy.grant(&grants);
            let refused = setup.invoke(command, args.clone());
            assert_eq!(missing(refused), needed, "{command} with {grants:?}");
            assert_eq!(setup.ran().len(), ran, "{command} with {grants:?} ran");
        }
        assert_eq!(fs::read_to_string(&a).unwrap(), on_disk, "{command}");
        assert_eq!(
            setup.active_path().as_deref(),
            Some(a.as_path()),
            "{command}"
        );

        let granted: Vec<&str> = std::iter::once("commands")
            .chain(needs.iter().copied())
            .collect();
        setup.policy.grant(&granted);
        setup
            .invoke(command, args)
            .unwrap_or_else(|error| panic!("{command}: {error}"));
        if saves {
            assert_eq!(fs::read_to_string(&a).unwrap(), format!("saved {A}"));
        } else {
            assert_eq!(setup.ran()[ran..], [command.to_owned()], "{command}");
        }
        if command == "workbench.open_path" {
            assert_eq!(setup.active_path().as_deref(), Some(b.as_path()));
        }
    }
}

#[gpui::test]
fn open_path_stays_inside_the_workspace(cx: &mut TestAppContext) {
    let mut setup = setup(cx);
    setup.policy.grant(&["commands", "fs.read:workspace"]);
    let b = setup.root.path().join("b.txt");

    // Row 1, column 2: `two`'s `o`.
    let at = json!({ "path": b.display().to_string(), "row": 1, "column": 2 });
    setup.invoke("workbench.open_path", at).unwrap();
    assert_eq!(setup.active_path().as_deref(), Some(b.as_path()));
    let editor = setup.active_editor();
    let caret = editor.read_with(setup.cx, |editor, _| {
        let model = editor.model();
        let head = model.primary_selection().head();
        let point = model.snapshot().text().offset_to_point(head);
        (point.row, point.column)
    });
    assert_eq!(caret, (1, 2));

    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.txt");
    fs::write(&secret, "secret\n").unwrap();
    let link = setup.root.path().join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(outside.path(), &link).unwrap();
    let escape = (setup.root.path().join(".."))
        .join(
            outside
                .path()
                .strip_prefix(setup.root.path().parent().unwrap())
                .unwrap(),
        )
        .join("secret.txt");
    for path in [secret.clone(), escape, link.join("secret.txt")] {
        let args = json!({ "path": path.display().to_string(), "row": 0 });
        let refused = setup.invoke("workbench.open_path", args);
        assert!(
            matches!(
                refused,
                Err(DispatchError::PluginDenied(
                    PluginDenied::OutsideWorkspace { arg: "path", .. }
                ))
            ),
            "{path:?}: {refused:?}"
        );
        assert_eq!(
            setup.active_path().as_deref(),
            Some(b.as_path()),
            "{path:?}"
        );
    }
    let opened = setup.workbench.read_with(setup.cx, |workbench, cx| {
        workbench.pane().read(cx).position(&secret).is_some()
    });
    assert!(!opened);
}

/// A symlink `link` to `target`, a file (not there yet, or itself a link).
fn symlink_file(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(target, link).unwrap();
}

#[gpui::test]
fn save_refuses_an_untitled_or_outside_buffer(cx: &mut TestAppContext) {
    let mut setup = setup(cx);
    setup
        .policy
        .grant(&["commands", "buffer.write", "fs.write:workspace"]);

    let outside = tempfile::tempdir().unwrap();
    let path = outside.path().join("outside.txt");
    fs::write(&path, "kept\n").unwrap();
    let mut model = EditorModel::open(&path).unwrap();
    model.buffer_mut().edit([(0..0, "changed ")], Origin::User);
    setup.add_tab(model);
    assert_eq!(setup.active_path().as_deref(), Some(path.as_path()));
    let refused = setup.invoke("editor.save", Value::Null);
    assert!(
        matches!(
            refused,
            Err(DispatchError::PluginDenied(
                PluginDenied::OutsideWorkspace { .. }
            ))
        ),
        "{refused:?}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "kept\n");

    // A symlink in the workspace to a file outside it not there yet, which
    // a save would create, directly and through a second link.
    {
        let dangling = setup.root.path().join("dangling.txt");
        let missing = outside.path().join("missing.txt");
        symlink_file(&missing, &dangling);
        let chained = setup.root.path().join("chained.txt");
        let hop = setup.root.path().join("hop.txt");
        let missing_too = outside.path().join("missing_too.txt");
        symlink_file(&missing_too, &hop);
        symlink_file(&hop, &chained);
        for (link, target) in [(dangling, missing), (chained, missing_too)] {
            let mut model = EditorModel::new(Buffer::new(""), Some(link.clone()));
            model.buffer_mut().edit([(0..0, "plugin")], Origin::User);
            setup.add_tab(model);
            let refused = setup.invoke("editor.save", Value::Null);
            assert!(
                matches!(
                    refused,
                    Err(DispatchError::PluginDenied(
                        PluginDenied::OutsideWorkspace { .. }
                    ))
                ),
                "{link:?}: {refused:?}"
            );
            assert!(!target.exists(), "{target:?}");
        }
    }

    let mut untitled = EditorModel::new(Buffer::new(""), None);
    untitled
        .buffer_mut()
        .edit([(0..0, "untitled")], Origin::User);
    setup.add_tab(untitled);
    assert_eq!(setup.active_path(), None);
    let before: Vec<_> = fs::read_dir(setup.root.path()).unwrap().collect();
    let refused = setup.invoke("editor.save", Value::Null);
    assert!(
        matches!(
            refused,
            Err(DispatchError::PluginDenied(
                PluginDenied::MissingPath { .. }
            ))
        ),
        "{refused:?}"
    );
    let after: Vec<_> = fs::read_dir(setup.root.path()).unwrap().collect();
    assert_eq!(after.len(), before.len());
    let editor = setup.active_editor();
    assert!(editor.read_with(setup.cx, |editor, _| editor.model().is_dirty()));
    // Saving it would have asked where, as Save As.
    let asked = setup.workbench.read_with(setup.cx, |workbench, _| {
        (workbench.commands_run.iter()).any(|run| run.command.as_str() == "workbench.save_as")
    });
    assert!(!asked);
    // Nothing was asked either: the window has no prompt.
    assert!(!setup.cx.has_pending_prompt());
}

#[gpui::test]
fn a_plugin_cannot_invoke_another_plugins_command(cx: &mut TestAppContext) {
    let mut setup = setup(cx);
    setup.policy.grant(&[
        "commands",
        "buffer.read",
        "buffer.write",
        "fs.read:workspace",
        "fs.write:workspace",
    ]);
    for (plugin, command) in [
        (PLUGIN, format!("{OTHER}.run")),
        (PLUGIN, format!("{PLUGIN}.run")),
        (PLUGIN, "workbench.close_tab".to_owned()),
    ] {
        let refused = setup.invoke_as(plugin, &command, Value::Null);
        assert!(
            matches!(
                refused,
                Err(DispatchError::PluginDenied(PluginDenied::Undeclared { .. }))
            ),
            "{command}: {refused:?}"
        );
    }
    assert_eq!(setup.ran(), Vec::<String>::new());
}

/// A save hook that holds the save it runs before until the test ends, and
/// counts its runs.
struct HoldSaves(Rc<Cell<usize>>);

impl fe_gpui::pane_item::SaveHook for HoldSaves {
    fn before_save(&self, _: &Entity<EditorView>, cx: &mut gpui::App) -> gpui::Task<()> {
        self.0.set(self.0.get() + 1);
        gpui::AppContext::background_spawn(cx, std::future::pending())
    }
}

#[gpui::test]
fn a_plugin_save_takes_the_path_it_was_checked_for(cx: &mut TestAppContext) {
    let mut setup = setup(cx);
    setup
        .policy
        .grant(&["commands", "buffer.write", "fs.write:workspace"]);
    let a = setup.root.path().join("a.txt");
    let editor = setup.active_editor();
    let runs = Rc::new(Cell::new(0));
    let hook: Rc<dyn fe_gpui::pane_item::SaveHook> = Rc::new(HoldSaves(runs.clone()));

    // With a save hook (format on save), the plugin's save writes now,
    // without running it, and the hook is put back. (A frame drawn sets
    // the hooks the settings want, and the test platform draws after each
    // update, so the hook is set and read in one.)
    setup.edit("plugin ");
    let save = Invocation::of("editor.save")
        .unwrap()
        .actor(Actor::Host(PLUGIN.into()))
        .source(Source::Script);
    let served = &setup.served;
    let kept = setup.cx.cx.update(|cx| {
        editor.update(cx, |editor, _| editor.set_save_hook(Some(hook.clone())));
        served.run(&save, cx).unwrap();
        editor.read(cx).save_hook().cloned()
    });
    assert_eq!(runs.get(), 0);
    assert!(kept.is_some_and(|kept| Rc::ptr_eq(&kept, &hook)));
    setup.cx.run_until_parked();
    assert_eq!(fs::read_to_string(&a).unwrap(), format!("plugin {A}"));

    // A Save As to outside the workspace, queued behind a save the hook
    // holds: a plugin's save queued behind them would write wherever the
    // Save As moves the buffer, so it's refused, and nothing is written.
    let outside = tempfile::tempdir().unwrap();
    let moved = outside.path().join("moved.txt");
    setup.edit("user ");
    let saving = setup.cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_save_hook(Some(hook.clone()));
            let held = editor.save(window, cx);
            (held, editor.save_as(moved.clone(), window, cx))
        })
    });
    draw(setup.cx);
    assert_eq!(runs.get(), 1);
    let refused = setup.invoke("editor.save", Value::Null);
    assert!(
        matches!(&refused, Err(DispatchError::Failed { error, .. }) if error.0.contains("under way")),
        "{refused:?}"
    );
    assert_eq!(fs::read_to_string(&a).unwrap(), format!("plugin {A}"));
    assert!(!moved.exists());
    drop(saving);
}

#[gpui::test]
fn a_forwarded_command_runs_with_no_focus_or_an_undrawn_one(cx: &mut TestAppContext) {
    let mut setup = setup(cx);
    setup.policy.grant(&["commands"]);
    let undrawn = setup.cx.update(|_, cx| cx.focus_handle());
    for (case, focus) in [("no focus", None), ("an undrawn focus", Some(&undrawn))] {
        setup.cx.update(|window, cx| match focus {
            Some(focus) => window.focus(focus, cx),
            None => window.blur(cx),
        });
        let ran = setup.ran().len();
        setup
            .invoke("problems.show", Value::Null)
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        assert_eq!(setup.ran()[ran..], ["problems.show".to_owned()], "{case}");
    }
}
