//! Binary files in the workbench: they open in a hex viewer from the tree,
//! quick open and `Pane::open`, large ones too; text, images and BOM files
//! open as before; Open as Hex and Open as Text swap a tab in place; the
//! hex tab has no minimap; and a saved layout reopens it.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::settings::SettingsStore;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, HexViewer, commands::RunCommand};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext, px, size};

use crate::{Launch, Pane, Workbench, loader, workbench::toast::Level};

/// What a tab shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shows {
    Hex,
    Editor,
    Image,
    /// The text viewer of a file too large to edit.
    Viewer,
}

/// A project of `files` (name, contents), its root canonical.
fn project(files: &[(&str, &[u8])]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    for (name, bytes) in files {
        fs::write(root.join(name), bytes).unwrap();
    }
    (dir, root)
}

/// Bytes with a NUL early on: binary.
fn binary(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 7) as u8).collect()
}

/// A valid 1 × 1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x38, 0x21, 0x27, 0xf7,
    0x1f, 0x00, 0x04, 0xba, 0x02, 0x04, 0x3e, 0x57, 0x1f, 0xc7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A workbench on `root` in a 1400 × 600 window, active and focused.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        let (layer, diagnostics) = fe_keymap::KeymapLayer::parse_for(
            Path::new(fe_keymap::DEFAULT_KEYMAP_PATH),
            fe_keymap::DEFAULT,
            None,
            kiln::core::Os::current(),
        );
        assert_eq!(diagnostics, []);
        // The built-in keymap as the app runs it on this host.
        fe_gpui::init_with_keymap(cx, fe_keymap::Keymap::from_layers(vec![layer.unwrap()]));
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.simulate_resize(size(px(1400.), px(600.)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(cx);
    (workbench, cx)
}

fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

/// Each tab of `pane`: its file's name and what shows it.
fn tabs(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Vec<(String, Shows)> {
    pane.read_with(cx, |pane, _| {
        (pane.tabs().iter())
            .map(|tab| {
                let shows = match () {
                    _ if tab.hex_viewer().is_some() => Shows::Hex,
                    _ if tab.editor().is_some() => Shows::Editor,
                    _ if tab.image_viewer().is_some() => Shows::Image,
                    _ if tab.viewer().is_some() => Shows::Viewer,
                    _ => panic!("{:?} shows something else", tab.path),
                };
                let path = tab.path.as_ref().expect("a file's tab");
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                (name, shows)
            })
            .collect()
    })
}

fn open_path(pane: &Entity<Pane>, path: PathBuf, cx: &mut VisualTestContext) {
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

fn hex_viewer(pane: &Entity<Pane>, ix: usize, cx: &mut VisualTestContext) -> Entity<HexViewer> {
    pane.read_with(cx, |pane, _| pane.tabs()[ix].hex_viewer().cloned())
        .expect("a hex viewer")
}

/// Activates tab `ix` of `pane`, focused.
fn activate(pane: &Entity<Pane>, ix: usize, cx: &mut VisualTestContext) {
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(ix, window, cx)));
    draw(cx);
}

/// Runs `command` as a key would in `frame` within the workbench.
fn run_in(command: &str, frame: &str, cx: &mut VisualTestContext) {
    let context: ContextStack = ["workbench", frame]
        .into_iter()
        .map(ContextFrame::new)
        .collect();
    let run = RunCommand(Invocation::of(command).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// The shown toasts' levels and messages, emptied.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(Level, String)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    let shown = layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| (entry.toast.level, entry.toast.message.clone()))
            .collect()
    });
    cx.update(|_, cx| layer.update(cx, |layer, cx| layer.dismiss_all(cx)));
    shown
}

