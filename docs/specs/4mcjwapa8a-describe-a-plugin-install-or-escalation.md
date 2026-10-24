---
task_uid: "01m5nd04g4sy96wb4mcjwapa8a"
task: "T301"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "warnings::read_plus_net_warns_it_can_send_your_code: buffer.read with net:api.example.com says it can read your code and send it to api.example.com"
    required: true
    manual: false
  - id: "C2"
    text: "warnings::spawn_names_the_programs_outside_the_sandbox: process.spawn:rg says it runs rg outside the sandbox"
    required: true
    manual: false
  - id: "C3"
    text: "warnings::interpreters_are_flagged_near_wildcard: process.spawn:sh and process.spawn:python3 carry the near-wildcard warning"
    required: true
    manual: false
  - id: "C4"
    text: "warnings::wildcards_are_extreme_and_full_access: net:* and process.spawn:* each give their extreme text and is_full_access is true; a scoped set is false"
    required: true
    manual: false
  - id: "C5"
    text: "warnings::a_benign_set_has_no_warning: buffer.read, commands and status give none"
    required: true
    manual: false
  - id: "C6"
    text: "plugin_prompt_tests::prompt_text_lists_every_capability_and_warning: plugin_prompt_text names the plugin, id, version, licence, directory, each capability and each warning"
    required: true
    manual: false
  - id: "C7"
    text: "plugin_prompt_tests::an_escalation_says_what_was_added_and_that_the_old_version_keeps_running: the detail names the added capabilities and the installed version still running"
    required: true
    manual: false
  - id: "C8"
    text: "plugin_prompt_tests::a_wildcard_needs_a_second_acceptance: Allow alone does not grant net:*; only Allow Full Access after the second prompt does"
    required: true
    manual: false
  - id: "C9"
    text: "plugin_prompt_tests::dont_allow_declines_and_closing_leaves_it_pending: the answers map to Declined and Pending"
    required: true
    manual: false
  - id: "C10"
    text: "cargo nextest run -p kiln-plugin -p fe-workbench and cargo clippy -p kiln-plugin -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Describe a plugin install or escalation in plain words with combination and wildcard warnings, and prompt for it with a separate full-access acceptance (Mac T1061)

## Problem

The install prompt must say what a plugin can do in plain words, not only list capabilities (decision 4), flag
`net:*` and `process.spawn:*` with an extreme warning and their own acceptance step (decision 2), and
describe an escalation as what was added while the old version keeps running (decision 3). A plugin placed in the user's
extensions directory loads only after the user answers.

## Change

- `kiln/crates/kiln-plugin/src/warnings.rs`: `combination_warnings(&Grants) -> Vec<Warning>`: `buffer.read` or `fs.read:*`
  with any `net` grant reads "can read your code and send it to <hosts>"; any `process.spawn` grant reads "runs
  <programs> outside the sandbox"; an interpreter (`sh`, `bash`, `zsh`, `fish`, `dash`, `node`, `python`, `python3`,
  `deno`, `bun`, `ruby`, `perl`) is "near-wildcard: it can run any code"; `net:*` is "can send your code anywhere";
  `process.spawn:*` is "can run any program as you, outside the sandbox"; and `is_full_access(&Grants)` is true
  when either wildcard is held (the list marks such plugins). Plain functions, no UI.
- `crates/fe-workbench/src/plugin_prompt.rs` and `plugin_prompt_tests.rs` (a `mod` line in `lib.rs`), modelled on T1038's
  `lsp_consent_prompt`: a pure `plugin_prompt_text(&PendingPluginGrant) -> (String, String)` (plugin name, id, version,
  licence, source directory, every capability with its scope, the warnings, and for an escalation what was added and that
  the installed version stays loaded), and `show_plugin_prompt(window, &PendingPluginGrant, cx)` using `window.prompt`:
  an ordinary "Allow" / "Don't Allow", and, when a wildcard was asked for, a second prompt after Allow that only a
  separate "Allow Full Access" answer grants. Answers are returned as a `PluginAnswer` value; recording them in the
  `GrantStore` and hooking a pending-grants source into a window is the app wiring card (P19), not this one.

## Notes

The UI half needs gpui, so only the model tests (`warnings`) are Linux-OK. Do not edit `workbench.rs`.

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new files; the scope lists exact files.

Lend note (helium lead): full JSON from the Mac (y55). Mac deps T1038 and T1040 (= helium T287) are on trunk.

## Implementation notes

- Revision 2 adds `crates/fe-workbench/Cargo.toml` and `Cargo.lock` to the scope: fe-workbench didn't depend on
  `kiln-plugin`, and the prompt takes its warnings and `needs_wildcard_acceptance` from it. wasmtime was already in
  fe-workbench's tree through tree-sitter's `wasm` feature, so the new edge adds only `wasmtime-wasi` and its deps.
- `warnings::describe(&Capability)` gives each capability's plain words for the prompt's list. The interpreter list
  is the spec's plus the LSP consent's (`ksh`, `nodejs`, `npx`, `uvx`, `php`, `lua`, `java`, `env`, …) and a few
  launchers (`xargs`, `sudo`, `busybox`). It matches without case or a version suffix (`python3.12`, `node22`).
- `net:*` stands in for named hosts in the warnings, and `process.spawn:*` for named programs and interpreters.
- `PendingPluginGrant.ask` is `Install` or `Escalation { added, keep_running }`, as `grants::evaluate` decides. Only
  what the request adds triggers the full-access prompt, as `GrantStore::grant` checks it: a wildcard accepted
  before doesn't ask again. The warnings cover the whole request, since old grants combine with added ones.
- `show_plugin_prompt` returns `Task<PluginAnswer>`. `Allowed { accept_wildcards }` maps onto `GrantStore::grant`'s
  flag. An answer that arrives after the window closed is `Pending`, at either prompt.
