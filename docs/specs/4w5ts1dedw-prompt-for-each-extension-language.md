---
task_uid: "01m5ffwqg0sbsn724w5ts1dedw"
task: "T1038"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "lsp_consent_prompt_tests::consent_prompt_text_shows_the_effective_invocation: consent_prompt_text names the extension id, version and licence, the server, the resolved executable path, every argument and each NAME=value env var; an interpreter adds the arbitrary-code line; a changed invocation shows the previous one and 'changed what it runs'"
    required: true
    manual: false
  - id: "C2"
    text: "lsp_consent_prompt_tests::a_pending_consent_prompts_the_active_window_once: with a pending consent, an active window shows one pending_prompt; a second window doesn't ask for the same one"
    required: true
    manual: false
  - id: "C3"
    text: "lsp_consent_prompt_tests::allow_grants_and_dont_allow_declines: answering Allow makes extension_servers(cx) include the server; Don't Allow leaves it out and doesn't prompt again until the invocation changes, when it does"
    required: true
    manual: false
  - id: "C4"
    text: "lsp_consent_prompt_tests::closing_the_window_leaves_it_pending: a prompt dismissed by closing the window leaves the consent pending and the next active window asks"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Prompt for each extension language server waiting on consent, showing its resolved command, arguments, environment and any interpreter or changed-script warning

## Problem

The consent gate for extension language servers (the card before this one) withholds a server until
the user grants it, but nothing asks. Extensions record §1: an `lsp` contribution is shown as its
effective invocation (resolved absolute executable, full argument vector, env), an interpreter as the
executable is flagged as able to run arbitrary code, and a changed specification asks again.

## Change

New `crates/fe-workbench/src/lsp_consent_prompt.rs` (with `#[cfg(test)] mod lsp_consent_prompt_tests;`
and the new file `crates/fe-workbench/src/lsp_consent_prompt_tests.rs`), hooked in `workbench.rs`:

- A workbench follows the pending-consent notification (`pending_extension_consents`, from the gate)
  and, when its window is active, shows one native prompt at a time with `window.prompt(
  PromptLevel::Warning, ..)` as `disk_sync` does (tests use `VisualTestContext::pending_prompt`).
  Windows don't ask twice: an answered or answering consent isn't offered to another window.
- Question: "Allow <extension name> to run a language server?". Detail: extension id, version,
  licence, source directory; the server's name and languages; the executable's resolved absolute path,
  every argument, and each env var as `NAME=value`; for a changed invocation, the previous one and
  "This extension changed what it runs."; for an interpreter, "<program> can run any code in its
  arguments." and, when a script file in the extension is hashed, its path. Buttons "Allow" and "Don't
  Allow" (the latter's text says it isn't asked again until the command changes).
- Allow calls `grant_extension_server`, Don't Allow `decline_extension_server`; closing the prompt
  without a choice (window closed) leaves it pending, asked again next time a window is active.
- Prompt text is built by a pure function `consent_prompt_text(&PendingConsent) -> (String, String)`
  so tests assert on it.

## Out of scope

The extensions list (next card); consent for built-in or user-configured servers; macOS-only visual
checks of the native prompt (a `Check ... in the running app` card, if wanted, is separate).
