//! Symbol icons: a glyph of its own per kind, drawn in the editor theme's
//! syntax colour for the kind, in the outline, the symbol picker, the call
//! hierarchy and the breadcrumbs.

use std::{fs, sync::Arc};

use fe_config::theme::EditorPalette;
use fe_syntax::OutlineKind;
use fe_ui::Icon;
use gpui::{Focusable as _, Hsla, TestAppContext, VisualTestContext, rgb};

use super::open_root;
use crate::outline_panel::{symbol_icon, symbol_tint};

const KINDS: [OutlineKind; 18] = [
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

/// `palette`'s colour for the highlight `name`, as listed.
fn listed(palette: &EditorPalette, name: &str) -> Hsla {
    let (_, color) = (palette.syntax.iter())
        .find(|(listed, _)| listed == name)
        .unwrap_or_else(|| panic!("the palette lists {name}"));
    gpui::rgba(color.to_rgba_u32()).into()
}

fn hex(rgb_hex: u32) -> Hsla {
    rgb(rgb_hex).into()
}

fn hakuji() -> Arc<EditorPalette> {
    Arc::new(EditorPalette::fastedit_light().clone())
}

#[test]
fn every_kind_has_its_own_glyph() {
    for kind in KINDS {
        assert!(Icon::ALL.contains(&symbol_icon(kind)), "{kind:?}");
    }
    for (i, a) in KINDS.iter().enumerate() {
        for b in &KINDS[i + 1..] {
            if matches!((a, b), (OutlineKind::Heading(_), OutlineKind::Heading(_))) {
                assert_eq!(symbol_icon(*a), symbol_icon(*b), "{a:?} and {b:?}");
            } else {
                assert_ne!(symbol_icon(*a), symbol_icon(*b), "{a:?} and {b:?}");
            }
        }
    }
    let icon = |kind| symbol_icon(kind);
    assert_eq!(icon(OutlineKind::Function), Icon::SquareFunction);
    assert_eq!(icon(OutlineKind::Method), Icon::MathFunction);
    assert_eq!(icon(OutlineKind::Interface), Icon::Plug);
    assert_eq!(icon(OutlineKind::Trait), Icon::Puzzle);
    assert_eq!(icon(OutlineKind::Impl), Icon::Blocks);
    assert_ne!(icon(OutlineKind::Impl), Icon::Link);
    assert_eq!(icon(OutlineKind::Macro), Icon::Wand);
    assert_ne!(icon(OutlineKind::Macro), Icon::Asterisk);
    assert_eq!(icon(OutlineKind::Field), Icon::KeyRound);
    assert_eq!(icon(OutlineKind::Variant), Icon::CircleDot);
}

#[test]
fn tints_follow_yohen_and_hakuji_syntax_colours() {
    let yohen = EditorPalette::fastedit_dark();
    let hakuji = EditorPalette::fastedit_light();
    for palette in [yohen, hakuji] {
        let tint = |kind| symbol_tint(kind, palette);
        for (kinds, name) in [
            (
                &[OutlineKind::Function, OutlineKind::Method][..],
                "function",
            ),
            (
                &[
                    OutlineKind::Struct,
                    OutlineKind::Class,
                    OutlineKind::Trait,
                    OutlineKind::Type,
                ],
                "type",
            ),
            (&[OutlineKind::Constant], "constant"),
            (&[OutlineKind::Macro], "function.macro"),
            (&[OutlineKind::Module], "module"),
        ] {
            for kind in kinds {
                assert_eq!(tint(*kind), Some(listed(palette, name)), "{kind:?}");
            }
        }
    }
    let tints = |palette| {
        [
            OutlineKind::Function,
            OutlineKind::Type,
            OutlineKind::Constant,
        ]
        .map(|kind| symbol_tint(kind, palette).unwrap())
    };
    assert_eq!(tints(yohen), [0xf2c77a, 0x7fc9be, 0xf0a3c2].map(hex));
    assert_eq!(tints(hakuji), [0x925a00, 0x0d6e66, 0xa32f6b].map(hex));
}

#[test]
fn a_kind_the_theme_leaves_uncoloured_has_no_tint() {
    let yohen = EditorPalette::fastedit_dark();
    assert!(!yohen.syntax.iter().any(|(name, _)| name == "variable"));
    assert_eq!(symbol_tint(OutlineKind::Variable, yohen), None);

    // The row given none draws its icon in the list's colour.
    let ui = fe_ui::UiTheme::default_dark();
    let row = fe_ui::TreeRow::new("row", "x", &ui).icon(symbol_icon(OutlineKind::Variable));
    let row = match symbol_tint(OutlineKind::Variable, yohen) {
        Some(tint) => row.icon_color(tint),
        None => row,
    };
    assert_eq!(row.icon_tint(), None);
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

#[gpui::test]
fn outline_rows_take_their_kinds_tint_selected_or_not(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    let src = "struct Point;\n\nfn main() {}\n";
    fs::write(&path, src).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|_, cx| fe_gpui::set_editor_palette(cx, hakuji()));
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-alt-o");
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let outline = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).outline().clone()
    });
    let palette = hakuji();
    let (ty, function) = (listed(&palette, "type"), listed(&palette, "function"));
    let tints = |cx: &mut VisualTestContext| {
        outline.update(cx, |outline, cx| {
            let rows = outline.render_rows(0..2, cx);
            rows.iter().map(|row| row.icon_tint()).collect::<Vec<_>>()
        })
    };
    assert_eq!(tints(cx), [Some(ty), Some(function)]);

    // `main` selected, the outline focused, then the editor.
    let editor = (workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned()))
        .expect("a file is shown");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    let main = src.find("main").unwrap();
    editor.update(cx, |editor, cx| editor.select_and_reveal(main..main, cx));
    draw(cx);
    cx.simulate_keystrokes("cmd-alt-o");
    draw(cx);
    assert_eq!(
        outline.read_with(cx, |outline, _| outline.selected()),
        Some(1)
    );
    assert!(cx.update(|window, cx| outline.focus_handle(cx).is_focused(window)));
    assert_eq!(tints(cx), [Some(ty), Some(function)]);

    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(
        outline.read_with(cx, |outline, _| outline.selected()),
        Some(1)
    );
    assert!(!cx.update(|window, cx| outline.focus_handle(cx).is_focused(window)));
    assert_eq!(tints(cx), [Some(ty), Some(function)]);
}

