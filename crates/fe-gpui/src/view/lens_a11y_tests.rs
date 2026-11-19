//! Lens items as screen-reader buttons (T1964): each item in view with an
//! invocation is a Button named for its title, whose Click runs it as a
//! pointer click does; plain text is a Label.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, BufferSnapshot};
use fe_command::{Dispatched, Effect, Invocation, Source};
use fe_editor::EditorModel;
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, TestAppContext, VisualTestContext,
    accesskit::Action, px, size,
};

use crate::{
    EditorTheme, EditorView, Lens, LensItem, LensProvider, commands::observe_invocations, init,
};

/// The rows with a lens: one in view, one far below it.
const SHOWN: u32 = 2;
const BELOW: u32 = 250;

/// Over [`SHOWN`] and [`BELOW`]: two items with commands and plain text.
struct Fixed;

impl LensProvider for Fixed {
    fn lenses(&mut self, _: &BufferSnapshot) -> Vec<Lens> {
        let item = |title: &'static str, command: Option<&str>| LensItem {
            title: title.into(),
            warning: false,
            invocation: command.map(|command| Invocation::of(command).unwrap()),
        };
        [SHOWN, BELOW]
            .map(|row| Lens {
                row,
                items: vec![
                    item("\u{25b7} Run test", Some("test.lens_run")),
                    item("Debug", Some("test.lens_debug")),
                    item("passed 2 times", None),
                ],
            })
            .into()
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// An 800×400 editor on 300 rows, with [`Fixed`]'s lenses.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let text = (0..300)
        .map(|n| format!("    line {n}\n"))
        .collect::<String>();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.add_lens_provider(Rc::new(RefCell::new(Fixed)));
        view
    });
    cx.simulate_resize(size(px(800.), px(400.)));
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    draw(cx);
    (view, cx)
}

/// The recorded node with `id`: its role and label.
fn node(id: &gpui::ElementId, cx: &mut VisualTestContext) -> Option<(fe_ui::Role, String)> {
    let nodes = cx.update(|window, cx| fe_ui::nodes(window, cx));
    (nodes.into_iter())
        .find(|node| node.id == *id)
        .map(|node| (node.role, node.label.to_string()))
}

/// The a11y actions recorded for `id`.
fn actions(id: &gpui::ElementId, cx: &mut VisualTestContext) -> Vec<Action> {
    let registered = cx.update(|window, cx| fe_ui::actions(window, cx));
    (registered.into_iter())
        .find(|node| node.id == *id)
        .map(|node| node.actions)
        .unwrap_or_default()
}

/// Each invocation of [`Fixed`]'s commands run from here on.
fn observe(cx: &mut VisualTestContext) -> Rc<RefCell<Vec<Invocation>>> {
    cx.update(|_, cx| {
        kiln_platform::commands::register_app_commands(cx, |registry| {
            for id in ["test.lens_run", "test.lens_debug"] {
                let ran = |_: &mut &mut gpui::App, _: &_| Ok(Dispatched::new(Effect::Nothing));
                registry.register(id, id, ran).unwrap();
            }
        });
    });
    let seen = Rc::new(RefCell::new(Vec::<Invocation>::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        })
    });
    seen
}

#[gpui::test]
fn a_lens_item_is_a_named_button(cx: &mut TestAppContext) {
    let (_view, cx) = editor(cx);
    let id = EditorView::lens_item_id;
    assert_eq!(
        node(&id(SHOWN, 0), cx),
        Some((fe_ui::Role::Button, "Run test".into()))
    );
    assert_eq!(
        node(&id(SHOWN, 1), cx),
        Some((fe_ui::Role::Button, "Debug".into()))
    );
    assert_eq!(actions(&id(SHOWN, 0), cx), [Action::Click]);
    assert_eq!(actions(&id(SHOWN, 1), cx), [Action::Click]);
    // Plain text is read, not pressed.
    assert_eq!(
        node(&id(SHOWN, 2), cx),
        Some((fe_ui::Role::Label, "passed 2 times".into()))
    );
    assert_eq!(actions(&id(SHOWN, 2), cx), []);
    // Out of view, no node.
    for index in 0..3 {
        assert_eq!(node(&id(BELOW, index), cx), None);
        assert_eq!(actions(&id(BELOW, index), cx), []);
    }
}

#[gpui::test]
fn a_screen_readers_click_runs_the_lens_command(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let seen = observe(cx);
    let debug = EditorView::lens_item_id(SHOWN, 1);
    let performed = cx.update(|window, cx| fe_ui::perform(window, &debug, Action::Click, cx));
    assert!(performed);
    cx.run_until_parked();
    let ran = std::mem::take(&mut *seen.borrow_mut());
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert_eq!(ran[0].command.as_str(), "test.lens_debug");
    assert_eq!(ran[0].source, Source::Mouse);

    // The pointer still reaches the item through the layer, once.
    draw(cx);
    let rows = view.read_with(cx, |view, _| view.lens_rows.clone());
    let at = rows[0].items[0].1.center();
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let ran = std::mem::take(&mut *seen.borrow_mut());
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert_eq!(ran[0].command.as_str(), "test.lens_run");
}

#[test]
fn a_leading_glyph_is_dropped_from_the_name() {
    let name = |title: &'static str| crate::lens::accessible_name(&title.into()).to_string();
    assert_eq!(name("\u{25b7} Run test"), "Run test");
    assert_eq!(name("\u{25b6}\u{fe0e} Run"), "Run");
    assert_eq!(name("3 references"), "3 references");
    assert_eq!(name("$ cargo"), "$ cargo");
    assert_eq!(name("\u{25b7}"), "\u{25b7}");
    assert_eq!(name("\u{e9}t\u{e9}"), "\u{e9}t\u{e9}");
}
