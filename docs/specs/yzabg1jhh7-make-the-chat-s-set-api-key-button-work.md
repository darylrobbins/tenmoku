---
task_uid: "01m5f775dc0t150ayzabg1jhh7"
task: "T1021"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-ui test text_field::masked_tests::masked_field_hides_its_text (declared from src/text_field.rs by a #[cfg(test)] #[path = \"tests/text_field_masked.rs\"] mod line, not from src/tests/mod.rs, so the base overlay doesn't compile it): a masked field holding sk-ant-x draws one bullet per grapheme and no other text; copy and cut leave the clipboard unchanged; paste and undo work; value() is the text"
    required: true
    manual: false
  - id: "C2"
    text: "Regression chat_panel::chat_panel_tests::set_key::the_set_api_key_button_stores_the_key_and_the_chat_uses_it fails at base and passes: SetKeyRequested opens the focused masked form; typing '  sk-ant-test  ' + enter stores sk-ant-test under the app id / chat.anthropic, key_origin() is Keychain and the provider gets that key"
    required: true
    manual: false
  - id: "C3"
    text: "Tests set_key::clearing_the_key_returns_to_no_key, set_key::cancel_leaves_the_store_alone and set_key::a_store_error_keeps_the_form pass: clear empties the store and key_origin() is None; escape stores nothing and focuses the composer; a failing set shows a toast and keeps the text"
    required: true
    manual: false
  - id: "C4"
    text: "crates/fe-chat-view/src/view.rs is unchanged; chat.set_api_key and chat.clear_api_key are typed commands listed in the palette"
    required: true
    manual: false
---

# Make the chat's Set API Key button work: a masked key form that stores the key in the Keychain, and chat.clear_api_key

# Make the chat's Set API Key button work: a masked key form that stores the key in the Keychain, and chat.clear_api_key

## Problem

The user (2026-10-21): "The set Anthropic API key button doesn't work." The button is fe-chat-view's
"Set API Key" (crates/fe-chat-view/src/view.rs:766, and the banner action at :100/:467). It emits
`ChatViewEvent::SetKeyRequested`; `ChatPanel::view_event` (crates/fe-workbench/src/chat_panel.rs:~407)
runs `run_command("chat.set_api_key")`, which finds no typed command of that id and only logs
`chat: chat.set_api_key isn't available yet` (chat_panel.rs:435). The command was left to T972, which is
queued on helium behind T969-T971. So the only way to give the chat a key is `ANTHROPIC_API_KEY` or a
manual `security add-generic-password`.

This card moves T972's key half (its C1 masked field and C3 set/clear commands) forward; T972 keeps its
keys, menu item and Ask About Selection.

## Change

- **`TextField::masked(bool)`** (builder) and `set_masked` in kiln/crates/kiln-ui/src/text_field.rs: draws
  each grapheme as `•`; copy and cut put nothing on the clipboard; select, paste, IME and undo work as
  before; `value()` is the real text. Tests in a new kiln/crates/kiln-ui/src/tests/text_field_masked.rs,
  declared in kiln/crates/kiln-ui/src/tests/mod.rs.
- **`chat.set_api_key`** ("Chat: Set API Key…"), a `#[kiln::command]` in chat_panel.rs, context
  `workbench` with the same propagate-to-`init` listener `chat.open` uses: shows the chat panel with a key
  form in place of the chat view: a masked `TextField` (focused), Save and Cancel buttons, and a line
  saying the key is kept in the system keychain and only sent to api.anthropic.com. Enter in the field
  saves, Escape cancels. Save trims the text; empty does nothing. It stores the key with the panel's
  `KeySource` store (service `key_service()`, account `KEY_ACCOUNT`) on the background executor, then
  closes the form, clears the field and calls `reload_key`, so the view leaves its no-key state. A store
  error is a toast naming it, and the form stays with its text. Cancel closes the form and focuses the
  composer. With `ANTHROPIC_API_KEY` set, the stored key is saved but the environment's still wins, and
  the form says so.
- **`chat.clear_api_key`** ("Chat: Clear API Key"): deletes the entry and reloads; the view returns to
  its no-key state unless `ANTHROPIC_API_KEY` is set.
- The form lives in chat_panel.rs (the panel draws it instead of `self.view`), not in fe-chat-view, so
  this card doesn't touch crates/fe-chat-view/src/view.rs (T969-T972 do).

## Tests

New file crates/fe-workbench/src/chat_key_tests.rs, declared at the end of
crates/fe-workbench/src/chat_panel_tests.rs by `#[path = "chat_key_tests.rs"] mod set_key;`. It must
compile against base API (Regatta overlays it at base): use only the parent's helpers (`opened`,
`dispatch`, `chat`, `scripted`, `last_key`, `set_source`), `ChatViewEvent::SetKeyRequested` emitted on
the panel's `view()`, `simulate_input`, `simulate_keystrokes`, `MemoryStore` and
`ChatPanel::key_origin`, nothing this card adds.

- `the_set_api_key_button_stores_the_key_and_the_chat_uses_it` (regression): with no env key and an
  empty `MemoryStore`, open the chat, emit `SetKeyRequested` from the view, `run_until_parked`, type
  `  sk-ant-test  ` and press enter, park: the store holds `sk-ant-test` under the app id's service and
  `chat.anthropic`, `key_origin()` is `Some(Keychain)`, and the provider factory was last given
  `sk-ant-test`. Fails at base: the event only logs, so nothing is stored.
- `clearing_the_key_returns_to_no_key`: after the above, `chat.clear_api_key` empties the store and
  `key_origin()` is `None`.
- `cancel_leaves_the_store_alone`: the form, typed text, escape: nothing stored, composer focused.
- `a_store_error_keeps_the_form`: a store (defined in the test file) whose `set` fails: a toast names the
  error and the field keeps its text.