fn click_row(workbench: &Entity<Workbench>, name: &str, cx: &mut VisualTestContext) {
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    let ix = panel.read_with(cx, |panel, _| {
        let tree = panel.tree().expect("the root is open");
        (tree.rows(0..tree.row_count()))
            .position(|row| row.name.to_string_lossy() == name)
            .unwrap_or_else(|| panic!("no row {name}"))
    });
    let bounds = panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

#[gpui::test]
fn a_binary_file_opens_in_hex(cx: &mut TestAppContext) {
    // A NUL anywhere in the first 8 KiB, but not past it.
    let mut late = vec![b'a'; loader::BINARY_PROBE + 10];
    late[loader::BINARY_PROBE - 1] = 0;
    let mut too_late = vec![b'a'; loader::BINARY_PROBE + 10];
    too_late[loader::BINARY_PROBE] = 0;
    let (_dir, root) = project(&[
        ("tree.bin", &binary(100)),
        ("quick.dat", &binary(300)),
        ("late.txt", &late),
        ("too_late.txt", &too_late),
    ]);
    assert!(loader::is_binary(&root.join("late.txt")));
    assert!(!loader::is_binary(&root.join("too_late.txt")));
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);

    click_row(&workbench, "tree.bin", cx);
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-p"
    } else {
        "ctrl-p"
    });
    cx.simulate_input("quick.dat");
    let picker = crate::tests::quick_open::picker(&workbench, cx);
    assert_eq!(crate::tests::quick_open::paths(&picker, cx)[0], "quick.dat");
    cx.simulate_keystrokes("enter");
    draw(cx);
    open_path(&pane, root.join("late.txt"), cx);
    open_path(&pane, root.join("too_late.txt"), cx);

    assert_eq!(
        tabs(&pane, cx),
        [
            ("tree.bin".into(), Shows::Hex),
            ("quick.dat".into(), Shows::Hex),
            ("late.txt".into(), Shows::Hex),
            ("too_late.txt".into(), Shows::Editor),
        ]
    );
    // The hex tab shows its rows, and has focus as an opened file's tab.
    activate(&pane, 0, cx);
    let viewer = hex_viewer(&pane, 0, cx);
    cx.update(|window, cx| assert!(viewer.focus_handle(cx).contains_focused(window, cx)));
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.view().len(), 100);
        assert!(viewer.rows_bounds().is_some(), "drawn");
    });
}

#[gpui::test]
fn text_images_and_bom_files_open_as_before(cx: &mut TestAppContext) {
    let utf16le: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain("hi\n".encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    let utf16be: Vec<u8> = [0xFE, 0xFF]
        .into_iter()
        .chain("hi\n".encode_utf16().flat_map(u16::to_be_bytes))
        .collect();
    let utf32be: &[u8] = &[0x00, 0x00, 0xFE, 0xFF, 0x00, 0x00, 0x00, b'h'];
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"/>"#;
    let (_dir, root) = project(&[
        ("notes.txt", b"plain text\n"),
        ("le.txt", &utf16le),
        ("be.txt", &utf16be),
        ("wide.txt", utf32be),
        ("logo.png", PNG),
        ("logo.svg", svg.as_bytes()),
    ]);
    for name in ["notes.txt", "le.txt", "be.txt", "wide.txt", "logo.svg"] {
        assert!(!loader::is_binary(&root.join(name)), "{name}");
    }
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);
    for name in [
        "notes.txt",
        "le.txt",
        "be.txt",
        "wide.txt",
        "logo.png",
        "logo.svg",
    ] {
        open_path(&pane, root.join(name), cx);
    }
    assert_eq!(
        tabs(&pane, cx),
        [
            ("notes.txt".into(), Shows::Editor),
            ("le.txt".into(), Shows::Editor),
            ("be.txt".into(), Shows::Editor),
            ("wide.txt".into(), Shows::Editor),
            ("logo.png".into(), Shows::Image),
            ("logo.svg".into(), Shows::Image),
        ]
    );
    let text = |ix: usize, cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, cx| {
            pane.tabs()[ix]
                .editor()
                .unwrap()
                .read(cx)
                .model()
                .text()
                .to_string()
        })
    };
    assert_eq!(text(1, cx), "hi\n");
    assert_eq!(text(2, cx), "hi\n");
}

#[gpui::test]
fn a_large_binary_opens_in_hex_not_the_file_viewer(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("big.bin", &binary(8192)), ("big.log", &[b'x'; 8192])]);
    loader::test_support::lower_threshold(&root, 4096, false);
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);
    open_path(&pane, root.join("big.bin"), cx);
    open_path(&pane, root.join("big.log"), cx);
    assert_eq!(
        tabs(&pane, cx),
        [
            ("big.bin".into(), Shows::Hex),
            ("big.log".into(), Shows::Viewer)
        ]
    );
}

