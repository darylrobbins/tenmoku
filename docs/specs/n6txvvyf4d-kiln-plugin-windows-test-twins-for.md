---
task_uid: "01m73yn46377twebn6txvvyf4d"
task: "T571"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "New helper kiln/crates/kiln-plugin/tests/support/script.rs, `script(dir, name, sh, cmd)`: writes an executable `#!/bin/sh` script on Unix or a `.cmd` on Windows, and copies System32 programs where tests used /usr/bin; declared from each test file that uses it."
    required: true
    manual: false
  - id: "C2"
    text: "`#![cfg(unix)]` is gone from tests/grants.rs and tests/process.rs, and `mod` gating of prelaunch.rs and program_budget.rs in src/tests/mod.rs is removed; all of their tests run on every OS through the helper."
    required: true
    manual: false
  - id: "C3"
    text: "The symlink tests `a_symlink_leading_outside_is_rejected` (manifest.rs) and `dotdot_and_symlinks_cannot_leave_the_root` (fs.rs) use `symlink_file`/`symlink_dir` on Windows."
    required: true
    manual: false
  - id: "C4"
    text: "Only checks of Unix modes stay cfg(unix); the mode-0644 prelaunch cases each get an 'extension not in PATHEXT' Windows twin."
    required: true
    manual: false
  - id: "C5"
    text: "On Linux the full kiln-plugin suite passes with no test removed (the test count does not drop)."
    required: true
    manual: false
---

# kiln-plugin Windows test twins for grants, process, prelaunch and symlinks (P4, W-h) (Mac T1693)

# kiln-plugin Windows test twins for grants, process, prelaunch and symlinks (P4, W-h)

Draft id P4 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 2.2 row W-h (kiln-plugin half). The `windows` CI job is the proof; on Linux keep behaviour identical. Scope is files, not tests/**; tests/cache.rs belongs to P7.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- **The helper**, `tests/support/script.rs`: `script(dir, name, sh, cmd)` writes `name` (a
  `#!/bin/sh` file, mode 0755) or `name.cmd` (`@echo off`, CRLF); `system_program(dir, name)`
  copies the first `name` on `kiln_core::platform::system_path()` (so `System32` on Windows),
  with its `<language>\<name>.exe.mui` files, which a copy looks for beside itself. macOS's
  `cc`-built exec wrapper moved into it from `tests/process.rs`. `src/tests/mod.rs` declares it
  with `#[path = "../../tests/support/script.rs"]`.
- **Windows stand-ins** in `tests/process.rs`, since `System32` has no `printf`, `env`, `sleep`
  or `cat`: `printf` is a `.cmd` writing each argument on a line as `cmd.exe` got it (in std's
  quotes, `"` doubled, when std quoted it); the recorded-path test copies `whoami`; `sleep` is a
  copy of `ping` (`-n <s+1> 127.0.0.1`); `cat` is a copy of `sort` given one line of text (no
  NUL); `env` is a script running `set`, which may add `cmd.exe`'s own `COMSPEC` and `PROMPT`.
  The output-cap test floods from a `.cmd` loop. The arguments twin passes `cmd.exe`
  metacharacters (`&`, `|`, `>`, `%PATH%`, `^`, `"`) and checks each arrives whole and nothing ran.
- **Unix-only, kept so:** `a_fifo_in_place_of_the_program_is_passed_over_without_blocking`
  (`mkfifo -m 755`; Windows' named pipes are under `\\.\pipe\`, which no `PATH` reaches). Its
  Windows twin, `a_folder_in_place_of_the_program_is_passed_over`, puts a folder named
  `tool.cmd` in the program's place.
- **PATHEXT twins** (`..._by_pathext`) take `.CMD` out of the process's `PATHEXT` and put it back,
  so they hold `tests::programs_alone()` (a write lock) while every other unit test that resolves
  or checks a program holds `tests::programs()`; nextest runs each test in its own process anyway.
  The two mode cases share one body, `late_suspension_of_a_restored_file`.
- **`dotdot_and_symlinks_cannot_leave_the_root` is ignored on Windows**, as its neighbours in
  `tests/fs.rs` are: `wasi_context` preopens a root under its host path (`C:\...`), which
  wasi-libc can't resolve, so no guest path reaches the root there. Mapping guest paths is W-d's
  `kiln_plugin::paths` (P6) in `src/fs.rs`, outside this card; the test harness can't translate
  them without bypassing `wasi_context`, which is what the test is about. Its links are made with
  `symlink_file`/`symlink_dir` on Windows, so it runs there as written once P6 lifts the ignore.
- **The output-cap test's deadline half** runs without the 16-byte cap: on Windows its sleeper,
  `ping`, writes more than that at once, and would be killed for its output, not its time.
- **Not run on Windows here:** the Windows side compiles and is clippy-clean for
  `x86_64-pc-windows-msvc` (cross-checked with a stubbed C compiler), but runs only in the
  `windows` CI job.
