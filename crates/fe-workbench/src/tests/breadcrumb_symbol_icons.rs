use std::fs;

use fe_syntax::OutlineKind;
use fe_ui::{CrumbKind, file_icon};
use gpui::{Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::open_root;
use crate::outline_panel::symbol_icon;

const SRC: &str = "\
mod outer {
    pub struct S;
    impl S {
        fn a() {}
        fn b() {
            let x = 1;
        }
    }
}
";

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

#[gpui::test]
fn symbol_crumbs_carry_their_kinds_icon(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    let path = dir.path().join("src/mod.rs");
    fs::write(&path, SRC).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    settle(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown");
    editor.update(cx, |editor, cx| {
        let offset = editor.model().text().to_string().find("let x").unwrap();
        editor.model_mut().set_cursor(offset);
        cx.notify();
    });
    settle(cx);

    let drawn = pane.read_with(cx, |pane, _| pane.drawn_crumbs().to_vec());
    assert_eq!(drawn[0], ("src".to_string(), CrumbKind::Path, None));
    assert_eq!(
        drawn[1],
        (
            "mod.rs".to_string(),
            CrumbKind::File,
            Some(file_icon("mod.rs"))
        )
    );
    let symbols: Vec<_> = drawn[2..].to_vec();
    assert_eq!(
        symbols,
        [
            (
                "outer".to_string(),
                CrumbKind::Symbol,
                Some(symbol_icon(OutlineKind::Module))
            ),
            (
                "impl S".to_string(),
                CrumbKind::Symbol,
                Some(symbol_icon(OutlineKind::Impl))
            ),
            (
                "b".to_string(),
                CrumbKind::Symbol,
                Some(symbol_icon(OutlineKind::Function))
            ),
        ]
    );
    assert_ne!(symbols[0].2, symbols[1].2);
    assert!(symbols.iter().all(|(.., icon)| icon.is_some()));

    // Clicking a symbol crumb opens the quick outline, whose rows carry the
    // same icons.
    let crumb = cx.debug_bounds("crumb:4:b").expect("drawn");
    cx.simulate_click(crumb.center(), Modifiers::none());
    settle(cx);
    assert!(workbench.read_with(cx, |workbench, _| workbench.toolbar_picker().is_none()));
    let icon = symbol_icon(OutlineKind::Function).path();
    assert!(
        cx.debug_bounds(format!("symbol-picker-icon:b:{icon}").leak())
            .is_some()
    );
}
