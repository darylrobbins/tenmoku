// SPDX-License-Identifier: LicenseRef-Proprietary

//! A masked TextField: bullets drawn for the text, the clipboard left
//! alone, everything else as in a plain field.

use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, Focusable as _, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window, div, px,
};

use crate::TextField;

struct Host {
    field: Entity<TextField>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(200.)).child(self.field.clone())
    }
}

fn open(cx: &mut TestAppContext) -> (Entity<TextField>, &mut VisualTestContext) {
    cx.update(|cx| cx.bind_keys(TextField::key_bindings(false)));
    let (host, cx) = cx.add_window_view(|window, cx| Host {
        field: cx.new(|cx| TextField::new(window, cx).masked(true)),
    });
    let field = host.read_with(cx, |host, _| host.field.clone());
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&field.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (field, cx)
}

fn drawn(field: &Entity<TextField>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    field.read_with(cx, |field, _| {
        field.drawn_text().map(|text| text.to_string())
    })
}

fn value(field: &Entity<TextField>, cx: &mut VisualTestContext) -> String {
    field.read_with(cx, |field, _| field.value().to_string())
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

#[gpui::test]
fn masked_field_hides_its_text(cx: &mut TestAppContext) {
    let (field, cx) = open(cx);
    cx.write_to_clipboard(ClipboardItem::new_string("before".into()));
    cx.simulate_input("sk-ant-x");
    assert_eq!(value(&field, cx), "sk-ant-x");
    assert_eq!(drawn(&field, cx).as_deref(), Some("••••••••"));

    // One bullet a grapheme, not a code point or a byte.
    cx.simulate_input("e\u{301}👍🏽");
    assert_eq!(drawn(&field, cx).as_deref(), Some("••••••••••"));

    // Copy and cut put nothing on the clipboard, and cut keeps the text.
    cx.simulate_keystrokes("secondary-a secondary-c");
    assert_eq!(clipboard(cx).as_deref(), Some("before"));
    cx.simulate_keystrokes("secondary-x");
    assert_eq!(clipboard(cx).as_deref(), Some("before"));
    assert_eq!(value(&field, cx), "sk-ant-xe\u{301}👍🏽");

    // Paste replaces the selection, and undo brings the text back.
    cx.write_to_clipboard(ClipboardItem::new_string("pasted".into()));
    cx.simulate_keystrokes("secondary-v");
    assert_eq!(value(&field, cx), "pasted");
    assert_eq!(drawn(&field, cx).as_deref(), Some("••••••"));
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(value(&field, cx), "sk-ant-xe\u{301}👍🏽");

    // The caret moves by graphemes of the real text.
    cx.simulate_keystrokes("end left left");
    cx.simulate_input("!");
    assert_eq!(value(&field, cx), "sk-ant-x!e\u{301}👍🏽");
    assert_eq!(drawn(&field, cx).unwrap().chars().count(), 11);
}