#[gpui::test]
fn open_as_hex_replaces_an_editor_in_place(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[
        ("a.txt", b"alpha\n"),
        ("b.txt", b"beta\n"),
        ("logo.png", PNG),
        ("big.log", &[b'x'; 8192]),
    ]);
    loader::test_support::lower_threshold(&root, 4096, false);
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);
    for name in ["a.txt", "b.txt", "logo.png", "big.log"] {
        open_path(&pane, root.join(name), cx);
    }
    activate(&pane, 0, cx);
    run_in("hex.open_as_hex", "editor", cx);
    assert_eq!(
        tabs(&pane, cx),
        [
            ("a.txt".into(), Shows::Hex),
            ("b.txt".into(), Shows::Editor),
            ("logo.png".into(), Shows::Image),
            ("big.log".into(), Shows::Viewer),
        ]
    );
    assert_eq!(pane.read_with(cx, |pane, _| pane.active()), Some(0));
    let viewer = hex_viewer(&pane, 0, cx);
    cx.update(|window, cx| assert!(viewer.focus_handle(cx).contains_focused(window, cx)));
    let first = viewer.read_with(cx, |viewer, _| viewer.view().bytes(0..6).unwrap());
    assert_eq!(first, b"alpha\n");

    // From an image viewer, and from a large file's text viewer, which
    // sets `editor` too.
    activate(&pane, 2, cx);
    run_in("hex.open_as_hex", "ImageViewer", cx);
    activate(&pane, 3, cx);
    run_in("hex.open_as_hex", "editor", cx);
    assert_eq!(
        tabs(&pane, cx),
        [
            ("a.txt".into(), Shows::Hex),
            ("b.txt".into(), Shows::Editor),
            ("logo.png".into(), Shows::Hex),
            ("big.log".into(), Shows::Hex),
        ]
    );
}

#[gpui::test]
fn open_as_hex_refuses_a_dirty_editor(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("a.txt", b"alpha\n")]);
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);
    open_path(&pane, root.join("a.txt"), cx);
    activate(&pane, 0, cx);
    cx.simulate_input("changed ");
    draw(cx);
    toasts(&workbench, cx);

    run_in("hex.open_as_hex", "editor", cx);
    assert_eq!(tabs(&pane, cx), [("a.txt".into(), Shows::Editor)]);
    assert_eq!(
        toasts(&workbench, cx),
        [(
            Level::Error,
            "Save the file before opening it as hex".into()
        )]
    );
    let text = pane.read_with(cx, |pane, cx| {
        pane.tabs()[0]
            .editor()
            .unwrap()
            .read(cx)
            .model()
            .text()
            .to_string()
    });
    assert_eq!(text, "changed alpha\n");
}

#[gpui::test]
fn open_as_text_from_hex_gives_an_editor(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[
        ("one.bin", b"ab\0cd"),
        ("two.bin", b"xy\0z"),
        ("c.txt", b"c\n"),
    ]);
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);
    for name in ["one.bin", "two.bin", "c.txt"] {
        open_path(&pane, root.join(name), cx);
    }
    activate(&pane, 0, cx);
    run_in("hex.open_as_text", "HexViewer", cx);
    assert_eq!(
        tabs(&pane, cx),
        [
            ("one.bin".into(), Shows::Editor),
            ("two.bin".into(), Shows::Hex),
            ("c.txt".into(), Shows::Editor),
        ]
    );

    // The header's Text button does the same.
    activate(&pane, 1, cx);
    let button = cx
        .debug_bounds(kiln_l10n::verbatim!("hex-viewer-open-as-text"))
        .expect("drawn");
    cx.simulate_click(button.center(), Modifiers::none());
    draw(cx);
    assert_eq!(tabs(&pane, cx)[1], ("two.bin".into(), Shows::Editor));
    let text = pane.read_with(cx, |pane, cx| {
        pane.tabs()[1]
            .editor()
            .unwrap()
            .read(cx)
            .model()
            .text()
            .to_string()
    });
    assert!(text.starts_with("xy"), "{text:?}");
    assert_eq!(pane.read_with(cx, |pane, _| pane.active()), Some(1));

    // And back again.
    run_in("hex.open_as_hex", "editor", cx);
    assert_eq!(tabs(&pane, cx)[1], ("two.bin".into(), Shows::Hex));
}

#[gpui::test]
fn open_as_text_of_a_large_binary_gives_the_file_viewer(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("big.bin", &binary(8192))]);
    loader::test_support::lower_threshold(&root, 4096, false);
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);
    open_path(&pane, root.join("big.bin"), cx);
    activate(&pane, 0, cx);
    let view = hex_viewer(&pane, 0, cx).read_with(cx, |viewer, _| viewer.view().clone());

    run_in("hex.open_as_text", "HexViewer", cx);
    assert_eq!(tabs(&pane, cx), [("big.bin".into(), Shows::Viewer)]);
    // Never read into a buffer: the same view, its rows from an index.
    pane.read_with(cx, |pane, cx| {
        let viewer = pane.tabs()[0].viewer().unwrap().read(cx);
        assert!(std::sync::Arc::ptr_eq(viewer.view(), &view));
    });
    assert!(workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).is_empty()));
}

