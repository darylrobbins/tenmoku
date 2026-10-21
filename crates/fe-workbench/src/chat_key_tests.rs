//! The chat's Set API Key: the view's button opens a masked form, which
//! stores the key in the keychain (a `MemoryStore` here), and
//! `chat.clear_api_key` deletes it.

use std::sync::Arc;

use fe_chat_view::ChatViewEvent;
use kiln::core::secrets::{MemoryStore, Secret, SecretError, SecretStore};

use super::{ChatPanel, KeyOrigin, chat, dispatch, last_key, opened, scripted, set_source};
use crate::Workbench;
use gpui::{Entity, Focusable as _, VisualTestContext};

const SERVICE: &str = "dev.fastedit.fastedit";
const ACCOUNT: &str = "chat.anthropic";

/// A store whose writes fail.
struct Failing;

impl SecretStore for Failing {
    fn get(&self, _: &str, _: &str) -> Result<Option<Secret>, SecretError> {
        Ok(None)
    }

    fn set(&self, _: &str, _: &str, _: &str) -> Result<(), SecretError> {
        Err(SecretError::Other("the keychain is locked".into()))
    }

    fn delete(&self, _: &str, _: &str) -> Result<(), SecretError> {
        Ok(())
    }
}

/// What the window's shown toasts say, top first.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

/// Has the view ask for a key, as its button does.
fn request_key(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |_, cx| cx.emit(ChatViewEvent::SetKeyRequested));
    cx.run_until_parked();
}

fn origin(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> Option<KeyOrigin> {
    panel.read_with(cx, |panel, _| panel.key_origin())
}

fn stored(store: &MemoryStore) -> Option<String> {
    (store.get(SERVICE, ACCOUNT).unwrap()).map(Secret::into_string)
}

#[gpui::test]
fn the_set_api_key_button_stores_the_key_and_the_chat_uses_it(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let made = scripted(cx);
    let store = Arc::new(MemoryStore::new());
    set_source(None, &store, cx);
    dispatch("chat.open", cx);
    let panel = chat(&workbench, cx).expect("made");
    assert_eq!(origin(&panel, cx), None);
    assert!(cx.debug_bounds("chat-no-key").is_some());

    request_key(&panel, cx);
    assert!(cx.debug_bounds("chat-key-form").is_some());
    cx.simulate_input("  sk-ant-test  ");
    // Not shown, not stored yet.
    assert_eq!(stored(&store), None);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    assert_eq!(stored(&store).as_deref(), Some("sk-ant-test"));
    assert_eq!(origin(&panel, cx), Some(KeyOrigin::Keychain));
    assert_eq!(last_key(&made).as_deref(), Some("sk-ant-test"));
    assert!(cx.debug_bounds("chat-key-form").is_none());
    assert!(cx.debug_bounds("chat-no-key").is_none());
}

#[gpui::test]
fn clearing_the_key_returns_to_no_key(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    scripted(cx);
    let store = Arc::new(MemoryStore::new());
    set_source(None, &store, cx);
    dispatch("chat.open", cx);
    let panel = chat(&workbench, cx).expect("made");
    request_key(&panel, cx);
    cx.simulate_input("sk-ant-test");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(origin(&panel, cx), Some(KeyOrigin::Keychain));

    dispatch("chat.clear_api_key", cx);
    cx.run_until_parked();
    assert_eq!(stored(&store), None);
    assert_eq!(origin(&panel, cx), None);
    assert!(cx.debug_bounds("chat-no-key").is_some());
}

#[gpui::test]
fn cancel_leaves_the_store_alone(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    scripted(cx);
    let store = Arc::new(MemoryStore::new());
    set_source(None, &store, cx);
    dispatch("chat.set_api_key", cx);
    let panel = chat(&workbench, cx).expect("made by the command");
    assert!(cx.debug_bounds("chat-key-form").is_some());
    cx.simulate_input("sk-ant-test");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    assert_eq!(stored(&store), None);
    assert_eq!(origin(&panel, cx), None);
    assert!(cx.debug_bounds("chat-key-form").is_none());
    let focused = cx.update(|window, cx| {
        let view = panel.read(cx).view().read(cx);
        view.composer().focus_handle(cx).is_focused(window)
    });
    assert!(focused, "the composer has focus");
}

#[gpui::test]
fn a_store_error_keeps_the_form(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    scripted(cx);
    cx.update(|_, cx| {
        cx.set_global(super::KeySource {
            env: None,
            store: Arc::new(Failing),
        });
    });
    dispatch("chat.open", cx);
    let panel = chat(&workbench, cx).expect("made");
    request_key(&panel, cx);
    cx.simulate_input("sk-ant-test");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let shown = toasts(&workbench, cx);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert!(shown[0].contains("the keychain is locked"), "{shown:?}");
    assert!(cx.debug_bounds("chat-key-form").is_some());
    assert_eq!(origin(&panel, cx), None);
    // The text is still there: save it again once the store works.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(toasts(&workbench, cx).len(), 1, "the same toast, once more");
}
