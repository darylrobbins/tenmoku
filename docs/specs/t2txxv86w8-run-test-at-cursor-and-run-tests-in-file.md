---
task_uid: "01m6s9dbmcdhwapkt2txxv86w8"
task: "T293"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "test_runnables_tests::rust_test_attributes_and_cfg_test_mods and test_runnables_tests::rust_async_test_and_rstest"
    required: true
    manual: false
  - id: "C2"
    text: "test_runnables_tests::go_test_functions_need_the_testing_t_parameter and test_runnables_tests::python_methods_in_test_classes"
    required: true
    manual: false
  - id: "C3"
    text: "test_runnables_tests::no_runnables_in_plain_code and test_runnables_tests::innermost_runnable_at_a_row_is_chosen"
    required: true
    manual: false
  - id: "C4"
    text: "test_commands_tests::cargo_command_for_a_test_in_a_nested_module, test_commands_tests::cargo_command_for_an_integration_test_file, test_commands_tests::go_command_runs_one_test_by_anchored_name and test_commands_tests::pytest_command_for_a_class_method"
    required: true
    manual: false
  - id: "C5"
    text: "test_commands_tests::run_at_cursor_starts_the_task_in_a_terminal_tab and test_commands_tests::rerun_last_repeats_it"
    required: true
    manual: false
  - id: "C6"
    text: "test_commands_tests::no_test_at_the_cursor_toasts and test_commands_tests::untrusted_folder_gets_the_trust_toast"
    required: true
    manual: false
  - id: "C7"
    text: "vim_lazyvim_preset_tests::lazyvim_layout_is_the_table passes with rows for \u003cleader\u003etr, \u003cleader\u003ett, \u003cleader\u003etl"
    required: true
    manual: false
  - id: "C8"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-workbench -p fastedit pass"
    required: true
    manual: false
---

# Run Test at Cursor and Run Tests in File for Rust, Go and Python, through the tasks runner (Mac T1409)

# Run Test at Cursor and Run Tests in File for Rust, Go and Python, through the tasks runner

