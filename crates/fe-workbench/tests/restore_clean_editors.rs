//! A restored session's clean files reopen clean: every editor a saved
//! layout restores over an unchanged file is clean, stays clean as frames
//! go by, and is clean again in the next launches, which restore the layout
//! as the window saved it; and each shows its file's path with no `..` in
//! it, though the layout saved one with.
//!
//! Some of the layout's tabs name hot exit's backups holding just what
//! their files hold, as a session leaves them that quit with those
//! buffers marked modified: a backup is restored dirty, and a dirty buffer
//! is backed up again at quit, so they came back marked at every launch
//! until saved.

use std::{
    fs::File,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use fe_buffer::{BackupId, BackupMeta, BackupStore, Buffer, UndoStore};
use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand, pane_item};
use fe_text::Text;
use fe_workbench::{Launch, Workbench, window_layouts};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::layout::{self, LayoutStore};

static SERIAL: Mutex<()> = Mutex::new(());

/// Runs the rest of a test with no other test of this binary running, and
/// nothing naming a config directory, so the workbench reads no user
/// settings and watches none.
fn serially() -> MutexGuard<'static, ()> {
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for var in [
        "TENMOKU_CONFIG_DIR",
        "XDG_CONFIG_HOME",
        "HOME",
        "APPDATA",
        "USERPROFILE",
    ] {
        // SAFETY: every test takes the lock before it starts gpui, so no
        // other thread is running to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    guard
}

/// The project's files, as clean on disk as a checkout's, and whether the
/// layout's tab of each names a backup holding the file's text.
const FILES: [(&str, &str, bool); 5] = [
    (
        "cmd/main.go",
        "package main\n\nfunc main() {\n\tprintln(\"hi\")\n}\n",
        false,
    ),
    (
        "e2e/admin.spec.ts",
        "import { test } from 'x';\n\ntest('a', () => {});\n",
        true,
    ),
    (
        "web/tsconfig.json",
        "{\n  \"compilerOptions\": {}\n}\n",
        false,
    ),
    (
        "internal/facets.go",
        "package books\n\n// Facets.\ntype Facets struct{}\n",
        true,
    ),
    // Saved through `..`.
    (
        "docker-compose.yml",
        "services:\n  db:\n    image: postgres\n",
        true,
    ),
];

/// A file whose backup holds unsaved changes: its name, its text on disk
/// and the backup's.
const EDITED: (&str, &str, &str) = ("notes.md", "on disk\n", "unsaved\n");

/// A project holding [`FILES`] and [`EDITED`], beside a folder, `other`, that the last
/// file's saved path goes through, and the state directory its windows'
/// layouts and backups are kept in.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    state: PathBuf,
}

fn project() -> Project {
    let dir = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(dir.path()).unwrap();
    let root = base.join("project");
    std::fs::create_dir(base.join("other")).unwrap();
    for (name, text, _) in FILES {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    std::fs::write(root.join(EDITED.0), EDITED.1).unwrap();
    let state = base.join("state");
    Project {
        _dir: dir,
        root,
        state,
    }
}

impl Project {
    fn layouts(&self) -> LayoutStore {
        window_layouts(self.state.join("layouts"), Some(&self.root))
    }

    /// The store hot exit keeps the root's windows' backups in: beside
    /// their layouts.
    fn backups(&self) -> BackupStore {
        let layouts = self.layouts().file();
        BackupStore::new((self.state.join("backups")).join(layouts.file_stem().unwrap()))
    }

    /// Each file's path as the layout saved it: the last through `..`.
    fn saved_paths(&self) -> Vec<PathBuf> {
        let base = self.root.parent().unwrap();
        (FILES.iter().enumerate())
            .map(|(ix, (name, _, _))| match ix + 1 == FILES.len() {
                true => base.join("other/../project").join(name),
                false => self.root.join(name),
            })
            .collect()
    }

    fn paths(&self) -> Vec<PathBuf> {
        (FILES.iter())
            .map(|(name, _, _)| self.root.join(name))
            .collect()
    }

    /// Saves the layout a session leaves as it quits: one window, its
    /// pane holding the files, each at a caret, the fourth active, the tabs
    /// [`FILES`] says naming a backup each, in the store, of their
    /// unchanged file; and last [`EDITED`], its backup's text unsaved.
    fn save_session(&self) {
        let store = self.backups();
        let edited = self.root.join(EDITED.0);
        let id = BackupId::new();
        let buffer = Buffer::from_file(File::open(&edited).unwrap()).unwrap();
        let meta = BackupMeta::for_buffer(&buffer, Some(edited.clone()), EDITED.0);
        store.write(id, &meta, &Text::new(EDITED.2)).unwrap();
        let edited = serde_json::json!({
            "kind": "fastedit.editor",
            "document": FILES.len(),
            "state": { "path": edited.to_str().unwrap(), "caret": 0, "backup": id.to_string() },
        });
        let items: Vec<serde_json::Value> = (self.saved_paths().iter().enumerate())
            .map(|(ix, path)| {
                let mut state = serde_json::json!({
                    "path": path.to_str().unwrap(),
                    "caret": ix * 3,
                });
                if FILES[ix].2 {
                    let id = BackupId::new();
                    let buffer = Buffer::from_file(File::open(path).unwrap()).unwrap();
                    let meta = BackupMeta::for_buffer(&buffer, Some(path.clone()), FILES[ix].0);
                    store.write(id, &meta, buffer.text()).unwrap();
                    state["backup"] = id.to_string().into();
                }
                serde_json::json!({
                    "kind": "fastedit.editor",
                    "document": ix,
                    "state": state,
                })
            })
            .chain([edited])
            .collect();
        let window = serde_json::json!({
            "breadcrumbs": true,
            "panes": {
                "tree": { "pane": { "items": items, "active": 3 } },
                "active": [],
            },
            "id": 1,
            "saved": 1,
            "session": 1,
        });
        let file = self.layouts().file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let json = serde_json::json!({ "version": 1, "windows": [window] });
        std::fs::write(file, serde_json::to_vec(&json).unwrap()).unwrap();
    }
}

/// The app as fastedit starts it, with vim on and an undo store.
fn init(project: &Project, cx: &mut TestAppContext) {
    let undo = UndoStore::new(project.state.join("undo"));
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
        fe_gpui::set_vim_enabled(cx, true);
        fe_gpui::pane_item::set_undo_store(cx, Some(undo));
    });
}

