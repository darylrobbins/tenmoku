use std::fs;

use fe_syntax::OutlineKind;
use fe_ui::Icon;
use gpui::{Focusable as _, TestAppContext};

use super::open_root;
use crate::outline_panel::symbol_icon;

#[test]
fn every_kind_maps_to_a_bundled_icon() {
    let kinds = [
        OutlineKind::Module,
        OutlineKind::Class,
        OutlineKind::Struct,
        OutlineKind::Enum,
        OutlineKind::Interface,
        OutlineKind::Trait,
        OutlineKind::Impl,
        OutlineKind::Type,
        OutlineKind::Function,
        OutlineKind::Method,
        OutlineKind::Constant,
        OutlineKind::Macro,
        OutlineKind::Table,
        OutlineKind::Heading(1),
        OutlineKind::Heading(6),
        OutlineKind::Field,
        OutlineKind::Variant,
        OutlineKind::Variable,
    ];
    for kind in kinds {
        assert!(Icon::ALL.contains(&symbol_icon(kind)), "{kind:?}");
    }
    assert_eq!(
        symbol_icon(OutlineKind::Heading(1)),
        symbol_icon(OutlineKind::Heading(5))
    );
    assert_ne!(
        symbol_icon(OutlineKind::Struct),
        symbol_icon(OutlineKind::Function)
    );
}

#[gpui::test]
fn rows_carry_their_kinds_icon(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "struct Point;\n\nfn main() {}\n").unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-shift-o");
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    cx.run_until_parked();
    for selector in ["tree-icon:Point:braces", "tree-icon:main:square-function"] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "nothing drawn as {selector}"
        );
    }
}
