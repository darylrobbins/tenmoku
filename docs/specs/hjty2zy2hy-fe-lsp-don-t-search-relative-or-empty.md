---
task_uid: "01m4bq2z39f1ef27hjty2zy2hy"
task: "T176"
rev: 5
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Regression test tests::launcher_path_safety::relative_path_entries_are_not_searched (fails at base): with an executable rust-analyzer only in a temp workspace root and in \u003croot\u003e/node_modules/.bin, LoginPath::resolve(\"rust-analyzer\", Some(PATH), root) returns None for each PATH of \".\", \"\" (empty), \"/nonexistent:\" (trailing empty entry) and \"node_modules/.bin\"; with an absolute temp bin dir also on PATH holding rust-analyzer, it returns that one"
    required: true
    manual: false
  - id: "C2"
    text: "tests::launcher_path_safety::home_and_absolute_entries_still_work: a PATH entry of ~/bin (with a temp home) and an absolute dir are still searched in order; a relative spec command such as ./bin/server still resolves under cwd; the existing tests::launcher tests pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "Only crates/fe-lsp/src/launcher.rs, the new test file crates/fe-lsp/src/tests/launcher_path_safety.rs (declared by one mod line in crates/fe-lsp/src/tests/mod.rs, not by a #[path] line in launcher.rs, so the base regression run compiles it), and the relative/empty PATH-entry block of tests::registry::bare_commands_use_the_login_shell_path in crates/fe-lsp/src/tests/registry.rs change; that block now asserts relative and empty PATH entries are skipped, and the rest of registry.rs is untouched; the docs on resolve and find_executable say relative and empty PATH entries are skipped and why"
    required: true
    manual: false
---

# fe-lsp: don't search relative or empty PATH entries for a server binary, which resolve under the workspace root

From the push security review of ce5f1ab (T141): `LoginPath::resolve` looks a bare server command (`rust-analyzer`) up on a `PATH` whose empty, `.` and other relative entries `find_executable` (crates/fe-lsp/src/launcher.rs ~338) resolves under `cwd`, which is the workspace root. A user whose login `PATH` holds `.`, an empty entry (a trailing `:`) or `node_modules/.bin` would run a `rust-analyzer` that a cloned repository ships at that place as soon as a Rust file opens. A shell does the same in that directory, but the editor does it unasked on open.

## Change

- `find_executable` passes over empty, `.` and any other relative `PATH` entry (after `~` expansion: `~` and `~/x` stay, as they are under home). Only absolute directories are searched.
- Update the `resolve` and `find_executable` docs: relative and empty entries are skipped, because the cwd is the workspace, which may be untrusted. A relative `spec.command` (one with a `/`, from built-in specs or the user's own `[lsp]` settings; project settings are already ignored) still resolves under `cwd`, as the user wrote it.
- Nothing else changes; the workspace-trust question (servers auto-starting in any folder) is separate and waits on the user.

## Tests

New file `crates/fe-lsp/src/tests/launcher_path_safety.rs`, declared by one `mod launcher_path_safety;` line in `crates/fe-lsp/src/tests/mod.rs` (T142 has landed, so that file is free). A `#[path]` line in `launcher.rs` would not do: the regression check's base run overlays only test files, so it would never compile the test at base. The existing `tests::registry::bare_commands_use_the_login_shell_path` asserted the old behaviour; its relative/empty PATH-entry block now asserts those entries are skipped.