/// A launch's window on the project's root, as fastedit opens it: keeping
/// its layout and backups in the project's state directory, and restoring
/// the layout saved there; drawn and focused.
fn launch(project: &Project, cx: &mut TestAppContext) -> (Entity<Workbench>, VisualTestContext) {
    let mut windows = layout::launch(&project.layouts(), true);
    assert_eq!(windows.len(), 1);
    let (layout, entry) = windows.remove(0);
    let entry: Value = entry.expect("a layout to restore");
    let launch = Launch {
        root: Some(project.root.clone()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        let mut workbench = Workbench::new(launch, theme, EditorSettings::default(), window, cx);
        workbench.restore_layout(&entry, window, cx);
        workbench.set_layout(layout, window, cx);
        workbench
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(&mut vcx);
    (workbench, vcx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A few frames, a second apart: past hot exit's wait to back up a dirty
/// buffer.
fn frames(cx: &mut VisualTestContext) {
    for _ in 0..4 {
        cx.executor().advance_clock(Duration::from_secs(1));
        draw(cx);
    }
}

/// Quits through `app.quit`, as the app does, asking nothing: the layout
/// is saved, and dirty buffers' backups written.
fn quit(workbench: Entity<Workbench>, mut vcx: VisualTestContext, cx: &mut TestAppContext) {
    // Only the window holds it, so it goes as the app quits.
    drop(workbench);
    let run = RunCommand(
        Invocation::of("app.quit")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    vcx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(&mut vcx);
    assert_eq!(vcx.pending_prompt(), None, "quitting asks nothing");
    cx.quit();
}

/// Each tab's path, and whether it's dirty, by its editor and as the tab
/// shows it; and the last tab's text, [`EDITED`]'s.
fn tabs(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<(PathBuf, bool, bool)>, String) {
    let tabs: Vec<(Entity<EditorView>, bool)> = workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().iter())
            .flat_map(|pane| {
                (pane.read(cx).tabs().iter())
                    .map(|tab| (tab.editor().cloned().unwrap(), tab.item().is_dirty(cx)))
                    .collect::<Vec<_>>()
            })
            .collect()
    });
    let edited = tabs.last().unwrap().0.clone();
    let edited = edited.read_with(cx, |editor, _| editor.model().text().to_string());
    let tabs = (tabs.into_iter())
        .map(|(editor, shown)| {
            editor.read_with(cx, |editor, _| {
                let model = editor.model();
                (model.path().unwrap().to_owned(), model.is_dirty(), shown)
            })
        })
        .collect();
    (tabs, edited)
}

/// The texts of the backups in `store`.
fn backed_up(store: &BackupStore) -> Vec<String> {
    let mut ids: Vec<BackupId> = (store.ids().unwrap().into_iter())
        .map(pane_item::primary)
        .collect();
    ids.dedup();
    (ids.into_iter())
        .map(|id| pane_item::read_newest(store, id).unwrap().text.to_string())
        .collect()
}

/// Restoring a layout of clean files, some naming backups of their
/// unchanged text and one saved through `..`, opens each clean at its path
/// without the `..`, and each stays clean over a few frames, a quit, and
/// the next two launches' restores of the layout as it was saved.
#[gpui::test]
fn restored_clean_files_stay_clean(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
    third: &mut TestAppContext,
) {
    let _serial = serially();
    let project = project();
    // Each clean, and the edited file's tab dirty, with its unsaved text.
    let mut clean: Vec<(PathBuf, bool, bool)> = (project.paths().into_iter())
        .map(|path| (path, false, false))
        .collect();
    clean.push((project.root.join(EDITED.0), true, true));
    let clean = (clean, EDITED.2.to_owned());

    project.save_session();
    init(&project, first);
    let (workbench, mut cx) = launch(&project, first);
    assert_eq!(tabs(&workbench, &mut cx), clean, "as restored");
    frames(&mut cx);
    assert_eq!(tabs(&workbench, &mut cx), clean, "frames later");
    quit(workbench, cx, first);
    assert_eq!(
        backed_up(&project.backups()),
        [EDITED.2],
        "clean buffers keep no backups"
    );

    for (n, app) in [(2, second), (3, third)] {
        init(&project, app);
        let (workbench, mut cx) = launch(&project, app);
        assert_eq!(tabs(&workbench, &mut cx), clean, "launch {n}, as restored");
        frames(&mut cx);
        assert_eq!(tabs(&workbench, &mut cx), clean, "launch {n}, frames later");
        quit(workbench, cx, app);
    }
    for (path, (_, text, _)) in project.paths().iter().zip(FILES) {
        assert_eq!(std::fs::read_to_string(path).unwrap(), text, "never saved");
    }
    let edited = std::fs::read_to_string(project.root.join(EDITED.0)).unwrap();
    assert_eq!(edited, EDITED.1, "never saved");
}