#[gpui::test]
fn symbol_crumb_icons_take_their_kinds_tint(cx: &mut TestAppContext) {
    let src = "\
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
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    let path = dir.path().join("src/mod.rs");
    fs::write(&path, src).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|_, cx| fe_gpui::set_editor_palette(cx, hakuji()));
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = (workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned()))
        .expect("a file is shown");
    editor.update(cx, |editor, cx| {
        let offset = editor.model().text().to_string().find("let x").unwrap();
        editor.model_mut().set_cursor(offset);
        cx.notify();
    });
    draw(cx);

    let palette = hakuji();
    let crumbs = pane.read_with(cx, |pane, _| {
        let labels = pane.drawn_crumbs().iter().map(|(label, ..)| label.clone());
        labels
            .zip(pane.crumb_icon_colors().iter().copied())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        crumbs[2..],
        [
            ("outer".to_string(), Some(listed(&palette, "module"))),
            ("impl S".to_string(), Some(listed(&palette, "type"))),
            ("b".to_string(), Some(listed(&palette, "function"))),
        ]
    );
    // The path and file crumbs keep the list's colour.
    assert_eq!(
        crumbs[..2]
            .iter()
            .map(|(_, tint)| *tint)
            .collect::<Vec<_>>(),
        [None, None]
    );
}
