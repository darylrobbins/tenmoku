//! Paths macOS hands the app as open-document events (Finder's Open With,
//! `open -a Tenmoku <path>`, a drop on the Dock icon) go to windows: a
//! folder to a window on it, a file to the window whose root contains it,
//! else to a new window.

use std::{fs, path::PathBuf};

use fe_gpui::{EditorSettings, EditorTheme};
use fe_workbench::{DocumentRoute, Launch, Workbench, open_documents, path_of_url, route_document};
use gpui::{TestAppContext, WindowHandle};

fn window(cx: &mut TestAppContext, root: &std::path::Path) -> WindowHandle<Workbench> {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    })
}

#[test]
fn urls_become_paths() {
    assert_eq!(
        path_of_url("file:///Users/me/a%20b/c.rs"),
        Some(PathBuf::from("/Users/me/a b/c.rs"))
    );
    assert_eq!(
        path_of_url("file:///tmp/dir/"),
        Some(PathBuf::from("/tmp/dir/"))
    );
    assert_eq!(path_of_url("https://example.com/x"), None);
}

#[test]
fn routes_by_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let outer = root.join("outer");
    let inner = outer.join("inner");
    let other = root.join("other");
    for d in [&inner, &other] {
        fs::create_dir_all(d).unwrap();
    }
    let file = inner.join("a.txt");
    fs::write(&file, "x").unwrap();
    let roots = [Some(outer.clone()), Some(inner.clone()), None];

    // C1: a folder reuses a live window on it, else opens one.
    assert_eq!(
        route_document(&outer, &roots),
        DocumentRoute::Window {
            window: 0,
            file: None
        }
    );
    assert_eq!(
        route_document(&other, &roots),
        DocumentRoute::NewWindow {
            root: other.clone(),
            file: None
        }
    );
    // C2: a file goes to the deepest window containing it, else a new
    // window on its parent.
    assert_eq!(
        route_document(&file, &roots),
        DocumentRoute::Window {
            window: 1,
            file: Some(file.clone())
        }
    );
    let loose = other.join("b.txt");
    fs::write(&loose, "y").unwrap();
    assert_eq!(
        route_document(&loose, &roots),
        DocumentRoute::NewWindow {
            root: other.clone(),
            file: Some(loose.clone())
        }
    );
    assert_eq!(
        route_document(&loose, &[]),
        DocumentRoute::NewWindow {
            root: other,
            file: Some(loose)
        }
    );
}

#[gpui::test]
fn opens_in_live_windows_or_asks_for_one(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let project = root.join("project");
    let elsewhere = root.join("elsewhere");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&elsewhere).unwrap();
    let inside = project.join("a.txt");
    let outside = elsewhere.join("b.txt");
    fs::write(&inside, "a").unwrap();
    fs::write(&outside, "b").unwrap();
    cx.executor().allow_parking();
    window(cx, &project);
    cx.run_until_parked();

    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = asked.clone();
    cx.update(|cx| {
        open_documents(
            &[
                project.clone(),
                inside.clone(),
                outside.clone(),
                elsewhere.clone(),
            ],
            cx,
            move |root, file, _| log.borrow_mut().push((root, file)),
        )
    });
    cx.run_until_parked();
    // The folder and the inside file reuse the live window; the other two
    // want windows (no live one is on either yet, the fake opens none).
    assert_eq!(
        *asked.borrow(),
        [(elsewhere.clone(), Some(outside)), (elsewhere, None),]
    );
    assert_eq!(cx.windows().len(), 1);
}
