---
task_uid: "01m6enxr22qk8zx9krhsj46j8g"
task: "T1310"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cli::tests::position_suffix_parses_line_and_column (a.rs:12, a.rs:12:5, a.rs:0 and a.rs:x clamp or stay literal, a real file named a:1 wins over the suffix)"
    required: true
    manual: false
  - id: "C2"
    text: "cli::tests::several_paths_keep_order_and_folder_first_is_root"
    required: true
    manual: false
  - id: "C3"
    text: "cli::tests::flags_help_version_double_dash_and_unknown_flag_exit_codes"
    required: true
    manual: false
  - id: "C4"
    text: "open_at::tests::open_document_at_puts_the_caret_at_line_and_column_clamped_and_scrolled (name in the new file's own tests module)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo deny check licenses bans sources passes and Cargo.lock gains no new package (pico-args 0.5.0 only becomes a direct dependency)"
    required: true
    manual: false
  - id: "C6"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Parse several paths and path:line[:col] in the tenmoku command line, with --help/--version and --new-window/--wait accepted, and open each at its position

## Problem
`crates/fastedit/src/main.rs` reads argv by hand: an optional `--perf`, then one path (`args.into_iter().next()`), passed to `fe_workbench::resolve_launch`. Extra paths are silently dropped, `file.rs:42:7` is a missing file named `file.rs:42:7`, and there is no `--help`/`--version`. User, 2026-11-02: "Yes please" to a proper `tenmoku` command-line launcher (several paths, `path:line[:col]`, `--wait`, `--new-window`, an install command). This card is the argument half; the running-instance handoff, `--wait` and the PATH install are separate cards.

## Change
Add `crates/fastedit/src/cli.rs`: `parse(args) -> Result<Cli, CliError>` over `pico-args` 0.5 (already in Cargo.lock through resvg; add it as a workspace dependency at the same version, so no new lock entry and no new licence). `Cli` holds `perf`, `new_window`, `wait` (parsed now, acted on by later cards; accepted and ignored here with a log line), and `targets: Vec<Target { path, line: Option<u32>, col: Option<u32> }>`. Rules: `path:line` and `path:line:col` are 1-based; the suffix is stripped only when the whole string is not an existing path (a file may really be named `a:1`); `--` ends options; `-h/--help` and `-V/--version` print and exit 0; an unknown flag prints usage to stderr and exits 2. Paths are made absolute against the cwd.
Launch from the targets: the first directory is the root (as `resolve_launch` does); files open as tabs in argument order, the last one active. With only files, the root is the first file's parent. Add `Workbench::open_document_at(path, line, col, window, cx)` in a new `crates/fe-workbench/src/open_at.rs` (opens like `open_document`, then puts the caret on the position, clamped to the text, and scrolls it into view); `main.rs` uses it for the launch file and for the rest. Keep the existing first-frame fast path for a single file launch.

## Implementation notes

- `Workbench::open_document_at` returns a `Task<()>` that ends once the file is in its tab or has failed to open. `Pane` lands reads in any order and keeps one pending reveal, so `main.rs` awaits each task before opening the next: that keeps tabs in argument order and each caret at its own position. The first file is opened synchronously, so a launch read the host still holds isn't read twice.
- `pane.rs` and `go_to_line.rs` are out of scope, so the caret is placed from outside the pane once the tab exists, and the row and char-column clamp repeats Go to Line's few lines. A viewer's column counts bytes, as `Pane::apply` already gives it.
- Folders after the first are logged and left out. A lone `-` is a file of that name. An argument that isn't UTF-8 still takes a position.
- Carried finding T1257 F1: `main.rs`'s log calls now name the `fastedit` target. `config.rs` and `app_icon.rs` are out of this card's scope and keep their implicit `tenmoku::…` targets, so `StderrLogger` now counts `tenmoku*` as the app's: their info logs show without `RUST_LOG`, under the binary's name.
