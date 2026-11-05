// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use gpui::{
    Context, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div,
};

use crate::{
    Icon, IconRef, IconSet, LucideIcons, TreeRow, UiTheme, file_icon, file_icon_path,
    file_icon_ref,
    file_icons::{EXTENSIONS, FILE_NAMES},
    folder_icon, folder_icon_path, folder_icon_ref, icon_set, reset_icon_set, set_icon_set,
};

/// Answers every file with `file` and every folder with `folder`.
struct Stub {
    file: IconRef,
    folder: IconRef,
}

impl IconSet for Stub {
    fn file(&self, _: &str) -> IconRef {
        self.file.clone()
    }

    fn folder(&self, expanded: bool) -> IconRef {
        match expanded {
            true => self.folder.clone(),
            false => Icon::Hash.into(),
        }
    }
}

/// The rows of the file_icons tests, plus every table entry.
fn rows() -> Vec<(String, Icon)> {
    let cases = [
        ("Cargo.toml", Icon::Package),
        ("Cargo.lock", Icon::FileLock),
        ("main.rs", Icon::FileCode),
        ("MAIN.RS", Icon::FileCode),
        ("README.md", Icon::FileText),
        ("data.json", Icon::FileBraces),
        ("archive.tar.gz", Icon::FileArchive),
        (".gitignore", Icon::GitBranch),
        ("Dockerfile", Icon::Container),
        (".env.local", Icon::FileKey),
        ("LICENSE", Icon::Scale),
        ("Makefile", Icon::FileCog),
        ("noext", Icon::File),
        ("trailing.", Icon::File),
        (".hidden", Icon::File),
        (".rs", Icon::File),
    ];
    let long = [
        (format!("{}.rs", "a".repeat(297)), Icon::FileCode),
        ("B".repeat(64), Icon::File),
        ("c".repeat(300), Icon::File),
    ];
    let cases = cases.map(|(name, icon)| (name.to_owned(), icon));
    let names = FILE_NAMES
        .iter()
        .map(|&(name, icon)| (name.to_owned(), icon));
    let extensions = EXTENSIONS
        .iter()
        .map(|&(ext, icon)| (format!("x.{ext}"), icon));
    cases
        .into_iter()
        .chain(long)
        .chain(names)
        .chain(extensions)
        .collect()
}

#[test]
fn default_is_lucide() {
    for (name, icon) in rows() {
        assert_eq!(file_icon(&name), icon, "{name}");
        assert_eq!(file_icon_path(&name), icon.path(), "{name}");
        assert_eq!(icon_set().file(&name), IconRef::Bundled(icon), "{name}");
        assert_eq!(file_icon_ref(&name), icon, "{name}");
        assert_eq!(LucideIcons.file(&name), icon, "{name}");
    }
    assert_eq!(folder_icon(false), Icon::Folder);
    assert_eq!(folder_icon(true), Icon::FolderOpen);
    assert_eq!(folder_icon_path(false), Icon::Folder.path());
    assert_eq!(folder_icon_path(true), Icon::FolderOpen.path());
}

#[gpui::test]
fn stub_replaces(cx: &mut TestAppContext) {
    cx.update(|cx| {
        set_icon_set(
            Arc::new(Stub {
                file: Icon::Diamond.into(),
                folder: Icon::Box.into(),
            }),
            cx,
        )
    });
    for name in ["main.rs", "Cargo.toml", "noext", ".env"] {
        assert_eq!(file_icon(name), Icon::Diamond, "{name}");
        assert_eq!(file_icon_ref(name), Icon::Diamond, "{name}");
        assert_eq!(file_icon_path(name), Icon::Diamond.path(), "{name}");
    }
    assert_eq!(folder_icon(true), Icon::Box);
    assert_eq!(folder_icon(false), Icon::Hash);
    assert_eq!(folder_icon_ref(true), Icon::Box);
    assert_eq!(folder_icon_path(true), Icon::Box.path());

    // An asset path comes through as it is.
    cx.update(|cx| {
        set_icon_set(
            Arc::new(Stub {
                file: IconRef::Asset("stub/file.svg".into()),
                folder: IconRef::Asset("stub/open.svg".into()),
            }),
            cx,
        )
    });
    assert_eq!(
        file_icon_ref("main.rs"),
        IconRef::Asset("stub/file.svg".into())
    );
    assert_eq!(file_icon_path("main.rs"), "stub/file.svg");
    assert_eq!(
        folder_icon_ref(true),
        IconRef::Asset("stub/open.svg".into())
    );
    assert_eq!(folder_icon_path(true), "stub/open.svg");
    assert_eq!(file_icon_ref("main.rs").name(), "stub/file.svg");

    cx.update(reset_icon_set);
    assert_eq!(file_icon("main.rs"), Icon::FileCode);
    assert_eq!(file_icon_ref("main.rs"), Icon::FileCode);
    assert_eq!(file_icon_path("main.rs"), Icon::FileCode.path());
    assert_eq!(folder_icon(true), Icon::FolderOpen);
    assert_eq!(folder_icon_path(false), Icon::Folder.path());
}

/// Draws a file row and a folder row, asking the set each time.
struct Rows;

impl Render for Rows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = UiTheme::default_dark();
        div()
            .size_full()
            .child(TreeRow::new("file", "a.rs", &theme).icon(file_icon_ref("a.rs")))
            .child(
                TreeRow::new("dir", "src", &theme)
                    .directory(true)
                    .icon(folder_icon_ref(true)),
            )
    }
}

/// Installing or resetting a set redraws what's on screen, with nothing
/// else to make it.
#[gpui::test]
fn swapping_redraws(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| Rows);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let drawn = |selector: &'static str, cx: &mut VisualTestContext| {
        cx.run_until_parked();
        cx.debug_bounds(selector).is_some()
    };
    assert!(drawn("tree-icon:a.rs:file-code", cx));
    assert!(drawn("tree-icon:src:folder-open", cx));

    cx.update(|_, cx| {
        set_icon_set(
            Arc::new(Stub {
                file: Icon::Diamond.into(),
                folder: Icon::Box.into(),
            }),
            cx,
        )
    });
    assert!(drawn("tree-icon:a.rs:diamond", cx));
    assert!(drawn("tree-icon:src:box", cx));

    cx.update(|_, cx| {
        set_icon_set(
            Arc::new(Stub {
                file: IconRef::Asset("stub/file.svg".into()),
                folder: IconRef::Asset("stub/open.svg".into()),
            }),
            cx,
        )
    });
    assert!(drawn("tree-icon:a.rs:stub/file.svg", cx));
    assert!(drawn("tree-icon:src:stub/open.svg", cx));

    cx.update(|_, cx| reset_icon_set(cx));
    assert!(drawn("tree-icon:a.rs:file-code", cx));
    assert!(drawn("tree-icon:src:folder-open", cx));
}

#[test]
fn a_set_installed_on_another_thread_is_not_seen() {
    std::thread::spawn(|| {
        let cx = TestAppContext::single();
        cx.update(|cx| {
            set_icon_set(
                Arc::new(Stub {
                    file: Icon::Diamond.into(),
                    folder: Icon::Box.into(),
                }),
                cx,
            )
        });
        assert_eq!(file_icon_ref("main.rs"), Icon::Diamond);
    })
    .join()
    .unwrap();
    assert_eq!(file_icon_ref("main.rs"), Icon::FileCode);
}
