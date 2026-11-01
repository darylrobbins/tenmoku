//! A restored session with the Git Log shown lists the repository of its
//! active file, which is read in the background after the first draw,
//! not the root's, and in a window with no root not "Not in a git
//! repository".

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_workbench::{Launch, Workbench, loader};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

static SERIAL: Mutex<()> = Mutex::new(());

/// Runs the rest of a test with no other test of this binary running, and
/// no config directory named (see the hot exit tests).
fn serially() -> MutexGuard<'static, ()> {
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for var in ["TENMOKU_CONFIG_DIR", "XDG_CONFIG_HOME", "HOME"] {
        // SAFETY: every test takes the lock before it starts gpui, so no
        // other thread is running to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    guard
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository(dir: &Path, file: &str, summary: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q"]);
    fs::write(dir.join(file), "text\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", summary]);
    dir.join(file)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn window(
    root: Option<&Path>,
    entry: Option<&Value>,
    file: Option<&Path>,
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, VisualTestContext) {
    // The project watcher's thread wakes the scheduler.
    cx.executor().allow_parking();
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: root.map(Path::to_owned),
        file: file.map(|path| {
            (
                path.to_owned(),
                Some(loader::open(path.to_owned()).unwrap()),
            )
        }),
    };
    let entry = entry.cloned();
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        let mut workbench = Workbench::new(launch, theme, EditorSettings::default(), window, cx);
        if let Some(entry) = &entry {
            workbench.restore_layout(entry, window, cx);
        }
        workbench
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    // Drawn at once: the restored file is still being read.
    vcx.update(|window, cx| window.draw(cx).clear(cx));
    (workbench, vcx)
}

/// The saved layout of a window on `root` with `file` open and the Git
/// Log shown.
fn saved(root: Option<&Path>, file: &Path, cx: &mut TestAppContext) -> Value {
    let (workbench, mut vcx) = window(root, None, Some(file), cx);
    let cx = &mut vcx;
    run("git.log", cx);
    wait(&workbench, cx, |_, _| true);
    workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx))
}

/// Draws until `done` holds of the Git Log's listing or a few seconds pass.
fn wait(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
    done: impl Fn(&[String], Option<&str>) -> bool,
) -> (Vec<String>, Option<&'static str>) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        draw(cx);
        let listing = workbench
            .read_with(cx, |workbench, cx| workbench.git_log_listing(cx))
            .expect("the dock has its Git Log");
        let ready = !listing.0.is_empty() || listing.1 == Some("Not in a git repository");
        if (ready && done(&listing.0, listing.1)) || Instant::now() > until {
            return listing;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[gpui::test]
fn a_restored_session_lists_its_active_files_repository(cx: &mut TestAppContext) {
    let _serial = serially();
    let mut app = cx.clone();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    repository(&root, "r.rs", "root commit");
    let file = repository(&root.join("inner"), "i.rs", "inner commit");
    let entry = saved(Some(&root), &file, cx);

    let (workbench, mut vcx) = window(Some(&root), Some(&entry), None, &mut app);
    let (summaries, message) = wait(&workbench, &mut vcx, |_, _| true);
    assert_eq!(message, None);
    assert!(
        summaries.iter().any(|text| text.contains("inner commit")),
        "{summaries:?}"
    );
    assert!(summaries.iter().all(|text| !text.contains("root commit")));
}

#[gpui::test]
fn a_rootless_restored_session_never_says_not_a_repository(cx: &mut TestAppContext) {
    let _serial = serially();
    let mut app = cx.clone();
    let dir = tempfile::tempdir().unwrap();
    let file = repository(&dir.path().join("lone"), "l.rs", "lone commit");
    let entry = saved(None, &file, cx);

    let (workbench, mut vcx) = window(None, Some(&entry), None, &mut app);
    let (summaries, message) = wait(&workbench, &mut vcx, |_, _| true);
    assert_eq!(message, None);
    assert!(
        summaries.iter().any(|text| text.contains("lone commit")),
        "{summaries:?}"
    );
}

#[gpui::test]
fn an_unresolved_active_file_ends_at_not_a_repository(cx: &mut TestAppContext) {
    let _serial = serially();
    let mut app = cx.clone();
    let dir = tempfile::tempdir().unwrap();
    // A layout with the Git Log shown whose file is gone by the restore.
    let file = dir.path().join("gone.rs");
    fs::write(&file, "text\n").unwrap();
    let entry = saved(None, &file, cx);
    fs::remove_file(&file).unwrap();

    let (workbench, mut vcx) = window(None, Some(&entry), None, &mut app);
    let (summaries, message) = wait(&workbench, &mut vcx, |_, _| true);
    assert!(summaries.is_empty());
    assert_eq!(message, Some("Not in a git repository"));
}
