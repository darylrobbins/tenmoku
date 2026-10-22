---
task_uid: "01m5j50t54xh0gdwbmr6a725kb"
task: "T278"
rev: 4
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "C1 fs::a_guest_reads_inside_a_preopened_folder: with fs.read:workspace the fs.wasm guest reads a file in a workspace root"
    required: true
    manual: false
  - id: "C2"
    text: "C2 fs::nothing_is_preopened_without_a_grant: with no fs capability the same read fails"
    required: true
    manual: false
  - id: "C3"
    text: "C3 fs::read_alone_cannot_write_and_write_can: fs.read:workspace refuses a write; fs.write:workspace allows it inside the root"
    required: true
    manual: false
  - id: "C4"
    text: "C4 fs::dotdot_and_symlinks_cannot_leave_the_root: a path with .. and a symlink pointing outside the root are both refused"
    required: true
    manual: false
  - id: "C5"
    text: "C5 fs::the_wasi_environment_is_empty: the guest sees no environment variables, no arguments and no inherited stdio"
    required: true
    manual: false
  - id: "C6"
    text: "C6 process::a_granted_program_runs_by_its_recorded_path: process.spawn:printf runs the absolute path recorded at grant time, never a re-lookup, and returns its stdout"
    required: true
    manual: false
  - id: "C7"
    text: "C7 process::an_ungranted_program_is_refused: a name with no matching grant is an error and nothing runs"
    required: true
    manual: false
  - id: "C8"
    text: "C8 process::a_changed_binary_suspends_the_grant_before_spawn: after the recorded file is overwritten the spawn is refused with Suspended and the grant needs a new prompt"
    required: true
    manual: false
  - id: "C9"
    text: "C9 process::arguments_are_not_shell_parsed: an argument of ; echo pwned reaches the program literally"
    required: true
    manual: false
  - id: "C10"
    text: "C10 process::the_environment_is_minimal: env run through the grant prints only PATH, HOME, LANG or LC_*, TMPDIR and TERM"
    required: true
    manual: false
  - id: "C11"
    text: "C11 process::output_over_the_cap_and_runs_over_the_deadline_are_killed: a 5 s sleep with a 300 ms test deadline is killed in under 2 s, and output past the cap is cut"
    required: true
    manual: false
  - id: "C12"
    text: "C12 cargo nextest run -p kiln-plugin and cargo clippy -p kiln-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Preopen workspace folders for fs grants only, give plugins an empty WASI environment, and spawn only the recorded absolute program path with a minimal environment (Mac T1054)

## Problem

Files reach a plugin only through preopened directories (WASI), and programs run only by the grant's recorded absolute path,
never a name looked up again, never through a shell, with a minimal environment (decisions 1 and 5, §7). The runner
(P3) gives plugins an empty WASI context; this card fills it, and adds `process.spawn`.

## Change

- `kiln/crates/kiln-plugin/src/fs.rs` and `tests/guests/fs/`: `fs::configure(&mut WasiCtxBuilder, &Grants, &Workspace)`
  preopens each workspace root read-only for `fs.read:workspace` and read-write for `fs.write:workspace`, and nothing
  without them; the environment is empty, no arguments, stdio not inherited, no network from WASI (sockets denied).
- `src/process.rs`, `wit/process.wit`: `spawn(program, args, stdin) -> Output`: the program must name a granted
  `process.spawn:<program>` (or `*`); the run uses the grant's recorded absolute path and re-checks its identity
  (P6's `check_program`) immediately before the spawn, suspending the grant when it changed; arguments go as a vector to
  `std::process::Command` (never `sh -c`); the environment is cleared and then only `PATH` (`/usr/bin:/bin`, the app's
  default system path), `HOME`, `LANG` and `LC_*`, `TMPDIR` and `TERM` from the app's values; stdin is the bytes the
  plugin passes; output is capped at `Limits::max_read_bytes` and a run over a named `SPAWN_DEADLINE` (30 s) is killed,
  with a test override.
- Guest fixture for the WASI cases: `kiln/crates/kiln-plugin/tests/guests/fs/` (own `[workspace]` table, `src/lib.rs`
  using `std::fs`, `build.zsh` for `wasm32-wasip2`, checked-in `fs.wasm`).

## Notes

Process-spawn tests use real programs found on `PATH` (`env`, `printf`, `sleep`) copied into a tempdir where the
identity must change, and keep each run under 2 s. macOS and Linux only; no Windows paths.

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new files; the scope lists exact files.

## Implementation notes

- Scope: `tests/guests/fs/Cargo.lock` was added (the guest's own lock, so `build.zsh` is reproducible), and
  `src/bindings.rs` and `tests/bindings.rs` are needed: `process::State` now holds the plugin's `GrantRecord`, limits,
  deadline and environment, so `HostState::new` builds it with `process::State::default()` (grants nothing), and the
  old `every_unimplemented_import_returns_the_unimplemented_error` case for `try-spawn` became
  `a_spawn_with_no_grants_is_denied`. Both were respecced in once T279, which held them, landed.
- The guest's exports are `read-file` and `write-file`, not `read` and `write`: world-level export names are core
  symbols, and `read`/`write` replaced wasi-libc's own, which crashed every write to stdout.
- WASI preopens each root at its own absolute path, so a plugin sees the app's paths. `fs.write:workspace` preopens
  read-write: WASI has no write-only directory.
- Stdio isolation is checked in a child test process (`stdio_reaches_nothing_of_the_host`, ignored, run by
  `the_wasi_environment_is_empty`) whose stdin carries bytes and whose stdout and stderr are captured.
- A `process.spawn:*` grant runs only a name bound to a program in the record (as `GrantStore::decide` binds it);
  any other name is refused, never looked up. A suspension is applied to the in-memory record and, with
  `State::recorded_in`, to the store through `GrantStore::check_programs`.
- Output is capped per stream at `Limits::max_read_bytes`; a program still running when it writes past the cap is
  killed (status none) and its output returned cut. A run past the deadline is killed and fails. The program's stdio
  are Unix socket pairs, not pipes, so the calling thread feeds and drains them without blocking and without helper
  threads: when a call returns, by any path, nothing of it is left in the host, even when a program it started holds
  the output open (that call fails at the deadline) or the program never reads its stdin. Only the direct child is
  killed; a program it started runs on outside the host.
- Review round 1 (F2): `grants::evaluate` now counts a request naming a suspended program as added even when
  `process.spawn:*` covers it, so a changed binary is asked about again under a wildcard grant too.
- Review round 2 (F4): a suspension now holds while anything the record keeps covers the program, `process.spawn:*`
  included, through both `GrantStore::decide` (narrowing) and `GrantStore::grant`. Fail-closed: only a prompt that
  names the program and passes a fresh identity in `programs` lifts it (the old recorded identity never rebinds a
  suspended program); binding it any other way is refused with `GrantStoreError::Suspended`, in `grant` and in
  `decide`'s narrowing alike, and `State::spawn` refuses it while suspended. It goes only when nothing covers the
  program any more. `a_suspension_holds_until_the_program_is_granted_again` walks the changed-binary + wildcard
  sequence through each transition.
- `stdio_reaches_nothing_of_the_host` in `tests/fs.rs` is `#[ignore]` on purpose: it is the child half of
  `the_wasi_environment_is_empty`, which runs it with `--ignored` in its own process.
- On this Linux host the system `printf`, `env` and `sleep` are one 11 MB uutils multi-call binary, so each identity
  hash costs ~0.3 s in debug builds; tests copy each program once and spawn it as few times as the case needs.
