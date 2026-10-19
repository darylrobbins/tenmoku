// SPDX-License-Identifier: LicenseRef-Proprietary

//! An app's own config source, added with `Config::source`.

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{App, TestAppContext};
use kiln::{
    Config,
    core::{Diagnostic, Severity},
    platform::config::{ConfigDiagnostics, ConfigSource, batch_channel},
};

/// A source reading one file, whose diagnostic is a warning naming how
/// many times it has read it: once made, again when installed, and on
/// each batch naming it, which it records.
struct Counting {
    path: PathBuf,
    reads: usize,
    installed: Rc<RefCell<bool>>,
    reloads: Rc<RefCell<Vec<Vec<PathBuf>>>>,
}

impl Counting {
    fn warning(path: &Path, reads: usize) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            file: path.to_owned(),
            key: None,
            line: reads,
            column: 1,
            message: format!("read {reads} times"),
        }
    }
}

impl ConfigSource for Counting {
    fn paths(&self) -> Vec<PathBuf> {
        vec![self.path.clone()]
    }

    fn install(&mut self, _cx: &mut App) {
        self.reads += 1;
        *self.installed.borrow_mut() = true;
    }

    fn reload(&mut self, changed: &[PathBuf], _cx: &mut App) {
        if changed.contains(&self.path) {
            self.reads += 1;
            self.reloads.borrow_mut().push(changed.to_vec());
        }
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        vec![Self::warning(&self.path, self.reads)]
    }
}

#[gpui::test]
fn config_source_reaches_diagnostics(cx: &mut TestAppContext) {
    let path = PathBuf::from("/nowhere/app.rc");
    let installed = Rc::new(RefCell::new(false));
    let reloads = Rc::new(RefCell::new(Vec::new()));
    let source = Counting {
        path: path.clone(),
        reads: 1,
        installed: installed.clone(),
        reloads: reloads.clone(),
    };
    let (sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .source(source)
        .watch_with(batches);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert!(*installed.borrow(), "installed when the service started");
    assert_eq!(
        diagnostics,
        [Counting::warning(&path, 2)],
        "returned by init as installed"
    );
    let published =
        |cx: &mut TestAppContext| cx.update(|cx| cx.global::<ConfigDiagnostics>().0.clone());
    assert_eq!(published(cx), [Counting::warning(&path, 2)]);

    // A batch naming only another file reaches it, but it reads nothing.
    sender.send(vec![PathBuf::from("/nowhere/other.toml")]);
    cx.run_until_parked();
    assert_eq!(*reloads.borrow(), Vec::<Vec<PathBuf>>::new());
    assert_eq!(published(cx), [Counting::warning(&path, 2)]);

    sender.send(vec![path.clone()]);
    cx.run_until_parked();
    assert_eq!(*reloads.borrow(), [vec![path.clone()]]);
    assert_eq!(published(cx), [Counting::warning(&path, 3)]);
}