#[gpui::test]
fn hex_commands_are_cataloged_in_their_contexts(cx: &mut TestAppContext) {
    // The workbench's two, typed, as the app registers them.
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let typed: Vec<(String, String, Option<&str>)> = cx.update(|cx| {
        (kiln::Contributions::commands(cx).iter())
            .filter(|spec| spec.id.as_str().starts_with("hex."))
            .map(|spec| (spec.id.to_string(), spec.title.clone(), spec.context))
            .collect()
    });
    for (id, title, context) in [
        (
            "hex.open_as_hex",
            "Open as Hex",
            "editor || ImageViewer || PdfNotice",
        ),
        ("hex.open_as_text", "Open as Text", "HexViewer"),
    ] {
        let found = typed.iter().find(|(typed, ..)| typed == id);
        assert_eq!(
            found.map(|(_, title, context)| (title.as_str(), *context)),
            Some((title, Some(context))),
            "{id}: {typed:?}"
        );
    }
    // The view's own, listed for the palette in the viewer's context.
    let list = crate::command_list();
    for (id, title) in [
        ("hex.go_to_offset", "Go to Offset…"),
        ("hex.find", "Find Bytes"),
        ("hex.find_next", "Find Next Bytes"),
        ("hex.find_previous", "Find Previous Bytes"),
        ("hex.copy", "Copy Bytes"),
        ("hex.copy_as_hex", "Copy as Hex"),
        ("hex.copy_as_text", "Copy as Text"),
    ] {
        let meta = (list.iter().find(|meta| meta.id.as_str() == id))
            .unwrap_or_else(|| panic!("{id} is listed"));
        assert_eq!(meta.label().as_ref(), title, "{id}");
        let shown = meta.context.as_ref().map(ToString::to_string);
        assert_eq!(shown.as_deref(), Some("HexViewer"), "{id}");
    }
    // The keymap knows them all, so their bindings aren't dropped.
    let catalog = crate::command_catalog();
    for id in [
        "hex.open_as_hex",
        "hex.open_as_text",
        "hex.find",
        "hex.go_to_offset",
        "hex.copy",
        "hex.find_next",
        "hex.find_previous",
    ] {
        let id = fe_command::CommandId::parse(id).unwrap();
        assert!(catalog.check(&id, &fe_command::Value::Null).is_ok(), "{id}");
    }
}

#[gpui::test]
fn hex_mode_has_no_minimap(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::write(root.join("data.bin"), binary(4096)).unwrap();
    let user = root.join("settings.toml");
    fs::write(&user, "[editor.minimap]\nenabled = true\n").unwrap();
    let mut store = SettingsStore::load(Some(user.clone()), None);
    let (workbench, cx) = open(cx, &root);
    let reload = |store: &mut SettingsStore, toml: &str, cx: &mut VisualTestContext| {
        fs::write(&user, toml).unwrap();
        if let Some(event) = store.reload(std::slice::from_ref(&user)) {
            workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
        }
        draw(cx);
    };
    reload(
        &mut store,
        "[editor.minimap]\nenabled = true\nmax_column = 100\n",
        cx,
    );
    let pane = pane(&workbench, cx);
    open_path(&pane, root.join("data.bin"), cx);
    activate(&pane, 0, cx);

    // No editor, so no minimap; the rows reach the pane's right edge, less
    // the scrollbar.
    let no_minimap = |cx: &mut VisualTestContext| {
        assert!(workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).is_empty()));
        let viewer = hex_viewer(&pane, 0, cx);
        let (rows, scrollbar) = viewer.read_with(cx, |viewer, _| {
            (
                viewer.rows_bounds().unwrap(),
                viewer.scrollbar_bounds().unwrap(),
            )
        });
        let pane_bounds = pane.read_with(cx, |pane, _| pane.bounds());
        assert_eq!(rows.right(), scrollbar.left());
        assert_eq!(scrollbar.size.width, fe_ui::SCROLLBAR_WIDTH);
        assert_eq!(scrollbar.right(), pane_bounds.right());
        rows
    };
    let rows = no_minimap(cx);
    // A reload toggling the minimap leaves it off.
    reload(&mut store, "[editor.minimap]\nenabled = false\n", cx);
    reload(&mut store, "[editor.minimap]\nenabled = true\n", cx);
    assert_eq!(no_minimap(cx), rows);

    // Open as Text: an editor, with its minimap.
    run_in("hex.open_as_text", "HexViewer", cx);
    let editor: Entity<EditorView> = pane
        .read_with(cx, |pane, _| pane.tabs()[0].editor().cloned())
        .expect("an editor");
    draw(cx);
    let minimap = editor.read_with(cx, |editor, _| {
        editor.minimap_bounds().map(|b| b.size.width)
    });
    assert!(minimap.is_some_and(|width| width > px(0.)), "{minimap:?}");
    // Open as Hex drops it again.
    run_in("hex.open_as_hex", "editor", cx);
    assert_eq!(no_minimap(cx), rows);
}

