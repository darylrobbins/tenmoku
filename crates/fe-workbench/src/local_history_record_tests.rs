//! Saves and changes found on disk record the file's text in local history.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use fe_buffer::{LocalHistory, local_history::Reason};
use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use fe_project::FsChange;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench};

struct Fixture<'a> {
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
    history: LocalHistory,
    path: PathBuf,
    _root: tempfile::TempDir,
    _store: tempfile::TempDir,
}

fn fixture(cx: &mut TestAppContext, install: bool) -> Fixture<'_> {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let history = LocalHistory::new(store.path().join("history"));
    let path = dunce::canonicalize(root.path()).unwrap().join("a.txt");
    fs::write(&path, "one\n").unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        if install {
            fe_gpui::pane_item::set_local_history(cx, Some(history.clone()));
        }
    });
    let launch = Launch {
        root: Some(root.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    Fixture {
        workbench,
        cx,
        history,
        path,
        _root: root,
        _store: store,
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

impl Fixture<'_> {
    fn open(&mut self) -> Entity<EditorView> {
        let pane = self.workbench.read_with(self.cx, |w, _| w.pane().clone());
        let path = self.path.clone();
        self.cx
            .update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(self.cx);
        self.workbench
            .read_with(self.cx, |w, cx| w.active_editor(cx).cloned())
            .expect("open")
    }

    fn type_text(&mut self, editor: &Entity<EditorView>, text: &str) {
        editor.update(self.cx, |editor, cx| {
            editor.model_mut().insert(text);
            cx.notify();
        });
    }

    fn save(&mut self, editor: &Entity<EditorView>) {
        let editor = editor.clone();
        self.cx.update(|window, cx| {
            editor
                .update(cx, |editor, cx| editor.save(window, cx))
                .detach();
        });
        draw(self.cx);
    }

    fn disk_change(&mut self, text: &str) {
        fs::write(&self.path, text).unwrap();
        let file = fs::File::options().write(true).open(&self.path).unwrap();
        file.set_modified(SystemTime::now() + Duration::from_secs(10))
            .unwrap();
        let panel = self.workbench.read_with(self.cx, |w, _| w.panel().clone());
        let changes = vec![FsChange::Path(self.path.clone())];
        panel.update(self.cx, |panel, cx| panel.apply_fs_changes(&changes, cx));
        draw(self.cx);
    }

    /// The recorded (reason, text) pairs, newest first.
    fn recorded(&self) -> Vec<(Reason, String)> {
        let entries = self.history.entries(&self.path).unwrap();
        entries
            .iter()
            .map(|entry| {
                let text = self.history.read(&self.path, entry.id).unwrap();
                (entry.reason, String::from_utf8(text).unwrap())
            })
            .collect()
    }
}

#[gpui::test]
fn a_save_records_the_saved_text(cx: &mut TestAppContext) {
    let mut f = fixture(cx, true);
    let editor = f.open();
    f.type_text(&editor, "two ");
    f.save(&editor);
    assert_eq!(f.recorded(), [(Reason::Saved, "two one\n".to_owned())]);
}

#[gpui::test]
fn saving_unchanged_text_records_once(cx: &mut TestAppContext) {
    let mut f = fixture(cx, true);
    let editor = f.open();
    f.type_text(&editor, "two ");
    f.save(&editor);
    f.save(&editor);
    assert_eq!(f.recorded().len(), 1);
}

#[gpui::test]
fn an_external_change_records_the_disk_text(cx: &mut TestAppContext) {
    let mut f = fixture(cx, true);
    let editor = f.open();
    f.disk_change("three\n");
    assert_eq!(
        editor.read_with(f.cx, |e, _| e.model().text().to_string()),
        "three\n"
    );
    assert_eq!(f.recorded(), [(Reason::External, "three\n".to_owned())]);
}

#[gpui::test]
fn a_dirty_buffer_records_the_disk_text_before_asking(cx: &mut TestAppContext) {
    let mut f = fixture(cx, true);
    let editor = f.open();
    f.type_text(&editor, "mine ");
    f.disk_change("theirs\n");
    assert!(f.cx.pending_prompt().is_some(), "asked");
    assert_eq!(f.recorded(), [(Reason::External, "theirs\n".to_owned())]);
}

#[gpui::test]
fn no_store_no_recording(cx: &mut TestAppContext) {
    let mut f = fixture(cx, false);
    let editor = f.open();
    f.type_text(&editor, "two ");
    f.save(&editor);
    assert_eq!(fs::read_to_string(&f.path).unwrap(), "two one\n");
    f.disk_change("three\n");
    assert!(!Path::new(f.history.dir()).exists());
}
