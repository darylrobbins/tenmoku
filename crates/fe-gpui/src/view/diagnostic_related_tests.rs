//! A diagnostic's related locations and its code's link in the hover
//! popover, under its message.

use std::path::PathBuf;

use fe_editor::diagnostics::{Diagnostic, RelatedInfo, Severity};
use gpui::{Modifiers, TestAppContext};

use super::{apply, at, editor_on, rest};

const TEXT: &str = "fn main() {\n    let a = &mut v;\n    let b = &mut v;\n}\n";

const HREF: &str = "https://doc.rust-lang.org/error_codes/E0499.html";

#[gpui::test]
fn the_hover_lists_related_locations_and_the_code_link(cx: &mut TestAppContext) {
    let (view, cx) = editor_on(TEXT, cx);
    let first = TEXT.find("&mut v").unwrap();
    let second = TEXT.rfind("&mut v").unwrap();
    let mut diagnostic = Diagnostic::new(
        Severity::Error,
        "cannot borrow `v` as mutable more than once at a time",
    );
    diagnostic.code = Some("E0499".into());
    diagnostic.code_href = Some(HREF.into());
    diagnostic.related = vec![
        RelatedInfo {
            path: Some(PathBuf::from("/project/src/main.rs")),
            row: 1,
            range: Some(first..first + 6),
            message: "first mutable borrow occurs here".into(),
        },
        RelatedInfo {
            path: Some(PathBuf::from("/project/src/lib.rs")),
            row: 9,
            range: None,
            message: "`v` declared here".into(),
        },
    ];
    apply(&view, vec![(second..second + 6, diagnostic)], cx);

    let over = at(&view, second + 1, cx);
    let expected = |line: u32| {
        format!(
            "cannot borrow `v` as mutable more than once at a time\n\
             ↳ line {line}: first mutable borrow occurs here\n\
             ↳ lib.rs:10: `v` declared here\n\
             E0499: {HREF}"
        )
    };
    assert_eq!(rest(&view, over, cx), Some(expected(2)));

    // A line inserted above: the same-file location's row moves with it.
    view.update(cx, |view, cx| {
        view.model_mut()
            .set_selections([fe_buffer::ResolvedSelection {
                range: 0..0,
                reversed: false,
            }]);
        view.model_mut().insert("\n");
        cx.notify();
    });
    cx.run_until_parked();
    let blank = at(&view, 0, cx);
    cx.simulate_mouse_move(blank, None, Modifiers::none());
    let over = at(&view, second + 2, cx);
    assert_eq!(rest(&view, over, cx), Some(expected(3)));
}
