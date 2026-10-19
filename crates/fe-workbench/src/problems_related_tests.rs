//! A problem's related locations as rows under it in the Problems panel,
//! each opening its location.

use std::fs;

use gpui::{Modifiers, TestAppContext};
use lsp_types::{DiagnosticRelatedInformation, DiagnosticSeverity, Location, Position, Range};

use super::{
    ShownRow, active_editor, active_path, diagnostic, in_panel, panel, publish, rows, selected,
    selection,
};
use crate::tests::lsp::Lsp;

/// `src/a.rs`: `v` declared at offset 17, used at 32.
const A: &str = "fn a() {\n    let v = 1;\n    use_it(v);\n}\n";
const V: usize = 17;
/// `src/b.rs`, never opened.
const B: &str = "zero\none\ntwo\nthree\nfour\n";

fn related(
    s: &Lsp,
    name: &str,
    line: u32,
    column: u32,
    message: &str,
) -> DiagnosticRelatedInformation {
    DiagnosticRelatedInformation {
        location: Location {
            uri: s.uri(name),
            range: Range::new(Position::new(line, column), Position::new(line, column + 1)),
        },
        message: message.into(),
    }
}

/// Clicks row `ix` of the panel, as the last frame drew it.
fn click(s: &mut Lsp, ix: usize) {
    let panel = panel(s);
    let bounds = panel.read_with(&s.cx, |panel, _| panel.row_bounds(ix).expect("drawn"));
    s.cx.simulate_click(bounds.center(), Modifiers::none());
    s.draw();
}

#[gpui::test]
fn related_rows_open_their_locations(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    fs::write(s.path("src/a.rs"), A).unwrap();
    fs::write(s.path("src/b.rs"), B).unwrap();
    s.open("src/a.rs");
    let uri = s.uri("src/a.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    let mut error = diagnostic(
        2,
        11,
        1,
        DiagnosticSeverity::ERROR,
        "use of moved value: `v`",
    );
    error.related_information = Some(vec![
        related(&s, "src/a.rs", 1, 8, "`v` declared here"),
        related(&s, "src/b.rs", 3, 2, "moved here"),
    ]);
    publish(&s, "src/a.rs", vec![error]);
    let shown = |row: u32, related: u32| {
        let rustc = Some("rustc".to_owned());
        vec![
            ShownRow::File("src/a.rs".into(), 1),
            ShownRow::Problem(
                "E",
                "use of moved value: `v`".into(),
                rustc,
                format!("{row}:12"),
            ),
            ShownRow::Related("`v` declared here".into(), related.to_string()),
            ShownRow::Related("moved here".into(), "b.rs:4".into()),
        ]
    };
    s.until("the related rows", |s| rows(s) == shown(3, 2));

    // The selection steps over the related rows too.
    s.run("problems.show");
    let mut steps = Vec::new();
    for _ in 0..4 {
        in_panel(&mut s, "problems.select_next");
        steps.push(selected(&mut s));
    }
    in_panel(&mut s, "problems.select_previous");
    steps.push(selected(&mut s));
    assert_eq!(steps, [Some(1), Some(2), Some(3), Some(3), Some(2)]);

    // A line inserted above: the problem and its same-file location move.
    let editor = active_editor(&mut s);
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut()
                .set_selections([fe_buffer::ResolvedSelection {
                    range: 0..0,
                    reversed: false,
                }]);
            view.model_mut().insert("\n");
            cx.notify();
        })
    });
    s.until("the rows moved", |s| rows(s) == shown(4, 3));
    assert_eq!(selected(&mut s), Some(2));

    // The first opens its range in a.rs's editor.
    click(&mut s, 2);
    assert_eq!(active_path(&mut s), s.path("src/a.rs"));
    assert_eq!(selection(&mut s), V + 1..V + 2);

    // The second opens b.rs, with the caret at its row's start.
    click(&mut s, 3);
    assert_eq!(selected(&mut s), Some(3));
    s.until("b.rs open", |s| active_path(s) == s.path("src/b.rs"));
    let three = B.find("three").unwrap();
    assert_eq!(selection(&mut s), three..three);
}
