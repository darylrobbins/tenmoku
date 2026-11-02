---
task_uid: "01m6ezv8ttz8shzvjy28zsxmz8"
task: "T412"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-config --test formatters_setting passes: with no [formatters] table the settings have none; [formatters.rust] command = [\"rustfmt\", \"--emit\", \"stdout\"] parses with prefer = external, stdin = true and timeout_ms = 3000 as defaults"
    required: true
    manual: false
  - id: "C2"
    text: "Test formatters_setting::file_placeholder_and_prefer_parse (cargo nextest run -p fe-config) passes: command = [\"prettier\", \"--stdin-filepath\", \"{file}\"] keeps {file}; stdin = false parses; prefer = \"lsp\" parses"
    required: true
    manual: false
  - id: "C3"
    text: "Test formatters_setting::bad_values_are_settings_diagnostics (cargo nextest run -p fe-config) passes: an empty command array, a non-array command, prefer = \"both\" and timeout_ms = 0 each produce a settings diagnostic naming the key and leave the other languages' entries intact"
    required: true
    manual: false
  - id: "C4"
    text: "settings.schema.json is regenerated and settings-template.toml documents [formatters.\u003clanguage\u003e] with commented prettier, rustfmt, black and gofmt examples; the repo's existing schema-up-to-date test still passes"
    required: true
    manual: false
---

# Add the [formatters.<language>] settings: an external formatter command per language, with prefer = external or lsp and a timeout (Mac T1324)

## Problem
Format Document only works through a language server (`lsp.format_document`, crates/fe-workbench/src/lsp_format.rs). Many users format with an external tool (prettier, rustfmt, black, gofmt, ruff format) that no language server offers. User approved 2026-11-02 as a day-one gap.

## Change
Settings only; nothing runs yet (f7b/f7c do that).
- `Settings.formatters: BTreeMap<String, FormatterSettings>` in crates/fe-config/src/settings.rs, keyed by Tenmoku's language name (rust, python, typescript, ...; same names as `languages` in the LSP server settings).
- `FormatterSettings { command: Vec<String> (first = program, rest = args; `{file}` is replaced with the buffer's absolute path, so prettier can find its config), stdin: bool = true (text goes to stdin and the formatted text is read from stdout; false = a temp copy is passed as `{file}` and read back, for tools that only format files), prefer: external | lsp = external (which one runs first; the other is the fallback when the first has nothing to do or fails), timeout_ms: u64 = 3000 (1..=60000) }`.
- Invalid entries are settings diagnostics (use the existing diagnostic path other settings use) and are dropped alone.
- Add to settings.schema.json (regenerate) and settings-template.toml (all examples commented out).
Does not touch `editor.format_on_save`: it keeps meaning "format before saving", now through whichever formatter applies.
