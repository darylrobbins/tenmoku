---
task_uid: "01m6j07wtxszzr46n32ss3vzvt"
task: "T433"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "register_names_languages_and_a_command: a plugin with buffer.read and buffer.write registers its own command for languages and Formatters lists it for each; registering again replaces it; another plugin command is invalid-argument (crates/fe-plugin/tests/formatting.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "a_plugin_without_buffer_write_cannot_register: register is denied and nothing is listed (crates/fe-plugin/tests/formatting.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "a_format_request_returns_the_answered_edits: request runs the command with the request, buffer, version and language and returns the edits the plugin answered; no answer is unchanged (crates/fe-plugin/tests/formatting.rs)"
    required: true
    manual: false
  - id: "C4"
    text: "a_bad_answer_or_an_overrun_fails_the_request: unordered or off-boundary edits are invalid-argument and fail the request; a command past the call deadline unloads the plugin and the request fails with a timeout (crates/fe-plugin/tests/formatting.rs)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-plugin -p tenmoku-plugin and cargo clippy -p fe-plugin -p tenmoku-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Add the fastedit:editor formatting import: a plugin registers a formatter command for languages and answers a format request with edits against the requested version (Mac T1354)

## Problem

A formatter written as a plugin has no way to offer itself to Format Document.

## Change

- `crates/fe-plugin/wit/formatting.wit`, interface `formatting` in `fastedit:editor@1.0.0`
  (imported by `editor-extension`):
  - `register: func(languages: list<string>, command: command-id) -> result<_, error>`: the plugin's
    own command (`<id>.<name>`, one it registered) formats those languages. Registering again
    replaces this plugin's entry for them. Needs `buffer.read` and `buffer.write`.
  - `answer: func(request: u64, edits: list<text-edit>) -> result<_, error>`: the reply to a
    request, edits as for `buffer.edit` (in order, apart, on char boundaries) against the requested
    version; anything else is `invalid-argument` and fails the request.
  The host asks by running the registered command with args
  `{"request": n, "buffer": id, "version": v, "language": "<name>"}`; the command reads the text with
  `buffer.read`, calls `answer` and returns. Returning without answering means "unchanged". An
  import rather than a new export, so components without a formatter need no stub and old guests keep
  loading.
- `crates/fe-plugin/src/formatting.rs` (+ `HostState` slice and `add_to_linker` in `lib.rs`): a
  shared `Formatters` registry (language to plugin and command; `unload(plugin)`) and
  `request(buffer, version, language) -> Result<Option<Vec<TextEdit>>, FormatError>` that runs the
  command on the plugin's thread under its call deadline (an overrun unloads the plugin, as for any
  call, and fails the request with a timeout).
- The test guest gains `register-formatter` and `format` commands (`format` upper-cases the text, say);
  rebuild `editor.wasm`.
- `crates/tenmoku-plugin/src/lib.rs`: re-export `formatting`.

## Notes

The app wiring (Format Document, format on save) is the follow-up card on T1326's seam. Rebuilding
the guest needs wasm32-wasip2 (helium). Host-independent.

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.