## Problem
There is no test runner. VS Code and nvim users (neotest, vim-test, `<leader>tr`, `<leader>tt` in LazyVim's test extra) run the test under the caret or a file's tests with one key. Tenmoku has the pieces: a tasks runner with problem matchers (T1327 to T1331: `task_defs.rs`, `task_run.rs`, `task_problems.rs`) and tree-sitter parses, but nothing that finds a test or builds its command.

## Change
- New `crates/fe-workbench/src/test_runnables.rs`, pure (no gpui): `runnables(language: &str, syntax: &Syntax, text: &Text) -> Vec<Runnable>` over the parse the editor already holds (as `spell_ranges.rs` does, nothing parses), where `Runnable { name, kind: Test | Module | File, rows: Range<u32>, selector }`:
  - Rust: a `function_item` preceded by an `attribute_item` whose path ends in `test` (`#[test]`, `#[tokio::test]`, `#[rstest]`, `#[test_case]`), and a `mod` carrying `#[cfg(test)]` (kind Module). The selector is the module path inside the file (`tests::adds`).
  - Go: a top-level `function_declaration` named `Test*`, `Benchmark*` skipped, with a `*testing.T` parameter.
  - Python: a `function_definition` named `test_*` at top level or in a `class Test*` (selector `Class::test_name`), and the class itself (kind Module).
  - JavaScript/TypeScript (`describe`/`it`/`test`) is the follow-up, not here.
- `test_command(runnable, path, root) -> Option<Task>` builds a `Task` (`task_defs::Task`, `Source::Cargo`/`Source::File`, `TaskCommand::Argv`, label `Test: {name}`) with the matching problem matcher:
  - Rust: nearest `Cargo.toml` upward from the file gives the package (`[package] name`, found by a small read through the same `ProjectFiles` seam `task_defs` uses), then `cargo test -p {pkg} {module_path_in_crate}::{selector} -- --exact --nocapture` (a file's tests: the file's module path, derived from `src/a/b.rs` -> `a::b`, `mod.rs`/`lib.rs`/`main.rs` handled; `tests/x.rs` -> `--test x`).
  - Go: `go test -run '^Name$' ./{package dir}`; file: `go test ./{dir}`; cwd the module root.
  - Python: `python -m pytest {file}::{selector}`; file: `python -m pytest {file}`.
  - A language with no support reports "No test runner for {language}".
- Commands in a new `crates/fe-workbench/src/test_commands.rs` (typed commands, `workbench_commands.rs` untouched): `tests.run_at_cursor` ("Run Test at Cursor": the innermost Test runnable containing the caret, else the enclosing Module, else a toast "No test at the cursor"), `tests.run_file` ("Run Tests in File"), `tests.rerun_last` ("Rerun Last Test"; the task last run through these, kept separate from `tasks.rerun_last`). All go through `Workbench::run_task`, so the output shows in a terminal tab, the trust toast applies, and failures list in the Problems panel.
- Palette entries come with the commands. Key bindings: none by default in the app keymap; the lazyvim preset gets `<leader>tr` (run nearest) , `<leader>tt` (run file), `<leader>tl` (rerun last) in lazyvim.vimrc with TABLE rows; this card depends on the preset-touching which-key chain only for that file (see deps).

## Tests (new files)
- `crates/fe-workbench/src/test_runnables_tests.rs` (`test_runnables_tests::...`): `rust_test_attributes_and_cfg_test_mods`, `rust_async_test_and_rstest`, `go_test_functions_need_the_testing_t_parameter`, `python_methods_in_test_classes`, `no_runnables_in_plain_code`, `innermost_runnable_at_a_row_is_chosen`.
- `crates/fe-workbench/src/test_commands_tests.rs` (`test_commands_tests::...`): command-line construction as pure assertions (`cargo_command_for_a_test_in_a_nested_module`, `cargo_command_for_an_integration_test_file`, `go_command_runs_one_test_by_anchored_name`, `pytest_command_for_a_class_method`), plus gpui tests with the fake sessions used by `task_commands_tests.rs`: `run_at_cursor_starts_the_task_in_a_terminal_tab`, `rerun_last_repeats_it`, `no_test_at_the_cursor_toasts`, `untrusted_folder_gets_the_trust_toast`.
- `vim_lazyvim_preset_tests::lazyvim_layout_is_the_table` with three new TABLE rows passes.

## Not in scope (later)
A test explorer tree with results and statuses, JS/TS/Java runners, debug-test, coverage, parsing `cargo test` results into per-test pass/fail, rust-analyzer's runnables and the LSP code-lens "Run Test" (code lenses from the language server are requested by fe-lsp but no workbench consumer exists).

## Implementation notes

- `fe-workbench` now depends on `tree-sitter` directly (one line in its `Cargo.toml`, and `Cargo.lock`): `test_runnables.rs` walks the `Tree` that `Syntax::tree()` returns, and its helpers take `tree_sitter::Node`. Nothing new is compiled; fe-syntax already links it.
- `test_command(files, language, runnable, path, root) -> Result<Task, String>` rather than `Option<Task>`: the error is the toast's text ("No Cargo package holds src/x.rs", "No go.mod holds …", "No test runner for …"). A whole file is `Runnable::file(path, rows)`, kind `File`.
- `Runnable` has a fifth field, `exact`: `#[rstest]` and `#[test_case]` expand a function into a module of cases, so their test runs without `--exact` (the filter `path::name` reaches the cases). A `#[cfg(test)]` module runs `path::`, so `tests` doesn't also run `tests_more`.
- Cargo targets come from the manifest first: `[lib]`, `[[bin]]`, `[[test]]`, `[[bench]]` and `[[example]]` with their `name` and `path` (or the path Cargo infers from the name), then the ones Cargo finds by their place (`src/lib.rs`, `src/main.rs` as the package's binary, `src/bin/x.rs`, `tests/x.rs`, `tests/x/main.rs` and so on). A file that is a target's root runs that target (`--bin tenmoku` for fastedit's `src/main.rs`); a module file belongs to the nearest root's directory, with that target's flag when only one target can hold it (a module in `src` with both a library and a binary gets no flag). A library's tests get no flag except a file run of its root (`--lib`). A file no target holds outside `src` (`build.rs`) says so. Package and manifest lookups never climb above the root. Raw identifiers lose their `r#`, as libtest names them. The manifest's `autolib`, `autobins`, `autotests`, `autobenches` and `autoexamples = false` keep Cargo from finding that kind by place, so a file in `tests` of a package with `autotests = false` is a module of the declared test whose root's directory holds it. A `#[cfg(test)] mod tests;` declared in a file runs that module from its declaration.
- Where each runs, so its failures' paths resolve in the Problems panel: `cargo test` in the workspace's directory (the nearest `Cargo.toml` with `[workspace]` above the package, else the package's), as Cargo prints paths from there; `go test -run '^Name$' .` in the package's directory, not the module root the spec named, since a failing Go test prints its file from its package's directory; `python -m pytest --tb=native …` at the root, so failures are Python's own tracebacks.
- The matchers (`task_problems.rs`) read test failures: rustc's also takes a panic, `thread 'name' (id) panicked at path:l:c:` with its message on the next line (the id as the pinned 1.98.1 prints it, or none as older toolchains do) (and the pre-1.73 one-line form), named for its test; Go's also takes a failing test's indented `file.go:l: message`, which has no column.
- Every command saves the window's files first (`Workbench::save_all`), as VS Code's tasks do by default: a test runs what's on disk. If one is still unsaved afterwards (a failed save, or an overwrite declined), it says "Save your changes to run the tests" and runs nothing. Reruns too.
- Python follows pytest's default collection: a `test` prefix for functions and methods, `Test` for classes, decorators included in a test's rows.
- The commands are declared on a `TestCommands` struct, as `save_commands.rs` does, so `workbench_commands.rs` is untouched; they take titles "Run Test at Cursor", "Run Tests in File", "Rerun Last Test". The last test is a global keyed by the workbench and kept with its root, so a new root has none, and only a run the trust check let through is remembered.
- The gpui tests turn rust-analyzer off through the temporary project's settings file: a Rust file in a root otherwise starts it on its own thread, which the test scheduler rejects.
