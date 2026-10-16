// SPDX-License-Identifier: LicenseRef-Proprietary

//! A pane item with a typed serde state: saved and restored through
//! `layout::save_panes` and `restore_panes`, with a split copy getting its
//! caret through `ItemState::apply`, and a state that doesn't decode
//! dropping its item and no other.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, Render, SharedString, Styled as _, Task, TestAppContext, VisualTestContext,
    Window, div,
};
use kiln_command::Value;
use kiln_core::serde::{Deserialize, Serialize};
use kiln_shell::{
    Direction, ItemPane, PaneGroup, PaneItem, PaneView,
    item::{ItemState, state},
    layout,
};

#[derive(Serialize, Deserialize)]
#[serde(crate = "kiln_core::serde")]
struct SlipState {
    text: String,
    caret: usize,
}

/// A slip of text with a caret; a split shares the text.
struct Slip {
    text: Rc<String>,
    caret: usize,
    focus: FocusHandle,
}

impl Render for Slip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full()
    }
}

impl Focusable for Slip {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ItemState for Slip {
    type State = SlipState;

    fn state(&self, _: &App) -> Option<SlipState> {
        Some(SlipState {
            text: (*self.text).clone(),
            caret: self.caret,
        })
    }

    fn from_state(state: SlipState, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        Task::ready(Some(cx.new(|cx| Slip {
            text: Rc::new(state.text),
            caret: state.caret,
            focus: cx.focus_handle(),
        })))
    }

    fn apply(&mut self, state: &SlipState, _: &mut Window, _: &mut Context<Self>) {
        self.caret = state.caret;
    }
}

impl PaneItem for Slip {
    const KIND: &'static str = "test.slip";

    fn tab_title(&self, _: &App) -> SharedString {
        self.text.as_str().to_owned().into()
    }

    fn shares_document(&self, other: &gpui::AnyView, cx: &App) -> bool {
        (other.clone().downcast::<Slip>())
            .is_ok_and(|other| Rc::ptr_eq(&other.read(cx).text, &self.text))
    }

    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let text = self.text.clone();
        Some(cx.new(|cx| Slip {
            text,
            caret: 0,
            focus: cx.focus_handle(),
        }))
    }

    fn serialize(&self, cx: &App) -> Option<Value> {
        state::serialize(self, cx)
    }

    fn restore(state: Value, window: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        state::restore::<Self>(state, window, cx)
    }

    fn apply_state(&mut self, state: &Value, window: &mut Window, cx: &mut Context<Self>) {
        state::apply(self, state, window, cx)
    }
}

fn slip(text: &str, caret: usize, cx: &mut VisualTestContext) -> Entity<Slip> {
    let text = Rc::new(text.to_owned());
    cx.new(|cx| Slip {
        text,
        caret,
        focus: cx.focus_handle(),
    })
}

/// `groups`' panes saved as one group: the first pane, the others split
/// to its right.
fn save(panes: Vec<Vec<Entity<Slip>>>, cx: &mut VisualTestContext) -> Value {
    let mut made: Vec<Entity<ItemPane>> = Vec::new();
    let mut group: Option<PaneGroup<ItemPane>> = None;
    for items in panes {
        let pane = cx.new(ItemPane::new);
        pane.update(cx, |pane, cx| {
            for item in items {
                pane.add_item(Box::new(item), cx);
            }
        });
        match &mut group {
            None => group = Some(PaneGroup::new(pane.clone())),
            Some(group) => group.split(&made[0], pane.clone(), Direction::Right),
        }
        made.push(pane);
    }
    let group = group.unwrap();
    cx.update(|_, cx| layout::save_panes(&group, &made[0], cx))
}

/// The slips `saved` restores to, per pane, as (text, caret).
fn restore(saved: &Value, cx: &mut VisualTestContext) -> Vec<Vec<(String, usize)>> {
    let restored = cx.update(|window, cx| {
        let mut new_pane = |_: &mut Window, cx: &mut App| cx.new(ItemPane::new);
        layout::restore_panes(saved, &mut new_pane, window, cx).unwrap()
    });
    let mut group = PaneGroup::new(restored.active.clone());
    group.set_root(restored.root);
    let filled = restored.filled;
    cx.spawn(async move |_| {
        filled.await;
    })
    .detach();
    cx.run_until_parked();
    group
        .panes()
        .into_iter()
        .map(|pane| {
            pane.read_with(cx, |pane, cx| {
                (pane.pane().items().iter())
                    .map(|item| {
                        let slip = item.downcast::<Slip>().unwrap();
                        let slip = slip.read(cx);
                        ((*slip.text).clone(), slip.caret)
                    })
                    .collect()
            })
        })
        .collect()
}

fn text_and_caret(text: &str, caret: usize) -> (String, usize) {
    (text.to_owned(), caret)
}

#[gpui::test]
fn typed_state_round_trips_through_a_saved_layout(cx: &mut TestAppContext) {
    cx.update(layout::register_item::<Slip>);
    let (_, cx) = cx.add_window_view(|_, _| gpui::EmptyView);
    let saved = save(vec![vec![slip("one", 4, cx), slip("two", 7, cx)]], cx);
    assert_eq!(
        restore(&saved, cx),
        [[text_and_caret("one", 4), text_and_caret("two", 7)]]
    );
}

#[gpui::test]
fn a_state_that_does_not_decode_drops_only_its_item(cx: &mut TestAppContext) {
    cx.update(layout::register_item::<Slip>);
    let (_, cx) = cx.add_window_view(|_, _| gpui::EmptyView);
    let mut saved = save(
        vec![vec![
            slip("one", 1, cx),
            slip("bad", 2, cx),
            slip("two", 3, cx),
        ]],
        cx,
    );
    // Breaks the state of the item whose text is "bad".
    fn corrupt(value: &mut Value) -> bool {
        match value {
            Value::Object(object) => {
                if object.get("state").and_then(|state| state["text"].as_str()) == Some("bad") {
                    object.insert("state".into(), Value::from_iter([("text", 5)]));
                    return true;
                }
                object.values_mut().any(corrupt)
            }
            Value::Array(items) => items.iter_mut().any(corrupt),
            _ => false,
        }
    }
    assert!(corrupt(&mut saved));
    assert_eq!(
        restore(&saved, cx),
        [[text_and_caret("one", 1), text_and_caret("two", 3)]]
    );
}

#[gpui::test]
fn apply_reaches_the_copy_a_split_made_while_restoring(cx: &mut TestAppContext) {
    cx.update(layout::register_item::<Slip>);
    let (_, cx) = cx.add_window_view(|_, _| gpui::EmptyView);
    let a = slip("a", 1, cx);
    let copy = cx.update(|window, cx| a.update(cx, |a, cx| a.split(window, cx)).unwrap());
    copy.update(cx, |copy, _| copy.caret = 3);
    let saved = save(vec![vec![a], vec![copy]], cx);
    assert_eq!(
        restore(&saved, cx),
        [[text_and_caret("a", 1)], [text_and_caret("a", 3)]]
    );
}