#[gpui::test]
fn a_saved_layout_reopens_the_hex_tab(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("data.bin", &binary(4096)), ("notes.txt", b"n\n")]);
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);
    open_path(&pane, root.join("data.bin"), cx);
    open_path(&pane, root.join("notes.txt"), cx);
    let viewer = hex_viewer(&pane, 0, cx);
    viewer.update(cx, |viewer, cx| {
        viewer.select(0x200, 0x210);
        cx.notify();
    });
    draw(cx);

    let entry = workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.restore_layout(&entry, window, cx)
        })
    });
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_eq!(
        tabs(&pane, cx),
        [
            ("data.bin".into(), Shows::Hex),
            ("notes.txt".into(), Shows::Editor)
        ]
    );
    let restored = hex_viewer(&pane, 0, cx);
    assert_ne!(restored.entity_id(), viewer.entity_id(), "reopened afresh");
    restored.read_with(cx, |viewer, _| {
        assert_eq!(viewer.selection_ends(), (0x200, 0x210));
        assert_eq!(viewer.path(), root.join("data.bin"));
    });
}

/// The find bar's keys in the hex viewer's header inputs, typed in the app,
/// where the workbench dispatches them as its own typed commands: Enter
/// goes to an offset or finds, Shift-Enter finds back, Match Case's key
/// toggles it, and Escape closes the input, stopping its search.
#[gpui::test]
fn the_header_inputs_take_the_find_bar_keys(cx: &mut TestAppContext) {
    let mut bytes = vec![b'.'; 256];
    bytes[0x40..0x42].copy_from_slice(b"\xDE\xAD");
    bytes[0x80..0x82].copy_from_slice(b"\xDE\xAD");
    bytes[0x90..0x92].copy_from_slice(b"Ab");
    bytes[0xA0..0xA2].copy_from_slice(b"ab");
    bytes[3] = 0;
    let (_dir, root) = project(&[("data.bin", &bytes)]);
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);
    open_path(&pane, root.join("data.bin"), cx);
    activate(&pane, 0, cx);
    let viewer = hex_viewer(&pane, 0, cx);
    let selected = |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.selected());
    let input_open = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| viewer.read(cx).input_focused(window, cx))
    };
    let secondary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    let case = if cfg!(target_os = "macos") {
        "alt-cmd-c"
    } else {
        "alt-c"
    };

    cx.simulate_keystrokes("ctrl-g");
    draw(cx);
    assert!(input_open(cx));
    cx.simulate_input("0x20");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(selected(cx), 0x20..0x21);
    assert!(!input_open(cx), "went, and closed");

    cx.simulate_keystrokes(&format!("{secondary}-f"));
    draw(cx);
    cx.simulate_input("de ad");
    draw(cx);
    assert_eq!(selected(cx), 0x40..0x42);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(selected(cx), 0x80..0x82);
    cx.simulate_keystrokes("shift-enter");
    draw(cx);
    assert_eq!(selected(cx), 0x40..0x42);

    // Text, then Match Case by its key: only the lowercase `ab`.
    let text_mode = cx
        .debug_bounds(kiln_l10n::verbatim!("hex-find-mode-text"))
        .expect("drawn");
    cx.simulate_click(text_mode.center(), Modifiers::none());
    draw(cx);
    // Reopened, its text selected, so typing replaces it.
    cx.simulate_keystrokes(&format!("{secondary}-f"));
    draw(cx);
    cx.simulate_input("ab");
    draw(cx);
    assert_eq!(selected(cx), 0x90..0x92);
    cx.simulate_keystrokes(case);
    draw(cx);
    assert_eq!(selected(cx), 0xA0..0xA2);

    // Escape: closed, the rows focused, the pane's own find bar untouched.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!input_open(cx));
    cx.update(|window, cx| assert!(viewer.focus_handle(cx).is_focused(window)));
    assert_eq!(selected(cx), 0xA0..0xA2);
}
