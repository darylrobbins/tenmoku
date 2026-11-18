//! PDFs in the workbench: they open in the "This is a PDF" notice from the
//! tree, quick open and `Pane::open`, whatever their bytes or size; other
//! files open as before; the notice's button and `hex.open_as_hex` do their
//! jobs; and a saved layout reopens it.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext, px, size};

use crate::{
    Launch, Pane, Workbench,
    loader::{self, Opened},
    workbench_commands::OpenSeam,
};

/// What a tab shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shows {
    Pdf,
    Hex,
    Editor,
    Image,
}

const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x38, 0x21, 0x27, 0xf7,
    0x1f, 0x00, 0x04, 0xba, 0x02, 0x04, 0x3e, 0x57, 0x1f, 0xc7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn project(files: &[(&str, &[u8])]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    for (name, bytes) in files {
        fs::write(root.join(name), bytes).unwrap();
    }
    (dir, root)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

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

fn tabs(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Vec<(String, Shows)> {
    pane.read_with(cx, |pane, _| {
        (pane.tabs().iter())
            .map(|tab| {
                let shows = match () {
                    _ if tab.pdf_notice().is_some() => Shows::Pdf,
                    _ if tab.hex_viewer().is_some() => Shows::Hex,
                    _ if tab.editor().is_some() => Shows::Editor,
                    _ if tab.image_viewer().is_some() => Shows::Image,
                    _ => panic!("{:?} shows something else", tab.path),
                };
                let name = tab.path.as_ref().unwrap().file_name().unwrap();
                (name.to_string_lossy().into_owned(), shows)
            })
            .collect()
    })
}

fn open_path(pane: &Entity<Pane>, path: PathBuf, cx: &mut VisualTestContext) {
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

fn activate(pane: &Entity<Pane>, ix: usize, cx: &mut VisualTestContext) {
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(ix, window, cx)));
    draw(cx);
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

fn run_in(command: &str, frame: &str, cx: &mut VisualTestContext) {
    let context: ContextStack = ["workbench", frame]
        .into_iter()
        .map(ContextFrame::new)
        .collect();
    let run = RunCommand(Invocation::of(command).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// A PDF's first bytes, then NULs and other bytes: a binary file.
fn binary_pdf() -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    bytes.extend((0..600).map(|i| (i % 3) as u8));
    bytes
}

/// A PDF whose header sits 100 bytes in.
fn late_header() -> Vec<u8> {
    let mut bytes = vec![b'j'; 100];
    bytes.extend_from_slice(b"%PDF-1.4\n");
    bytes.extend_from_slice(&[0, 1, 2, 3]);
    bytes
}

#[gpui::test]
fn a_pdf_opens_in_the_notice_not_hex_or_text(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[
        ("tree.pdf", &binary_pdf()),
        ("quick.dat", &late_header()),
        ("ascii.PDF", b"plain ascii body\n"),
        ("direct.bin", &binary_pdf()),
        ("big.pdf", &vec![b'x'; 8192]),
        ("bigbinary.dat", &{
            let mut bytes = binary_pdf();
            bytes.resize(8192, 0);
            bytes
        }),
    ]);
    loader::test_support::lower_threshold(&root, 4096, false);
    for name in [
        "tree.pdf",
        "quick.dat",
        "ascii.PDF",
        "big.pdf",
        "bigbinary.dat",
    ] {
        let opened = loader::open(root.join(name)).unwrap();
        assert!(matches!(opened, Opened::Pdf), "{name}: {opened:?}");
    }
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);

    click_row(&workbench, "tree.pdf", cx);
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
    for name in ["ascii.PDF", "direct.bin", "big.pdf", "bigbinary.dat"] {
        open_path(&pane, root.join(name), cx);
    }
    assert_eq!(
        tabs(&pane, cx),
        [
            ("tree.pdf".into(), Shows::Pdf),
            ("quick.dat".into(), Shows::Pdf),
            ("ascii.PDF".into(), Shows::Pdf),
            ("direct.bin".into(), Shows::Pdf),
            ("big.pdf".into(), Shows::Pdf),
            ("bigbinary.dat".into(), Shows::Pdf),
        ]
    );
    // The notice says so, and has the file's size.
    activate(&pane, 0, cx);
    for id in [
        "pdf-notice-name",
        "pdf-notice-says",
        "pdf-notice-open-default",
        "pdf-notice-open-hex",
    ] {
        assert!(cx.debug_bounds(id).is_some(), "{id} drawn");
    }
    let len = pane.read_with(cx, |pane, cx| {
        pane.tabs()[0].pdf_notice().unwrap().read(cx).file_len()
    });
    assert_eq!(len, Some(binary_pdf().len() as u64));
}

#[gpui::test]
fn other_files_open_as_before(cx: &mut TestAppContext) {
    let utf16le: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain("hi\n".encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"/>"#;
    let (_dir, root) = project(&[
        ("notes.txt", b"plain text\n"),
        ("data.bin", &[0, 1, 2, 3, 4, 5]),
        ("logo.png", PNG),
        ("logo.svg", svg.as_bytes()),
        ("bom.txt", &utf16le),
        // The magic past the probe is not a PDF.
        ("far.txt", &{
            let mut bytes = vec![b'a'; loader::PDF_PROBE];
            bytes.extend_from_slice(b"%PDF-1.7\n");
            bytes
        }),
    ]);
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);
    for name in [
        "notes.txt",
        "data.bin",
        "logo.png",
        "logo.svg",
        "bom.txt",
        "far.txt",
    ] {
        open_path(&pane, root.join(name), cx);
    }
    assert_eq!(
        tabs(&pane, cx),
        [
            ("notes.txt".into(), Shows::Editor),
            ("data.bin".into(), Shows::Hex),
            ("logo.png".into(), Shows::Image),
            ("logo.svg".into(), Shows::Image),
            ("bom.txt".into(), Shows::Editor),
            ("far.txt".into(), Shows::Editor),
        ]
    );
}

static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn record(path: &Path, _: &mut gpui::App) {
    OPENED.lock().unwrap().push(path.to_owned());
}

#[gpui::test]
fn open_in_default_app_passes_the_path_to_the_seam(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("paper.pdf", &binary_pdf()), ("a.txt", b"a\n")]);
    let (workbench, cx) = open(cx, &root);
    cx.update(|_, cx| cx.set_global(OpenSeam(record)));
    OPENED.lock().unwrap().clear();
    let pane = pane(&workbench, cx);
    open_path(&pane, root.join("a.txt"), cx);
    open_path(&pane, root.join("paper.pdf"), cx);
    activate(&pane, 1, cx);

    // The command, in the notice's context only.
    activate(&pane, 0, cx);
    run_in("pdf.open_in_default_app", "editor", cx);
    assert_eq!(*OPENED.lock().unwrap(), Vec::<PathBuf>::new());
    activate(&pane, 1, cx);

    // The button.
    let button = cx.debug_bounds("pdf-notice-open-default").expect("drawn");
    cx.simulate_click(button.center(), Modifiers::none());
    draw(cx);
    assert_eq!(*OPENED.lock().unwrap(), [root.join("paper.pdf")]);
    // The tab stays a notice.
    assert_eq!(tabs(&pane, cx)[1], ("paper.pdf".into(), Shows::Pdf));

    run_in("pdf.open_in_default_app", "PdfNotice", cx);
    assert_eq!(OPENED.lock().unwrap().len(), 2);
}

