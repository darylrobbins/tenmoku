---
task_uid: "01m6vvvw6a7cw4e0g8ge5r8tf7"
task: "T1560"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C2"
    text: "Prompt answers are still matched by button index; en-US byte-equal; cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# fe-workbench extensions, plugins, updates and trust strings (i18n W5, rest)

# fe-workbench extensions, plugins, updates and trust strings (W5, rest)

Spec: card **W5** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9, for the fe-workbench files the B3 surface
cards don't restyle (they move their own files' text). Sections 3–8 are the
design. en-US stays byte-equal to today's, so no golden and no behaviour test
changes except tests that construct removed literals.

Files: `extension_install.rs`, `extension_snippets.rs`, `extensions_picker.rs`, `plugin_bridge.rs`, `plugin_dev.rs`, `plugin_events.rs`, `plugin_host.rs`, `plugin_log.rs`, `plugin_manage.rs`, `plugin_paint.rs`, `plugin_prompt.rs`, `grammar_prompt.rs`, `update_check.rs`, `lsp_consent.rs`, `lsp_consent_prompt.rs`, `trust.rs`, `user_snippets.rs`.

Add `crates/fe-workbench/l10n/en-US/extensions.ftl` (keys `workbench-extensions-…`) to
`resources!` in `crates/fe-workbench/src/l10n.rs`, and these files to
`l10n-allow.toml`'s `migrated` list. English plural helpers become plural
messages; fragments become whole messages.

Goes before T1485 (`grammar_prompt.rs`): lead call 2026-11-12, T1485 waits on the grammar registry publish, so the order is reversed and T1485 adds its new prompt strings to `extensions.ftl`. The plan's wait for the audit's P8 is
dropped (not filed). Dialog answers stay matched by button index.

Platform: **Linux-OK**.

## Implementation notes

- **Prompts.** Every prompt's buttons are resolved when it's shown
  (`buttons()`, `full_access_buttons()`, `uninstall_buttons()`,
  `replace_buttons()`), and the answers are still matched by the index
  constants (`ALLOW`, `DONT_ALLOW`, `UNINSTALL`, `REPLACE`, …). Allow, Don't
  Allow and Cancel are shared keys (`workbench-extensions-allow`, `-dont-allow`,
  `-cancel`).
- **Multi-paragraph details** (plugin grant, full access, LSP consent): each
  paragraph and each listed line is a whole message; the code joins them with
  line breaks and indents capability and invocation lines with two spaces, so
  no message starts or ends with a line break Fluent would trim. Where a
  sentence had an optional clause spliced in (`shut down: …`, `Linked from …: it
  reloads …. It asks before it runs.`, `Couldn't install … . The language pack
  …`, the escalation footer's installed version), each variant is its own
  message.
- **Plurals.** The extensions list's "1 theme, 2 lsp servers" counts are plural
  messages (`workbench-extensions-kind-*`), joined by ", " as a pre-joined list.
- **Kept English, with reasons in `l10n-allow.toml`:** ids and file formats
  (a staging folder's name, temporary files, the consent file's TOML comment
  header and hash records, a plugin's key-context flag, thread name, status
  item id, the User-Agent), the plugin host's lifecycle records (debug log
  lines), `PluginDenied`'s `&'static str` arg (kiln-command's English error),
  and `USER_SNIPPET`, which `snippet_picker.rs` (not in this card) still shows;
  user snippets' completions use `workbench-extensions-user-snippet`.
- **`REBUILD`** stays public under its name (lib.rs is append-only) as a
  `Display` value that resolves `workbench-extensions-plugin-rebuild` when
  shown; a plugin that doesn't link gets `workbench-extensions-plugin-unlinked`,
  which references it.
- **Byte-equality** was checked with a throwaway snapshot of every prompt
  text, plugin row and extensions-list row before and after (not committed):
  identical.
- **Carried T1356 F1.** `HostedPlugin::call`'s failure path now stops the
  inbox with the diagnostic (`Inbox::stop`) instead of only closing it, so the
  host's `stopped_running` runs whoever made the call: a formatter's (through
  `LateHandle`), and even once the formatting request stopped waiting. A
  command's dispatch also calls `stopped_running`; the second call finds the
  load gone and does nothing. Regression test
  `plugin_format_tests::a_formatter_overrunning_its_deadline_stops_the_plugin`
  (in `plugin_format_tests.rs`, where the formatter harness is): the plugin's
  read of the buffer goes unanswered past its 2 s call deadline, and it ends
  Stopped (timed out), its commands and formatter gone. It fails without the
  fix.
