//! The Problems panel listing a server's diagnostics for a file no editor
//! shows (`LspEvent::FileDiagnostics`), decoded against its text on disk,
//! with a UTF-16 fake: an emoji before the problem is two code units, one
//! char and four bytes.

use std::fs;

use fe_lsp::fake::{InitializeReply, Script, initialize_result};
use gpui::{Modifiers, TestAppContext};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, PositionEncodingKind, PublishDiagnosticsParams,
    notification::PublishDiagnostics,
};

use super::{
    ShownRow, active_editor, active_path, diagnostic, focused, item, panel, rows, selection,
};
use crate::tests::lsp::Lsp;

/// `src/other.rs`: `bad` on row 1, after an emoji.
const OTHER: &str = "fn g() {\n    let s = \"🦀\"; bad();\n}\n";

/// `bad`'s offset in `OTHER`, and its UTF-16 column on its row.
const BAD: usize = 29;
const BAD_UTF16: u32 = 18;

/// The error on `bad`, in UTF-16 columns.
fn bad() -> Diagnostic {
    diagnostic(
        1,
        BAD_UTF16,
        3,
        DiagnosticSeverity::ERROR,
        "cannot find `bad`",
    )
}

/// The error's row: its column is counted in bytes of the row, as an open
/// file's is (`EditorModel::problems`), past the emoji's four.
fn bad_row() -> ShownRow {
    ShownRow::Problem(
        "E",
        "cannot find `bad`".into(),
        Some("rustc".into()),
        "2:21".into(),
    )
}

/// The project with `OTHER` on disk and `src/main.rs` open on a UTF-16
/// rust fake, which has `src/other.rs` closed.
fn setup(cx: &mut TestAppContext) -> Lsp {
    let rust = Script {
        initialize: InitializeReply::Result(Box::new(initialize_result(Some(
            PositionEncodingKind::UTF16,
        )))),
        ..Script::default()
    };
    let mut s = Lsp::with_scripts(cx, rust, Script::default());
    s.set_trust(true);
    fs::write(s.path("src/other.rs"), OTHER).unwrap();
    s.open("src/main.rs");
    let uri = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    assert_eq!(&OTHER[BAD..BAD + 3], "bad");
    s
}

/// Has the fake publish `diagnostics` for `name` at `version`, or
/// unversioned.
fn publish(s: &Lsp, name: &str, version: Option<i32>, diagnostics: Vec<Diagnostic>) {
    s.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri: s.uri(name),
            diagnostics,
            version,
        });
}

fn other_rows() -> Vec<ShownRow> {
    vec![ShownRow::File("src/other.rs".into(), 1), bad_row()]
}

#[gpui::test]
fn unopened_files_are_listed_and_open_on_click(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    publish(&s, "src/other.rs", None, vec![bad()]);
    s.until("other.rs listed", |s| rows(s) == other_rows());
    assert_eq!(item(&mut s).as_deref(), Some("1"));

    // A click opens it in the active pane, `bad` selected, focused.
    let pane = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    s.run("problems.show");
    let panel = panel(&mut s);
    let bounds = panel.read_with(&s.cx, |panel, _| panel.row_bounds(1).expect("drawn"));
    s.cx.simulate_click(bounds.center(), Modifiers::none());
    s.draw();
    assert_eq!(s.workbench.read_with(&s.cx, |w, _| w.pane().clone()), pane);
    assert_eq!(active_path(&mut s), s.path("src/other.rs"));
    assert_eq!(selection(&mut s), BAD..BAD + 3);
    let editor = active_editor(&mut s);
    assert!(focused(&mut s, &editor));

    // Open, it's listed from its buffer, which has no problems yet.
    assert_eq!(rows(&mut s), []);
    assert_eq!(item(&mut s), None);
}

#[gpui::test]
fn an_open_file_is_listed_once(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    publish(&s, "src/other.rs", None, vec![bad()]);
    s.until("other.rs listed", |s| rows(s) == other_rows());

    // Opened, the stored problems aren't listed, nor its buffer's until the
    // server publishes for its document.
    s.open("src/other.rs");
    let uri = s.uri("src/other.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    s.draw();
    assert_eq!(rows(&mut s), []);
    let version = s.fake.document(&uri).expect("it's open").version;
    let unused = diagnostic(1, 8, 1, DiagnosticSeverity::WARNING, "unused `s`");
    publish(&s, "src/other.rs", Some(version), vec![unused, bad()]);
    let open_rows = vec![
        ShownRow::File("src/other.rs".into(), 2),
        ShownRow::Problem("W", "unused `s`".into(), Some("rustc".into()), "2:9".into()),
        bad_row(),
    ];
    s.until("the buffer's problems", |s| rows(s) == open_rows);
    assert_eq!(item(&mut s).as_deref(), Some("1"));

    // Closed, the stored ones are listed again.
    s.run("workbench.close_tab");
    s.until("other.rs listed again", |s| rows(s) == other_rows());
    assert_eq!(item(&mut s).as_deref(), Some("1"));
}

#[gpui::test]
fn cleared_and_restarted_servers_drop_them(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    publish(&s, "src/other.rs", None, vec![bad()]);
    s.until("other.rs listed", |s| rows(s) == other_rows());

    // An empty publication clears them.
    publish(&s, "src/other.rs", None, Vec::new());
    s.until("other.rs cleared", |s| rows(s).is_empty());
    assert_eq!(item(&mut s), None);

    // So does the server restarting.
    publish(&s, "src/other.rs", None, vec![bad()]);
    s.until("other.rs listed again", |s| rows(s) == other_rows());
    s.fake.crash_at(1);
    s.wait_item(Some("rust-analyzer: restarting (1/5)"));
    assert_eq!(rows(&mut s), []);
    assert_eq!(item(&mut s), None);
}
