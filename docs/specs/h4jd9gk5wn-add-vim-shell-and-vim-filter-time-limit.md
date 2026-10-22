---
task_uid: "01m55rk0kp93kzj4h4jd9gk5wn"
task: "T165"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test vim_filter_settings_and_project_files (top level, in crates/fe-config/tests/vim_filter_settings.rs) passes: vim.shell defaults to empty and vim.filter_time_limit to 10000, both round-trip from a user file, 99 is a located schema error; a project file's vim.shell and [[vim.remap]] are dropped with a warning each at their key while its vim.jump_on_s still applies."
    required: true
    manual: false
  - id: "C2"
    text: "fe-config's schema and template tests pass with the regenerated crates/fe-config/settings.schema.json and crates/fe-config/settings-template.toml, and the terminal panel's a_project_file_cannot_set_the_shell still passes."
    required: true
    manual: false
---

# Add vim.shell and vim.filter_time_limit settings for vim's ! filter, and ignore a project file's vim.shell and vim.remap (Mac T912)

Filed on argon from Mac T912 (card ~/coordination/argon-cards/zzq/T912.json rev 1; note zzq, Vim parity after the user's decisions). Mac task uid 01m55rh4wkretjrwaz01r1hftr. Base: argon trunk 7edfe4f (= mac/main c7b6a04 synced). Corpus cases are recorded with nvim 0.12.x; unit tests are named by full module path, integration tests by bare fn name. Argon deps: T164 (Mac T911). Held in the inbox: waits until Mac T873 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T912)".

## Problem

The user approved vim's `!` filter (T908, T909) on 2026-10-17 with guardrails: it runs the user's shell from a
`vim.shell` setting (default `$SHELL` on Unix, falling back to `/bin/sh`; Windows `%COMSPEC%`), under a
timeout that is a setting, and never from workspace-provided config. Neither setting exists
(`VimSettings`, `crates/fe-config/src/settings.rs`). A project settings file can set every `[vim]` key today,
so once T873 lands a folder's `.fastedit/settings.toml` could add `[[vim.remap]]` entries that type
`:%!…<CR>` as the user. The terminal panel already keeps a project from choosing its program
(`drop_terminal_shell`, a `project_filter`, warning `PROJECT_TERMINAL_SHELL`).

## Change

- `VimSettings` gains `shell: String` (default empty: the user's shell as T909 resolves it, documented as
  such for the schema) and `filter_time_limit: u64` (milliseconds a `!` filter or `:r !` may run before it is
  killed and nothing changes; default 10000; `#[schemars(range(min = 100))]`).
- A new project filter `drop_vim_user_only`, registered next to `drop_terminal_shell`, drops a project file's
  `vim.shell` and `vim.remap` with a warning each at the key (`vim.shell and vim.remap in project files are
  ignored: a folder can't choose the program ! runs or type keys as you`). Other `[vim]` keys a project sets
  still apply.
- Regenerate `crates/fe-config/settings.schema.json`; add the commented defaults to
  `crates/fe-config/settings-template.toml` beside `# regex_time_limit = 2000`.

Test: a new `crates/fe-config/tests/vim_filter_settings.rs`.

Order: settings.rs, the schema and the template are changed by T873 (`vim.remap`, which this filter drops),
T894 and the ReplaceWithRegister setting card: this card depends on that card (after T894) and on T873. T842
(extension themes) also touches settings.rs, unrelated; the scheduler keeps them apart.