#[gpui::test]
fn open_as_hex_replaces_the_notice_in_place(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[
        ("a.txt", b"a\n"),
        ("one.pdf", &binary_pdf()),
        ("two.pdf", &binary_pdf()),
    ]);
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);
    for name in ["a.txt", "one.pdf", "two.pdf"] {
        open_path(&pane, root.join(name), cx);
    }
    // The button.
    activate(&pane, 1, cx);
    let button = cx.debug_bounds("pdf-notice-open-hex").expect("drawn");
    cx.simulate_click(button.center(), Modifiers::none());
    draw(cx);
    assert_eq!(
        tabs(&pane, cx),
        [
            ("a.txt".into(), Shows::Editor),
            ("one.pdf".into(), Shows::Hex),
            ("two.pdf".into(), Shows::Pdf),
        ]
    );
    assert_eq!(pane.read_with(cx, |pane, _| pane.active()), Some(1));
    let first = pane.read_with(cx, |pane, cx| {
        let viewer = pane.tabs()[1].hex_viewer().unwrap().read(cx);
        viewer.view().bytes(0..8).unwrap()
    });
    assert_eq!(first, b"%PDF-1.7");

    // The command.
    activate(&pane, 2, cx);
    run_in("hex.open_as_hex", "PdfNotice", cx);
    assert_eq!(tabs(&pane, cx)[2], ("two.pdf".into(), Shows::Hex));
}

#[gpui::test]
fn a_saved_layout_reopens_the_notice(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("paper.pdf", &binary_pdf()), ("notes.txt", b"n\n")]);
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);
    open_path(&pane, root.join("paper.pdf"), cx);
    open_path(&pane, root.join("notes.txt"), cx);
    let before = pane.read_with(cx, |pane, _| pane.tabs()[0].pdf_notice().cloned());
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
            ("paper.pdf".into(), Shows::Pdf),
            ("notes.txt".into(), Shows::Editor)
        ]
    );
    let after = pane.read_with(cx, |pane, _| pane.tabs()[0].pdf_notice().cloned());
    assert_ne!(
        before.unwrap().entity_id(),
        after.unwrap().entity_id(),
        "reopened afresh"
    );
}

#[gpui::test]
fn a_missing_pdf_is_not_restored(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("paper.pdf", &binary_pdf()), ("notes.txt", b"n\n")]);
    let (workbench, cx) = open(cx, &root);
    let pane = pane(&workbench, cx);
    open_path(&pane, root.join("paper.pdf"), cx);
    open_path(&pane, root.join("notes.txt"), cx);
    let entry = workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    fs::remove_file(root.join("paper.pdf")).unwrap();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.restore_layout(&entry, window, cx)
        })
    });
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_eq!(tabs(&pane, cx), [("notes.txt".into(), Shows::Editor)]);
}
