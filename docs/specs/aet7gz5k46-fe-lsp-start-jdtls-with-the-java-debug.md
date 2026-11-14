---
task_uid: "01m7cafe870axp6baet7gz5k46"
task: "T644"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test tests::java_debug::jdtls_initialization_options_carry_the_hosts_bundles passes (cargo nextest run -p fe-lsp java_debug): the bundle paths the host gives are merged into jdtls's initializationOptions.bundles, after any the user's [lsp.servers.jdtls] initialization_options list, without replacing them"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::java_debug::execute_command_returns_the_servers_reply passes against the fake server: workspace/executeCommand vscode.java.startDebugSession answers a port, and resolveMainClass/resolveClasspath replies come back as JSON"
    required: true
    manual: false
  - id: "C3"
    text: "Test tests::java_debug::execute_command_refuses_a_command_the_server_did_not_advertise passes (executeCommandProvider.commands), without a request"
    required: true
    manual: false
  - id: "C4"
    text: "Test tests::java_debug::new_bundles_apply_on_the_next_jdtls_start passes, with what the host is told recorded in the decision file"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-lsp passes"
    required: true
    manual: false
---

# fe-lsp: start jdtls with the java-debug bundle and a public workspace/executeCommand (debugger D21) (Mac T1857)

# fe-lsp: start jdtls with the java-debug bundle and a public workspace/executeCommand (debugger D21)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **m**.

## Problem

Java joins the MVP (§13 "Scope"): java-debug runs **inside jdtls** as a bundle, and a
client asks jdtls over LSP for the adapter's port. jdtls has been built in since
Mac T1658 (`crates/fe-lsp/src/registry.rs`, `docs/decisions/cydayg0n57-java-kotlin-servers.md`),
but fe-lsp can neither load bundles into it nor send `workspace/executeCommand` from
a host (`code_action.rs`'s `execute_command` is crate-private).

## What to build (§13 Java)

- **Bundles**: a way for the host to give jdtls's spec extra `bundles` (the
  java-debug plugin jar, `com.microsoft.java.debug.plugin-*.jar`), merged into its
  `initializationOptions` with the user's own.
- **A public `workspace/executeCommand`** on the manager for a root's server: the
  command, JSON arguments, a future of the JSON reply, refused locally when the server
  didn't advertise the command. D21b uses it for `vscode.java.startDebugSession`,
  `vscode.java.resolveMainClass` and `vscode.java.resolveClasspath`.
- The fake server answers `executeCommand` by script.

Split from §13's D21 so the fe-lsp change lands on its own; D21b is the debugger side.

## Notes

`registry.rs` is also in Mac T1742's scope and `lib.rs` in T1710's; rebase over them.

## Not in this card

Finding the jar, the java adapter spec, detection and launching (D21b).

## Conventions for every debugger card (§13)

- **Strings.** Every user-visible string goes through the catalogue
  (`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4-5): `tr!` / `text!`, whole
  messages, plural selectors for counts. Workbench keys are `workbench-debug-…` in
  `crates/fe-workbench/l10n/en-US/debug.ftl` (the first card that needs the file
  adds it to `resources!` in `src/l10n.rs`); fe-dap's are in
  `crates/fe-dap/l10n/en-US/debug.ftl`; fe-gpui's in `crates/fe-gpui/l10n/en-US/debug.ftl`.
  A new command's title is the key `command-<id>` in the same file. Tooltips and
  accessible names count. Tests build expected text through the catalogue.
- **Accessibility.** Every control this card adds has an accessible name (an
  icon-only button names itself), and session changes are announced through
  `kiln_ui::a11y::announce` ("Stopped at line N", "Running", "Session ended").
- **Tests.** New tests go in new files, following today's placement:
  `src/tests/<name>.rs` declared from `src/tests/mod.rs` (ids `tests::<name>::<test>`),
  or an integration test under `tests/` for public API (ids bare, no binary prefix).
- **Design calls** go in your decision file `docs/decisions/<lane>-*.md` (exempt
  from scope). Where §13 and §§1-12 of the record disagree, §13 wins.

Platform: **Linux-OK** (headless tests only; no screenshot goldens move).

## Implementation notes

- `LspManager::set_jdtls_bundles(Vec<PathBuf>) -> JdtlsBundles`: the bundles merge into a routed
  jdtls's `initializationOptions.bundles` as it launches (after the user's, skipping repeats), so
  a change never restarts a running jdtls. They apply on its next start at a root. The report
  (also `jdtls_bundles()`) gives the roots still running without them and why options that
  can't take them were refused. A jdtls that gave up after too many crashes stays stopped. Design
  calls are in
  `docs/decisions/aet7gz5k46-jdtls-debug-bundles.md`.
- `LspManager::execute_command_at(name, root, command, arguments)` (`execute_command.rs`) answers
  raw JSON. It refuses a command the server's `executeCommandProvider` doesn't list without sending
  anything: at once, or on the server thread if it is still initializing.
- The fake answers `workspace/executeCommand` by command (`Script::commands`,
  `FakeServer::set_command_reply`).
