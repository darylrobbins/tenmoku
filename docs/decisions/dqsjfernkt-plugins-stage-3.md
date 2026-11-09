# Plugins stage 3: Windows, two budget tiers, API gaps, and what stays core

T1652. A design card: this document is the only change. It answers the user's
questions of 2026-11-09:

> "We're going to want the plugins to be cross-platform, so Windows does need to
> be a target. Are the limits on a plugin enough? That's great for a tree sitter,
> but let's say it's something more advanced, like compiling the project, or doing
> a more advanced lookup. And 64 meg probably couldn't run a language server in
> the sandbox. Anything on the interactive loop, yes, we want these fast
> timelines, but if it's an asynchronous process, maybe we can give it more
> latitude. And let's shore up the API gaps as well. Is there anything in the app
> that we should be moving to a plugin instead of keeping core? Or is that going
> to slow things down and take up more memory?"

Short answers, argued in the sections below:

- **Windows** is mostly there already. kiln-core has Windows twins for program
  lookup, file identity, stdio pumping and the keychain. Seven things are still
  Unix-only or wrong on Windows (§2). Nine cards fix them, and the `windows` CI
  job proves the fixes.
- **Limits.** The current limits become the *interactive* tier and stay as they
  are. A new *background* tier is for jobs: wall time up to 60 minutes, memory
  the manifest declares (up to 2 GiB), and streamed program output. It has no
  fuel (§3).
- **Language servers** stay native processes that fe-lsp runs. A plugin
  contributes one declaratively and can talk to it. A language server isn't run
  inside the sandbox (§3.6).
- **API gaps.** §4 sketches language features, panels, tasks and runs, settings,
  the 1.1 compatibility path and signing, in WIT.
- **Core vs plugin.** A plugin costs about 65–165 KiB of resident memory. Calls
  cost microseconds. Compiling a component the first time costs about 0.5 s per
  MB of wasm. Memory is not the problem; copies and thread hops on hot paths
  are. Nothing built in moves out in stage 3. Three new first-party plugins
  dogfood the API instead (§5).

## 1. Status

**Status: proposed.** Approver: **the user (Daryl Robbins)**. Nothing is built
until the user signs off.

- **No code changed.** `git diff --stat` against the lane's base (the commit
  adding this card's spec, 3964c10 on a801b67 at this revision; a rebase onto
  a newer main moves it) lists only this file. The measurements in
  §5 ran from a temporary test file and a temporary dev-dependency. Both were
  removed before the commit, and cargo's target dir was outside the repo.
- **No cards filed.** This card files no implementation card. When it lands, the
  lead brings this document to the user. The cards in §6 are drafted only after
  the user approves it.

**Amends** the accepted record
[vmtnmkay62-extensions-and-plugins.md](vmtnmkay62-extensions-and-plugins.md)
(T839). That file isn't edited here, because this card's scope is the new
document only. Once approved, this record supersedes these lines of it:

| Accepted record | Line | Superseded by |
|---|---|---|
| §7 "Stage 2 decisions (user, 2026-10-21)" | "Targets: macOS and Linux only. Windows is not a target now." | §2: Windows is a target for plugins (x86_64 first, Q1). |
| §7 same list | "Defaults, as named constants that settings may override later: 100 ms … 64 MB … 256 queued events … 16 MB … 10 s …" | §3: these become the **interactive tier**, unchanged. A **background tier** is added. The per-plugin overrides become `[extensions.plugin_limits."<id>"]`, with host ceilings. |
| §7 same list | "Panels are deferred." | §4.2: panels are designed (a declarative view tree, still no webviews) and carded in §6. |
| §7 "API surface", last bullet | "Panels later: a declarative view tree (list, tree, text, buttons) the host renders with kiln-ui; no webviews." | §4.2 is that design. |
| §7 "Security decisions", decision 1, last bullet | "Language servers stay host-spawned from declarative config, so an LSP plugin needs no spawn capability." | **Kept**, and extended by §3.6: a code plugin may also contribute `lsp` servers, and may send its own or a built-in server's session custom requests, with the new capability `lsp:<server>`. |
| §7 "Stage 2 decisions (user, 2026-11-03)" | "The WIT is unstable at @1.0.0 … until language-feature and UI imports exist" | §4.5: the exact conditions for declaring the worlds stable. |
| §7 "Versioning", *Preview* paragraph | adapters "apply from stabilization on" | §4.5: what a 1.1 minor needs, adapters included. |
| §8 "Out of scope" | "Extensions contributing settings, settings defaults, commands …, panels or webviews." | **Code plugins** may contribute settings (§4.4) and panels (§4.2). Declarative (stage-1) extensions are unchanged. Webviews stay out. |
| §8 "Out of scope" | "A marketplace, registry, network install, updates and package signing (stage 2 decides signing with the registry)." | §4.6: plugins join the signed grammar registry of T1481–T1486. |
| §12 Q9 | "Stage-2 signing and registry. Deferred 2026-10-21" | §4.6 proposes the answer. Q4 of §7 asks the user. |

§12's Q8 (grammar WASM cost) and Q10 (`allowed_licenses`) are untouched. So
are the AI chat's place ("stays built in") and the 2026-11-03 decision of no
Lua layer; §5 reaffirms both.

**Clean room.** This was written from this repository's code, its decision
records and its cards (T1570, T1481–T1486). It also used the manifests of the
wasmtime 48.0.8 crates in cargo's registry, for their feature lists. No
external documentation was read for it. Facts about the Windows APIs it names
(job objects, `CREATE_SUSPENDED`, DACLs, junctions, Credential Manager's blob
size) are general knowledge, and each card that relies on one proves it with a
test in the `windows` job. No other editor's plugin code or plugin
documentation was read.

**Decision:** proposed, for the user to approve. No code and no cards until
then.

## 2. Windows as a stage-2 target

### 2.1 What is already portable (read at the base)

Stage 2 was built for macOS and Linux, but much of it already compiles and runs
on Windows, because kiln-core keeps OS differences in `kiln-core/src/platform/`.

- **Stdio.** `kiln-core/src/platform/stdio.rs` feeds and drains a program's
  stdio from one thread. It uses non-blocking socket pairs on Unix. On Windows
  it uses a thread per stream over `io::pipe`, because anonymous pipes can't be
  made non-blocking. `without_console_window` sets `CREATE_NO_WINDOW`.
- **Program lookup.** `program_candidates` tries `PATHEXT`'s extensions, and
  `is_executable` checks for one. `file_identity` uses the volume serial number
  and file index. `canonicalize` drops the `\\?\` prefix for drive paths under
  `MAX_PATH`.
- **Grant store.** `locked_by` uses std's `File::lock`. `create_private_dir`
  and `private` fall back to Windows' defaults. `sync_dir` does nothing there.
- **Secrets.** `kiln-core/src/platform/keychain.rs` uses the Windows
  Credential Manager through `windows_native_keyring_store`.
- **The app.** `crates/fe-workbench/src/plugin_dev.rs:105` makes a dev link
  with `symlink_dir`. `plugin_log.rs:153` uses `localtime_s`.
  `extension_install.rs` has `cfg(not(unix))` twins of `same_file`,
  `is_executable`, `is_hard_linked` and `set_executable`.
- **The tree watcher** (`kiln-core/src/watch_tree.rs`) follows links itself:
  it registers each link's target directory and re-stats every `RECONCILE`. So
  it doesn't rely on the OS reporting changes behind a link.
- **fe-plugin** (`crates/fe-plugin/src`, `tests`) has no `cfg` at all. All of
  it already runs in the `windows` job.

### 2.2 What is Unix-only or wrong on Windows

Every item below was found by reading the code at the base. Each has the change
that fixes it and the card (§6) that makes it.

| # | Item, where | On Windows today | Change | Card |
|---|---|---|---|---|
| W-a | **Spawn environment.** `SYSTEM_PATH = "/usr/bin:/bin"` (`kiln-plugin/src/process.rs:117`). `minimal_environment` (:397) passes only `PATH`, `HOME`, `LANG`, `LC_*`, `TMPDIR` and `TERM`. fe-workbench's `programs()` (`plugin_host.rs:2101`) falls back to `SYSTEM_PATH` when `PATH` is unset. | The child gets `PATH=/usr/bin:/bin` and no `SystemRoot`, `windir`, `TEMP`, `TMP`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA` or `PATHEXT`. Programs that use Winsock or the crypto APIs fail without `SystemRoot`. `HOME` and `TMPDIR` mean nothing there. | Add `kiln_core::platform::system_path()` and `minimal_environment(os_env)`, one rule set per OS. On Windows, `PATH` is `%SystemRoot%\System32;%SystemRoot%;%SystemRoot%\System32\Wbem`, and the variables passed are `SystemRoot`, `windir`, `SystemDrive`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `TEMP`, `TMP`, `PATHEXT`, `ProgramData`, `NUMBER_OF_PROCESSORS` and `PROCESSOR_ARCHITECTURE`. Names are matched case-insensitively there (`Path` is `PATH`). `process.wit`'s doc comment lists both sets. It also says "(30 s)" where `PROGRAM_DEADLINE` is 10 s, which the same card corrects. | P3 |
| W-b | **Batch files are run through a shell.** `resolve_program` binds the first `PATHEXT` match, which can be a `.bat` or `.cmd` file. That is how npm installs every CLI on Windows (`prettier.cmd`, `eslint.cmd`). | std's `Command` runs a batch file through `cmd.exe`, which parses its arguments again. std escapes them, or refuses those it can't escape. So decision 1's "never through a shell" doesn't hold for these files. | `ProgramIdentity` gains `interpreter: Option<String>`, set to `cmd.exe` for `.bat` and `.cmd`. The grant prompt flags these as `sh` is flagged ("runs through cmd.exe, which reads its arguments again"). A spawn std refuses to quote fails with `invalid-argument`. `.exe` and `.com` keep running directly, and `PATHEXT` order already prefers them. | P3 |
| W-c | **Process-tree kill.** `Running::kill` (`process.rs:508`) kills the direct child only, on every OS. | A grandchild that holds the output open makes the run time out, but the grandchild keeps running, on Unix as well as Windows. fe-lsp already solves this for servers: `process_group(0)` plus `kill_process_group` on Unix, and a job object on Windows (`crates/fe-lsp/src/launcher.rs:374`, `:802`, `:884`). Its job is assigned after the spawn, so anything the server starts before then escapes. | Add `kiln_core::platform::ProcessTree`. On Unix, the child leads a new process group, and cancelling sends SIGTERM to the group, then SIGKILL 2 s later. On Windows, the process is created with `CREATE_SUSPENDED`, assigned to a job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then resumed (its one thread is found with a Toolhelp32 snapshot), so nothing escapes. Killing is `TerminateJobObject`, with no grace period, because a process with no console has no Ctrl-Break to send. kiln-plugin's `run` and §3's streamed programs use it. Moving fe-lsp onto it is a follow-up note, not a card here. | P2 |
| W-d | **Preopens and guest paths.** `fs::configure` (`kiln-plugin/src/fs.rs:51`) preopens each root under its host path string (`root.to_str()`, absolute). Paths cross the WIT as host strings too: `events.buffer-opened.path`, `buffer-saved.path` and `buffer.snapshot.path`. | The guest sees `C:\work` as a preopen name. A guest's libc and Rust's wasi `Path` split only on `/` and treat only `/…` as absolute. So `std::fs::read("C:\\work\\a.rs")` in a guest matches no preopen, and a path from an event can't be joined or split. | Add one mapping, `kiln_plugin::paths`. Host `C:\Users\d\work\a.rs` becomes guest `/C:/Users/d/work/a.rs`, which is the path part of its `file:///C:/…` URI, so paths and URIs agree. The drive letter is upper-cased, as `canonicalize` gives it. It applies to preopens and to every path in the WIT, both ways. A guest path holding `\`, or not starting `/<letter>:/` on Windows, is `invalid-argument`. On Unix the mapping is the identity. UNC (`\\server\share`) and over-long verbatim roots aren't preopened (a warning in the log) until Q7 is settled. The mapping is pure string code, so all of it is unit-tested on every OS. | P6 |
| W-e | **Backslashes and drive letters in manifests and grants.** `wasm_file` (`kiln-plugin/src/manifest.rs:160`) and stage 1's `contributed_file` (`kiln-core/src/extension.rs:582`) check only `has_root`/`is_absolute` and then walk the components. `parse_program` (`kiln-command/src/capability.rs:123`) refuses only whitespace and control characters. `resolve_program` refuses a name holding `is_separator`. | `wasm = "components\p.wasm"` loads on Windows but is a file name containing `\` on macOS and Linux, so a manifest written on Windows breaks elsewhere. (`C:p.wasm` is caught by the `Prefix` arm, so drive letters are already safe there.) `process.spawn:tools\rg` passes the parser, fails resolution on Windows, and on Unix looks for a file literally named `tools\rg`. | Contributed paths refuse `\` on **every** OS ("use / in extension paths"), so one manifest works everywhere. `process.spawn:` names refuse `/`, `\` and `:` on every OS. A name ending in a `PATHEXT` extension (`rg.exe`) is a warning ("write `rg`; the extension is found on Windows"). `fs.read:workspace` and `net:<host>` hold no path, so nothing changes for them. | P1 |
| W-f | **Symlinked dev links.** `plugin_dev.rs:106` uses `std::os::windows::fs::symlink_dir`. | That needs Developer Mode or `SeCreateSymbolicLinkPrivilege`. A standard user without Developer Mode gets `ERROR_PRIVILEGE_NOT_HELD` (1314), and Install for Development fails. CI's runner is an administrator, so `plugin_dev_tests` pass there and hide this. | On error 1314, fall back to a directory junction, which needs no privilege but works on local volumes only. `linked_from`, `links_to` and uninstall's remove-only-the-link must accept a junction. A test forces the junction path through a seam, so the admin runner runs it too. | P8 |
| W-g | **Compile cache.** `ComponentCache` trusts an entry only if `platform::owned_and_unshared` passes for its directory and file (`kiln-plugin/src/cache.rs:93`, `:100`, `:129`). On Windows that function is always `false` (`kiln-core/src/platform/fs.rs:63`). | Nothing is ever trusted, so **every launch recompiles every plugin** (`tests/cache.rs:5` says so). Measured on the Mac: 46–49 ms per 92 KB component, about 1 s for 2.8 MB (§5.1). The cache dir is under `%LOCALAPPDATA%` (`AppPaths`, `kiln-core/src/paths.rs:187`). | On Windows, `create_private_dir` sets a protected DACL: full control for the user's SID and SYSTEM, inheritance off. `owned_and_unshared` checks that the owner SID is the current user's (`GetSecurityInfo` and `GetTokenInformation(TokenUser)`), and that the DACL gives write access to no one but that user, SYSTEM and Administrators. The cache then works as on Unix. | P7 |
| W-h | **Tests that exist only on Unix**, because their programs are shell scripts made executable with `PermissionsExt` or copied from `/usr/bin`. In kiln-plugin: `tests/grants.rs` (`#![cfg(unix)]`, 11 tests), `tests/process.rs` (`#![cfg(unix)]`, 10), `tests/cache.rs` (`#![cfg(unix)]`, 10, see W-g), `src/tests/prelaunch.rs` (17) and `src/tests/program_budget.rs` (7) (`src/tests/mod.rs:14`), `tests/manifest.rs:130` `a_symlink_leading_outside_is_rejected`, and `tests/fs.rs:164` `dotdot_and_symlinks_cannot_leave_the_root`. In fe-workbench, `plugin_host_tests.rs`: `a_granted_program_runs_and_an_ungranted_one_is_denied` (:976), `a_granted_program_that_outlasts_the_call_deadline_completes` (:1256), `a_spawn_in_flight_ends_before_the_plugin_unloads` (:1274), `a_reauthorized_program_reloads_the_plugin` (:1297) and `an_event_handler_edits_its_buffer_after_a_slow_process` (:1540), with their helpers. | None of these run in the `windows` job. So binding, suspending, deadlines, output caps, the prelaunch checks and WASI's symlink containment are unproven there. | Each gets a Windows twin in the same file, in place of the `cfg(unix)`. A `.cmd` script stands in for each `#!/bin/sh` script, and copies of `System32` programs stand in for `/usr/bin`'s, through one helper, `script(dir, name, sh, cmd)`. A `.cmd` also exercises W-b, and W-c's job object once it spawns a child. The symlink tests use `symlink_file` and `symlink_dir`, which the admin runner can create. Only checks that are about Unix modes themselves stay `cfg(unix)` (`a_group_writable_directory_or_file_is_not_trusted` gets a DACL twin in P7, the prelaunch mode-0644 cases get an "extension not in PATHEXT" twin). | P4, P5, P7 |
| W-i | **Secrets and renames.** `secrets.wit` caps names (64 characters) but not values. The grant store (`grants.rs:1043`) and the cache write a temporary file and rename it over the old one. `GrantStore::get` reads without the lock (`grants.rs:575`). | Credential Manager holds a blob of at most 2,560 bytes, which a long token passes. On Windows a rename over a file another process holds open without delete sharing fails with access denied. Indexers and antivirus scanners do hold files that way. | Test a 4 KiB secret against the real Windows store. If it's refused, set one documented value cap for every OS in `secrets.wit`, so a plugin can't work on macOS and fail on Windows. On a sharing violation, retry the rename for up to 1 s (`kiln_core::platform::replace_file`), with a test that holds the target open. | P9 |

Two smaller points, which no card needs:

- **`MAX_PATH`.** wasmtime-wasi reaches files through handles relative to the
  preopen, so a deep path under a short root works. Only the root itself must
  fit, and roots over 260 characters keep `\\?\` and are warned about, as UNC
  roots are (W-d).
- **Line endings.** A buffer's text is what the editor holds, CRLF included.
  Plugins get byte ranges into that text, so nothing about it is
  Windows-specific.

### 2.3 How the `windows` CI job proves it

`.github/workflows/ci.yml:104` runs `windows-2025` with
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo nextest run --workspace --profile ci --lib --bins --tests`.
`.config/nextest.toml`'s `ci` profile filters out no plugin test. So:

- **Running there today:** every kiln-plugin test that isn't `cfg(unix)`:
  `tests/bindings.rs`, `commands.rs`, `compat.rs`, `events.rs`, `fs.rs` (all but
  the symlink test), `hostile.rs`, `limits.rs`, `manifest.rs` (all but the
  symlink test), `net.rs`, `net_deadline.rs`, `secrets.rs`, `single_wasmtime.rs`
  and `warnings.rs`. All of fe-plugin's tests: `bindings.rs`, `decorations.rs`,
  `diagnostics.rs`, `editor.rs`, `formatting.rs`, `net.rs`, `single_wasmtime.rs`
  and `status.rs`. fe-workbench's plugin tests, except the five in W-h. That
  includes `plugin_dev_tests.rs` (with `symlink_dir`) and
  `plugin_manage_tests.rs`.
- **Added by §6's Windows cards:** the twins in W-h; `kiln-core/tests/process_tree.rs`
  (P2); `kiln-plugin/tests/environment.rs` (P3); `kiln-plugin/tests/paths.rs`
  (P6); `tests/cache.rs` on Windows, with a DACL twin (P7); the junction case in
  `plugin_dev_tests.rs` (P8); and the secret-size and rename cases (P9).
- **The rule after P5:** no kiln-plugin, fe-plugin or fe-workbench plugin test
  is `cfg(unix)` unless it checks Unix modes themselves, and each of those has
  a named Windows twin. §4.5 makes this a condition for declaring the API stable.
- **Checking off the Mac.** The `x86_64-pc-windows-msvc` target is installed
  here, so a worker can type-check the Windows code with
  `cargo check --target x86_64-pc-windows-msvc -p kiln-core` before CI, where
  the crate's build scripts allow it. Only CI runs it.

**Decision:** Windows (x86_64) is a target for plugins. The seven gaps W-a to
W-i are fixed by cards P1–P9, which come first in §6. The `windows` job is the
proof, and after P5 no plugin test is Unix-only without a named Windows twin.
**Open question Q1:** is Windows on ARM (aarch64) a target too, or x86_64 only
until the app ships an ARM build?

## 3. Two budget tiers

### 3.1 The interactive tier: today's limits, unchanged

Everything the user waits on runs in the interactive tier: command handlers,
event handlers, formatting requests, and §4's language-feature, panel-event and
task-provider calls. Its numbers are today's constants in
`kiln-plugin/src/limits.rs`, kept for the reasons the measurements give:

| Limit | Value | Why it stays |
|---|---|---|
| Call deadline (`CALL_DEADLINE`) | **100 ms** of wasm per call | Typing feels instant under 100 ms. A completion that takes longer is better answered from an index a background job built (§3.2). The runner's own cost is 3–7 µs per call (§5.1), so nearly all 100 ms is the plugin's. |
| Memory (`MEMORY_BYTES`) | **64 MiB** per plugin | A loaded plugin holds 65–165 KiB resident (§5.1). 64 MiB fits a 16 MiB buffer read and its parse two or three times over. More than that is background work. |
| Program share (`PROGRAM_DEADLINE`) | **10 s** per call, for all its `process.spawn` runs together | A formatter or `git` call. The call holds only its plugin's thread, never the UI, but 10 s is already the longest a user waits on a command. |
| Event queue (`EVENT_QUEUE`) | **256** | Coalesced. 10,000 change events cross the boundary in 0.57 ms (§5.1), so the cap is about memory and staleness, not speed. |
| One read (`MAX_READ_BYTES`) | **16 MiB** | An 8 MiB buffer read and echoed back takes 2.4 ms. 16 MiB bounds a single copy into the 64 MiB store. |

An overrun still unloads the plugin, as now. An interrupted guest's state can't
be trusted, so it can't be resumed.

### 3.2 The background tier: jobs

**Shape.** A plugin starts a **job** from any interactive call (or `init`). The
host runs it in a **worker store**, a second instance of the same component, on
a thread of its own. So the interactive instance keeps answering while the job
runs, and the job has its own memory cap. A worker instance doesn't run `init`.
Imports that register things (commands, providers, panels, formatters) answer
`denied("not in a background job")` there. Its other imports work under the
plugin's grants: buffer reads, diagnostics, decorations, status, fs, net,
secrets, `data` and `set-view`. A job ends by returning a result, which the host hands to
the interactive instance's `job-finished` export.

**Numbers**, with reasons:

| Limit | Default | Manifest may declare | Host ceiling | Why |
|---|---|---|---|---|
| Wall time per job | **10 min** | `background.wall`, up to the ceiling | **60 min** | `cargo check` on this repo, or a first index of a large monorepo, takes minutes. A job past its wall time is cancelled (§3.4); the plugin stays loaded and the Run shows "timed out". |
| Guest slice | **5 s** of wasm without entering a host import | — | — | Catches an infinite loop without fuel. wasmtime's call hook resets the epoch deadline each time the guest returns from a host import, `job-context.cancelled()` included. So a job that does real work (reads files, reports progress, waits on a program) is never cut, and 5 s of pure compute between two imports is far past any sane inner loop. An overrun kills the worker store, not the plugin. The call hook needs wasmtime's `call-hook` feature, which the workspace doesn't enable today; B3 enables it in kiln-plugin's own `Cargo.toml`. |
| Memory per job (worker store) | **256 MiB** | `background.memory`, up to the ceiling | **min(2 GiB, ¼ of physical RAM)** | 256 MiB holds a symbol index of a few hundred thousand entries. The ceiling stays under wasm32's 4 GiB address space with room to spare, and at a quarter of RAM it can't push an 8 GB machine into swap alone. |
| All running jobs, together | — | — | **min(4 GiB, ½ of physical RAM)** of declared memory | A job whose cap would pass this waits in the queue ("waiting for memory"), rather than failing. |
| Concurrency | **2** running jobs per plugin, **4** in all | — | — | Queued jobs start in order. Job threads run at lowered priority: QoS utility on macOS, `nice` 10 on Linux, `THREAD_PRIORITY_BELOW_NORMAL` on Windows. |
| Fuel | **off** | — | — | Measured: fuel adds 59% to a tight loop (268.8 → 426.8 ms for 5×10⁸ iterations, §5.1). It's an `Engine`-wide setting, so either every interactive call pays it, or background stores need a second engine, which compiles every component twice. Its one advantage, determinism, buys nothing here; wall time is what users feel. The wall time plus the 5 s slice bound the CPU instead. |
| Result | ≤ **16 MiB** | — | — | The same as one read. A bigger result, such as an index the interactive instance queries later, belongs in the plugin's data store (§3.2, "Plugin data"), or in published diagnostics. |
| Queued and running jobs | **8** per plugin, **32** in all | — | — | `jobs.start` past either fails with `failed("too many jobs")` and queues nothing. So queued specs can't pile up in the host. |
| A job spec | `name` ≤ 64 bytes, `title` ≤ 200 characters, `args` ≤ **1 MiB** | — | — | Larger input goes through the data store. An oversized spec is `invalid-argument`. All queued specs together hold at most 8 MiB per plugin. |
| Undelivered results | ≤ **32 MiB** per plugin | — | — | The host keeps a result only until `job-finished` has been called with it, then drops it. A job finishing while this is full is delivered as `failed("result dropped: too many undelivered")`. |
| `process.spawn` in a job | the job's remaining wall time | — | — | `PROGRAM_DEADLINE`'s 10 s per call becomes, in a job, whatever is left of the job's wall time. Long programs should use `process.start` instead (§3.3). |

**The manifest:**

```toml
[contributes.plugin]
wasm = "plugin.wasm"
capabilities = ["fs.read:workspace", "process.spawn:cargo", "decorations"]

[contributes.plugin.background]   # optional: without it, jobs get the defaults
memory = "1GiB"                    # 1 MiB to the host ceiling, KiB/MiB/GiB units
wall = "30m"                       # 1s to 60m, s/m units
data = "256MiB"                    # the plugin data quota, up to 2 GiB (below)
```

**The install prompt** names what was declared, and the ceiling: "Runs
background jobs using up to 1 GB of memory and 30 minutes each (this computer
allows up to 2 GB)." A declaration above the ceiling is clamped to it, with a
config warning on the manifest. Background limits count as capabilities for
decision 3 (escalation): an update that raises `memory` or `wall` stops and asks
again, and lowering one never asks.

**The user's per-plugin overrides** finally get the settings stage 2 promised,
in user settings only (a project file can't change them):

```toml
[extensions.plugin_limits."acme.cargo"]
background_memory = "512MiB"   # lower than declared, or higher up to the ceiling
background_wall = "60m"
call_deadline_ms = 250          # interactive: 50 to 1000
memory = "128MiB"               # interactive: up to 256 MiB
```

An override wins over the manifest and needs no prompt, because the user wrote
it. Past a ceiling, it is clamped with a settings diagnostic. A plugin can't
read or change its overrides.

**Plugin data.** A job that builds something larger than a result (an index, a
cache) needs somewhere to keep it, and today `fs::configure` preopens only the
granted workspace roots. A writable WASI preopen can't be held to a quota: its
writes reach the disk before the host could measure them. So plugin data is a
**host-accounted store**, not a preopen. Its files live in
`<state dir>/plugin-data/<plugin id>/`, but the plugin reaches them only through
this interface, and every byte is charged before it is written:

```wit
// kiln:extension@1.1.0
interface data {
    use types.{error};
    /// A flat name: 1 to 128 characters of `a-z`, `0-9`, `.`, `_` and `-`, not starting with `.`.
    type name = string;
    /// At most 16 MiB from `offset`; fewer at the end of the file.
    read: func(file: name, offset: u64, len: u32) -> result<list<u8>, error>;
    /// Writes `bytes` (at most 16 MiB) at `offset`, growing the file as needed. Refused
    /// whole, with nothing written, when the growth would take the plugin past its quota.
    write: func(file: name, offset: u64, bytes: list<u8>) -> result<_, error>;
    /// Replaces the file with `bytes` atomically (a temporary file renamed over it).
    replace: func(file: name, bytes: list<u8>) -> result<_, error>;
    truncate: func(file: name, len: u64) -> result<_, error>;
    delete: func(file: name) -> result<_, error>;
    list: func() -> result<list<tuple<name, u64>>, error>;
    /// Bytes used and the quota.
    usage: func() -> tuple<u64, u64>;
}
```

- **Enforcement before growth.** The host keeps one counter of bytes used per
  plugin, shared by its interactive store and all its worker stores, behind
  one lock. A `write`, `replace` or `truncate` that grows a file takes the
  lock, checks used + growth ≤ quota, charges the growth, writes, and gives
  back what a failed write didn't use. Two concurrent writers can't both pass
  the check. A `replace` charges its new size while the old file still exists,
  and gives the old size back after the rename. Files hold at most 10,000
  names per plugin, also counted under the lock.
- **The quota:** 64 MiB by default. The manifest may declare
  `background.data = "1GiB"`, up to a **2 GiB** ceiling, shown in the install
  prompt and counted as an escalation like `memory`. The user can set
  `background_data` in `[extensions.plugin_limits."<id>"]`.
- **At load:** the counter starts from the directory's real size, so files
  left by a crash count. A plugin already over a lowered quota keeps reading
  and deleting, and its growing writes are refused, with
  `failed("over the data quota")`, until it is back under. It isn't unloaded.
- **Lifecycle:** kept across updates and reloads. Deleted on uninstall (the
  confirmation says so, as for secrets), and by **Clear Data** in the
  extensions list, which takes the lock, so no write races it.
- **Capability:** none, as for secrets, since it is the plugin's own. **Tier:**
  both, and the interactive instance reads what a job wrote after the job has
  ended.
- **Cards:** B7 adds it.

### 3.3 Streamed programs, backpressure and cancellation

`process.spawn` runs a program to completion and returns its output. That stays
as it is, for interactive calls. Jobs get `process.start`, which returns a
`child` resource:

```wit
// kiln:extension@1.1.0, interface process (additions)
resource child {
    /// Waits up to `timeout-ms` for output, then returns at most `max-bytes` of each
    /// stream: what has come so far, possibly nothing. `exited` is set once the program
    /// has exited and both streams are drained.
    read: func(max-bytes: u32, timeout-ms: u32) -> result<chunk, error>;
    /// Writes to its stdin; returns how many bytes it took (fewer when the pipe is full).
    write: func(bytes: list<u8>) -> result<u64, error>;
    close-stdin: func();
    /// Ends the program and everything it started (ProcessTree, §2 W-c).
    kill: func();
}
record chunk { stdout: list<u8>, stderr: list<u8>, exited: option<exit> }
record exit { code: option<s32>, killed: bool }
/// Starts `program` as `spawn` does (grant, bound path, no shell, minimal environment),
/// in `cwd` (a guest path inside a preopened root), without waiting. Only in a job.
start: func(program: string, args: list<string>, cwd: option<string>) -> result<child, error>;
```

- **Grants and environment** are `spawn`'s: the same bound path, identity check,
  minimal environment (W-a) and refusal of anything ungranted. `start` in an
  interactive call is `denied("process.start runs only in a background job")`.
- **Backpressure.** The host buffers at most **1 MiB per stream per child**.
  When a buffer is full, the host stops reading that pipe, so the program
  blocks on its next write, as in a shell pipeline. A plugin that reads slowly
  slows the program and never grows host memory. `read` returns at most 1 MiB
  per stream per call. There is no cap on the total output, since it is
  streamed.
- **Bounds.** At most **8 live children per job**. A child lives no longer
  than its job.
- **Cancellation** (from the Runs panel, `jobs.cancel`, the plugin unloading,
  reloading or being disabled, the preview being switched off, or the app
  quitting) runs in this order:
  1. `job-context.cancelled()` turns true, and blocking imports return
     `failed("cancelled")`.
  2. Every child tree gets SIGTERM on Unix. On Windows its job object is
     terminated at once (W-c).
  3. After **2 s**, the Unix trees get SIGKILL, and a guest still running is
     interrupted by the epoch.
  4. The worker store is dropped, and the Run ends **Cancelled**.

  Nothing a job started outlives this, on any OS.

### 3.4 The job API

```wit
// kiln:extension@1.1.0
interface jobs {
    use types.{error};
    type job-id = u64;
    enum job-kind { build, test, index, other }
    record job-spec {
        /// What the worker's `run-job` is told to do.
        name: string,
        args: string,
        /// Shown in the Runs list: "cargo check", "Indexing TODOs".
        title: string,
        kind: job-kind,
        /// Whether the user may cancel it from the Runs list.
        cancellable: bool,
        /// The open root it works on, as a guest path (§2 W-d): it must be the root of
        /// an open window that the plugin's fs grant reaches, else `invalid-argument`.
        /// `run-job` is told it; none for work about no root.
        root: option<string>,
    }
    enum waiting { concurrency, memory }
    /// What `state` says. A finished job's result is not here: it is handed over once,
    /// to `job-finished`, and then dropped.
    variant job-status { queued(waiting), running(progress), succeeded, failed, cancelled, timed-out, unknown }
    variant job-state { finished(result<string, error>), cancelled, timed-out }
    record progress { fraction: option<f32>, message: option<string> }
    /// Queues a job; it starts when §3.2's concurrency and memory allow. Fails with
    /// `failed` past §3.2's queue limits and `invalid-argument` past its spec limits.
    start: func(spec: job-spec) -> result<job-id, error>;
    /// Cancels a queued job (it never starts) or a running one (§3.3's order).
    cancel: func(job: job-id) -> result<_, error>;
    /// `unknown` once the job's end has been delivered.
    state: func(job: job-id) -> job-status;
}

/// Imported by a worker store only.
interface job-context {
    use jobs.{job-id, progress};
    /// Shown on the job's Run. At most 10 updates a second reach the UI; the latest wins.
    report: func(progress: progress);
    /// True once the job is being cancelled.
    cancelled: func() -> bool;
}

/// Exported: run in the worker store.
interface job-runner {
    use jobs.{job-id};
    run-job: func(job: job-id, name: string, args: string, root: option<string>) -> result<string, error>;
}

/// Exported: run in the interactive store, interactive tier.
interface job-handler {
    use jobs.{job-id, job-state};
    job-finished: func(job: job-id, state: job-state);
}
```

Capability: **`jobs`**, which grants nothing by itself; the job's imports need
their own grants. The tier is background for `run-job` and interactive for
`job-finished`.

**Unload and reload.** Unloading, reloading or disabling a plugin cancels its
running jobs (§3.3), removes its queued ones, and drops its undelivered
results. A reloaded plugin starts with no jobs, and `state` of an old id is
`unknown`.

### 3.5 The Runs model of T1570: it fits, with four additions

T1570 (ready, not built; it depends on T1565 and T1561) defines
`Run { id, kind: Build | Test | Script | Agent, title, status: Running {
progress: Option<f32> } | Passed | Failed { code } | Cancelled, summary:
RunSummary, started, finished }`, kept by the workbench, with `begin`, `update`,
`finish`, `list` and a change event. A plugin job maps onto it:

- `job-kind` `build` and `test` become `Build` and `Test`, and `index` and
  `other` become `Script`.
- `progress.fraction` becomes `Running { progress }`.
- `finished(ok)` becomes `Passed`, `finished(err)` and `timed-out` become
  `Failed { code: None }`, and `cancelled` becomes `Cancelled`.

It does **not** fit in four places, which the job card (B6) adds to T1570's
types after T1570 lands:

1. **An owner, for cancel.** T1570 cancels a run only by closing its terminal
   tab. Add `Run.owner: RunOwner::Task(tab) | Plugin { id, job }`, so the Runs
   list's cancel reaches `jobs.cancel`.
2. **Text progress.** `RunSummary` has no free text. Add `RunSummary::Text(String)`
   for `progress.message`, ≤ 200 characters, shown as data, never as markup.
3. **"Timed out"**, as `Failed` with a reason: `Failed { code: None, reason:
   Some(TimedOut) }`.
4. **Queued.** T1570's statuses start at `Running`. Add `RunStatus::Queued {
   waiting: Concurrency | Memory }`, shown as "waiting for another job" or
   "waiting for memory".
   - **Timestamps:** a queued Run has `queued_at` set and `started: None`.
   - **Starting:** moving to `Running` sets `started`.
   - **Cancelling before it starts:** the Runs list's cancel calls
     `jobs.cancel`, which removes it from the queue, and the Run ends
     `Cancelled` with `started: None`. A queued Run is never evicted, like a
     running one.

**Decision:** the fit is partial. B6 depends on T1570 and adds those four.

### 3.6 Language servers from plugins: native and run by fe-lsp

The two options:

- **(a) Native, run by fe-lsp.** The plugin's extension contributes a server
  exactly as a stage-1 `lsp` contribution does: `[contributes.lsp.<name>]`,
  with the effective-invocation consent of stage 1 §1, folder trust, crash
  budgets, restart, status and the log. Through T1645 it can also be an
  add-on server. What a code plugin adds is a narrow import for the server's
  custom methods:

  ```wit
  // fastedit:editor@1.1.0
  interface lsp {
      use kiln:extension/types@1.1.0.{buffer-id, error};
      /// Which session of the server: the one an open buffer is placed on, or the
      /// one started at an open root (a guest path, §2 W-d). A server runs one
      /// session per (name, root), so a bare name is never enough.
      variant target { buffer(buffer-id), root(string) }
      /// Sends a request the language server `server` defines (rust-analyzer's
      /// `experimental/runnables`, say) to the session `target` names, and returns
      /// its result as JSON. Needs `lsp:<server>`. Methods fastedit already routes
      /// (`textDocument/*`, `workspace/*`, `completionItem/*` and the like) are
      /// `denied`: they go through §4.1.
      request: func(server: string, target: target, method: string, params: string) -> result<string, error>;
      notify: func(server: string, target: target, method: string, params: string) -> result<_, error>;
  }
  ```

  - **Which session:** `target` must name a buffer the plugin can read
    (`buffer.read`), or a trusted open root its fs grant reaches. With no
    session there (not started, untrusted, crashed out), the call is
    `not-found`, and nothing is started for it.
  - **Which servers:** `lsp:<server>` may name a server the same extension
    contributes, or a built-in server (`rust-analyzer`, say). The prompt names
    it either way ("can send requests to rust-analyzer"). Another extension's
    server is always `denied`, so one extension can't drive another's.
  - **Tier and limits:** interactive tier, with the server's `lsp.timeouts`
    entry as the deadline, capped at the call deadline's program share. Params
    and results are each at most 16 MiB.

- **(b) A WASI server in a long-lived store.** The server is compiled to
  `wasm32-wasip2` and runs in a store with no per-call deadline and a large
  memory cap, speaking LSP over in-process streams. It would cost:
  - **New host machinery:** an fe-lsp transport over in-process streams, with
    framing, cancellation and crash detection (about one m card); a store kind
    with no deadline but a liveness check (one m card); and directory watching
    fed to the guest, since WASI has none (one s card).
  - **Memory:** real servers hold 0.5–4 GB on large workspaces. That passes the
    2 GiB background ceiling, and wasm32 cannot pass 4 GiB at all. So option (b)
    means memory64, which needs a toolchain and wasmtime support we haven't
    checked.
  - **Speed:** Cranelift code is slower than a native release build (by an
    amount not measured here), and no server would get native threads.
  - **Supply:** the servers people use don't build for WASI today. rust-analyzer
    spawns processes for proc macros and build scripts; tsserver needs Node;
    gopls needs Go's runtime and processes. So (b) serves only servers written
    for it.
  - **Its one gain** is a server sandboxed like a plugin. That is real, but it
    serves the few servers that could run that way.

**Decision:** (a) is the supported path. A plugin provides a language server
declaratively, fe-lsp runs it natively under stage 1's consent rules, and the
plugin reaches its custom methods through `lsp.request` with `lsp:<server>`.
(b) is not built. It is reconsidered if a WASI-built server worth having
appears. **Open question Q3:** may a plugin download a server binary into its
own data directory and name it in its `lsp` contribution? Consent would bind
the binary's SHA-256, as stage 1 binds a script's, so any change asks again.
Or must servers come from `PATH` or the extension's own files, as today?

**Decision (§3):** two tiers. Interactive keeps today's numbers. Background jobs
run in worker stores with a 10-minute default wall time (60-minute ceiling), a
5 s guest slice, 256 MiB by default with a declared cap up to
min(2 GiB, RAM/4), a combined min(4 GiB, RAM/2), 2 jobs per plugin and 4 in all,
no fuel, streamed and backpressured programs with tree kill, and progress in
T1570's Runs with four additions. Queues, specs and undelivered results are
bounded per plugin and in all. Each plugin has a data store whose quota is charged before every write.
**Open question Q2:** are 2 GiB per job and
4 GiB for all jobs the right ceilings, or should a job be able to ask for more
on large machines?

## 4. API gaps, with WIT sketches

Every new interface below is part of the 1.1 minor (§4.5) and lives in
`kiln:extension@1.1.0` or `fastedit:editor@1.1.0`. For each one this section
names its capability, its tier from §3 and its host-side limits. Byte offsets
are UTF-8, as in today's `range`, against the buffer `version` the call names.
Positions in files that aren't open are `(line, byte-in-line)`.

### 4.1 Language features

**Routing comes from T1645, unchanged.** A plugin's language provider is
routed as one more **add-on server** in the routing of
[q5j03ax2af-lsp-add-on-servers.md](q5j03ax2af-lsp-add-on-servers.md) §5 (the
T1645 decision record, proposed). Plugin providers come **after** every language
server of the document, in plugin id order, and are never the lead. So each
feature follows that record's table row, with nothing restated here:

| Feature | T1645 §5 row | What it means for a plugin provider |
|---|---|---|
| completion, resolve | `textDocument/completion`, "merged, one version per list", with its **Completion merging** rules; `completionItem/resolve`, "owner" | Its items are merged after the servers'. They are deduped only when final (`resolves-completions: false`) and command-free. Resolve goes back to the plugin. |
| hover | `textDocument/hover`, "merged" | Shown after the servers' contents, separated by a rule. |
| signature help | `textDocument/signatureHelp`, "first-that-answers" | Asked last, so it is used only when no server answers. |
| definition, declaration, type definition, implementation | `textDocument/definition`, `declaration`, `typeDefinition`, `implementation`, "merged" | Locations deduped by (path, range). |
| references | `textDocument/references`, "merged" | The same. |
| document symbols | `textDocument/documentSymbol`, "provider" | Used only when no placed server provides symbols. |
| workspace symbols | `workspace/symbol`, "merged across every ready session at the root" | Each provider is asked once, deduped by (name, kind, location). |
| code actions, resolve | `textDocument/codeAction`, "merged", with **Code-action dedupe**; `codeAction/resolve`, "owner" | Its actions come after the servers'. Resolve and commands go to the plugin. |
| inlay hints, resolve | `textDocument/inlayHint`, "merged"; `inlayHint/resolve`, "owner" | Ordered by position, then source. Not deduped. |
| formatting | `textDocument/formatting`, `rangeFormatting`, "provider" | That is today's order already: the plugin formatter is "the last source `lsp_format` tries" (`crates/fe-workbench/src/plugin_format.rs:2`). The existing `formatting` interface stays as it is. |
| diagnostics | `textDocument/publishDiagnostics` (pushed), "every server, merged by owner" | The existing `diagnostics` interface. Its sets are their own owner, as now. |

An **owner** for a plugin item is `(plugin id, provider id, load generation)`.
A follow-up (resolve, a command, an action's edit) is sent only while that same
load of the plugin runs. This is the plugin's equivalent of T1645's `request_if`.
After a reload it answers "the plugin reloaded", as a restarted server does.

**If T1645 changes**, each row above follows its row there, because a plugin
provider is an add-on and nothing more. If T1645 is **rejected** and
fastedit keeps one server per language, the merged rows have no merge to join.
Then card A1 is blocked, and this subsection is revised with its own table. A1
and A2 depend on T1645's cards A6 (provider and first-that-answers), A7 (merges)
and W1 (workbench call sites).

```wit
// fastedit:editor@1.1.0
interface language {
    use kiln:extension/types@1.1.0.{buffer-id, version, range, diagnostic, error, command-id};
    use buffer.{text-edit};

    flags features {
        completion, hover, signature-help,
        definition, declaration, type-definition, implementation, references,
        document-symbols, workspace-symbols, code-actions, inlay-hints,
    }
    record provider {
        languages: list<string>,
        features: features,
        completion-triggers: list<string>,
        signature-triggers: list<string>,
        /// False: its completion items and code actions are final (T1645's dedupe terms).
        resolves-completions: bool,
        resolves-code-actions: bool,
        resolves-inlay-hints: bool,
    }
    /// Adds or replaces this plugin's provider `id`. Needs `language` and `buffer.read`.
    register: func(id: string, provider: provider) -> result<_, error>;
    unregister: func(id: string) -> result<_, error>;

    record at { buffer: buffer-id, version: version, offset: u64 }
    record position { line: u32, byte: u32 }
    /// `path` is a guest path (§2 W-d).
    record location { path: string, start: position, end: position }
    enum completion-kind { text, method, function, field, variable, class, module, property, keyword, snippet, color, file, folder, constant }
    record completion-item {
        label: string, kind: completion-kind, detail: option<string>,
        /// Markdown, rendered as the editor renders a server's.
        documentation: option<string>,
        filter-text: option<string>, sort-text: option<string>,
        edit: text-edit, additional-edits: list<text-edit>,
        snippet: bool, commit-characters: list<string>,
        command: option<command-id>,
        /// Opaque to the host; handed back on resolve.
        data: option<string>,
    }
    record completion-list { incomplete: bool, items: list<completion-item> }
    record hover { markdown: string, range: option<range> }
    record signature { label: string, documentation: option<string>, parameters: list<range> }
    record signature-help { signatures: list<signature>, active-signature: u32, active-parameter: option<u32> }
    enum location-kind { definition, declaration, type-definition, implementation, references }
    enum symbol-kind { file, module, namespace, class, method, property, field, %constructor, %enum, %interface, function, variable, constant, %string, number, boolean, array, object, key, struct, event, operator, type-parameter }
    /// Flat, parents before children: WIT has no recursive types.
    record symbol { name: string, kind: symbol-kind, detail: option<string>, range: range, selection: range, parent: option<u32> }
    record workspace-symbol { name: string, kind: symbol-kind, container: option<string>, location: location }
    record code-action {
        title: string, kind: option<string>, preferred: bool, disabled: option<string>,
        /// Edits to the request's buffer only; applied as one undo step under the plugin's origin.
        edits: list<text-edit>,
        command: option<command-id>, data: option<string>,
    }
    enum hint-kind { %type, parameter, other }
    record inlay-hint {
        offset: u64, label: string, kind: hint-kind, tooltip: option<string>, pad-left: bool, pad-right: bool,
        /// Opaque to the host; handed back on resolve, as completion items' `data` is.
        data: option<string>,
    }
}

/// Exported; every function is interactive tier.
interface language-handler {
    use language.{at, completion-list, completion-item, hover, signature-help, location-kind, location, symbol, workspace-symbol, code-action, inlay-hint};
    use kiln:extension/types@1.1.0.{buffer-id, version, range, diagnostic, error};
    complete: func(provider: string, at: at, trigger: option<string>) -> result<completion-list, error>;
    resolve-completion: func(provider: string, item: completion-item) -> result<completion-item, error>;
    hover: func(provider: string, at: at) -> result<option<hover>, error>;
    signature-help: func(provider: string, at: at, trigger: option<string>) -> result<option<signature-help>, error>;
    locations: func(provider: string, kind: location-kind, at: at) -> result<list<location>, error>;
    document-symbols: func(provider: string, buffer: buffer-id, version: version) -> result<list<symbol>, error>;
    /// `root` is the open root being searched (a guest path), as T1645 asks each session at a root.
    workspace-symbols: func(provider: string, root: string, query: string) -> result<list<workspace-symbol>, error>;
    code-actions: func(provider: string, buffer: buffer-id, version: version, range: range, diagnostics: list<diagnostic>) -> result<list<code-action>, error>;
    resolve-code-action: func(provider: string, action: code-action) -> result<code-action, error>;
    inlay-hints: func(provider: string, buffer: buffer-id, version: version, range: range) -> result<list<inlay-hint>, error>;
    /// Fills in a hint's tooltip, label or edits later; only for providers with `resolves-inlay-hints`.
    resolve-inlay-hint: func(provider: string, hint: inlay-hint) -> result<inlay-hint, error>;
}
```

- **Capability:** `language`, together with `buffer.read`. **Every edit a
  provider hands over is a write**, so the host enforces `buffer.write` on
  each one before it applies it, on the item as answered and again on the item
  as resolved:
  - **A completion's main `edit`** is allowed without `buffer.write` only if
    its range is exactly the **host's completion span**, or an empty range at
    the caret. The host computes that span itself, from the buffer and never
    from the plugin: the run of the language's word characters touching the
    caret, from its start to the caret. It is the prefix the user typed, which
    the editor already filters the list by, and which accepting any item
    replaces anyway. A range any wider (the rest of the word after the caret,
    the whole line, a span that merely contains the caret) needs
    `buffer.write`. Without the grant, the item is dropped and logged.
  - **A completion's `additional-edits`** (auto-imports and the like) always
    need `buffer.write`. Without the grant, the item is dropped, never
    applied with its additional edits cut off, since half an auto-import
    breaks code.
  - **A resolved completion** is checked again by the same two rules, against
    the same host span, when it comes back from `resolve-completion`. If it fails them, accepting it
    applies the item as first answered, if that passed, and otherwise nothing.
  - **A code action's `edits`** need `buffer.write`. Without it, the edits
    are dropped and the action is kept only if it has a command. A resolved
    action is checked again.
  - Edits to files other than the request's buffer are left out of 1.1.
- **Root:** `workspace-symbols` is asked once per open root, with that root,
  and only for roots the plugin's fs grant reaches. Without a grant it is
  asked only with the root of the active window.
- **Follow-ups:** `resolve-completion`, `resolve-code-action` and
  `resolve-inlay-hint` go to the item's owner (plugin, provider, load
  generation) and only while that load runs, as T1645's owner rows require.
  An item from an earlier load is never sent to a reloaded plugin.
- **Tier:** interactive. The deadline is 100 ms, or the feature's
  `lsp.timeouts` entry if that is shorter. On a timeout the plugin's answer is
  left out of the merge, as a slow server's is. An *overrun* of the call
  deadline unloads the plugin, as for any call.
- **Coalescing:** at most one call per provider per feature is in flight, and
  a newer request replaces the one queued behind it. Typing never builds a
  queue.
- **Host limits:**
  - per answer: 2,000 completion items, 64 KiB of hover markdown, 16
    signatures, 1,000 locations, 10,000 document symbols, 1,000 workspace
    symbols, 100 code actions (with at most 10,000 edits each), and 2,000 inlay
    hints per range;
  - over a limit, the answer is cut and logged once per load;
  - an item with a range off a character boundary, or past the text, is
    dropped and logged. Neither unloads the plugin.

### 4.2 Panels: a declarative view tree, no webviews

```wit
// fastedit:editor@1.1.0
interface panels {
    use kiln:extension/types@1.1.0.{error, command-id};
    enum tone { normal, muted, accent, success, warning, error }
    record text-node { text: string, tone: tone, mono: bool, wrap: bool }
    /// Every interactive node carries `label`, its accessible name.
    record button-node { label: string, icon: option<string>, command: option<command-id>, enabled: bool }
    record toggle-node { label: string, on: bool, enabled: bool }
    record input-node { label: string, value: string, placeholder: option<string> }
    record list-node { label: string, multi-select: bool }
    record tree-item-node { label: string, icon: option<string>, expandable: bool, expanded: bool, detail: option<string>, tone: tone }
    variant node-kind {
        column, row, scroll,
        heading(string), text(text-node), icon(string), badge(text-node),
        button(button-node), toggle(toggle-node), input(input-node),
        %list(list-node), tree-item(tree-item-node), separator, progress(option<f32>),
    }
    /// The tree as a flat arena: `nodes[0]` is the root, `children` are indices
    /// into `nodes`. `id` is the plugin's own, unique in the view.
    record node { id: string, kind: node-kind, children: list<u32> }
    record view { nodes: list<node> }

    /// Replaces panel `panel`'s view. Needs `panels`; the panel is one the manifest declares.
    set-view: func(panel: string, view: view) -> result<_, error>;
    /// Replaces the subtree at node `id` with `view`'s root.
    patch: func(panel: string, id: string, view: view) -> result<_, error>;
    /// Shows the panel and focuses it, only while a command the user ran is being handled.
    reveal: func(panel: string) -> result<_, error>;
}

/// Exported; interactive tier.
interface panel-handler {
    variant panel-event {
        activated(string),            // button pressed, tree item or list row opened (Enter or double-click)
        toggled(tuple<string, bool>),
        input(tuple<string, string>), // debounced 150 ms
        submitted(tuple<string, string>),
        selected(tuple<string, list<string>>),
        expanded(tuple<string, bool>),
        shown, hidden,
    }
    on-event: func(panel: string, event: panel-event);
}
```

```toml
[[contributes.plugin.panels]]
id = "todos"            # the panel is "<plugin id>.todos"
title = "TODOs"
icon = "list-checks"    # a name in the app's icon set
dock = "left"           # left, right or bottom
```

- **Node kinds.** Containers are `column`, `row` and `scroll`. Content is
  `heading`, `text`, `icon`, `badge`, `separator` and `progress`. Controls are
  `button`, `toggle`, `input`, `list` and `tree-item`. A tree-item's children
  are its nodes; `expanded` with no children asks the plugin through
  `expanded`, so a big tree loads lazily. Lists and trees are virtualized by the
  host.
- **Theming.** There are no colours or fonts in the WIT. `tone` maps to theme
  tokens, `mono` to the editor font, and `icon` to names in the app's icon set
  (unknown names show nothing and are logged). So every theme, Yohen and Hakuji
  included, styles plugin panels as it styles built-ins, contrast rules and all.
- **Accessibility.** Every `button`, `toggle`, `input`, `list` and `tree-item`
  needs a non-empty `label`; a view without one is `invalid-argument` and isn't
  shown. The host maps each to its AccessKit role (button, check box, text
  input, list and list item, tree and tree item). It owns focus, keyboard
  navigation (document order, arrow keys in lists and trees, Enter to activate)
  and scrolling. A plugin can't trap focus.
- **Limits:**
  - 10,000 nodes per view and depth 32;
  - 16 KiB of text per node and 1 MiB per view;
  - 8 panels per plugin;
  - 20 updates a second per panel, coalesced so the latest view wins.
  An invalid view is refused whole, leaving the last good one shown.
- **Capability:** `panels`. **Tier:** `on-event` is interactive. `set-view`
  and `patch` may also be called from a job's worker store, so an index can
  fill a panel while it is being built.

**Opening a location.** Neither the WIT nor the plugin-callable command
allowlist can open a file today; the allowlist holds editor movement and
edits. A panel whose rows are places in the code (TODOs, search results, test
failures) needs one operation:

```wit
// fastedit:editor@1.1.0
interface navigation {
    use kiln:extension/types@1.1.0.{error};
    use language.{location};
    /// Opens `location` in an editor and selects its range, as Go to Definition does.
    open: func(location: location) -> result<_, error>;
}
```

- **Only on a user action.** `open` works only during a call that delivers a
  user's act: a panel's `activated` or `submitted` event, a command dispatched
  with `Actor::User`, or a status item's click. The host marks that call and
  nothing else. At any other time (an event handler, a job, a timer-like
  loop), it is `denied("open needs a user action")`. A plugin can't pull the
  user's focus around.
- **Containment.** `path` is a guest path (§2 W-d), mapped back and
  canonicalized. It must lie inside an open, trusted root that the plugin's
  `fs.read:workspace` grant reaches, else `denied`. A symlink leading out of
  the root is refused, as WASI refuses it.
- **Capability:** `fs.read:workspace`, since the plugin can read the file
  anyway. **Tier:** interactive. **Limits:** one `open` per user action; a
  second is `denied`.

### 4.3 Tasks and runs

```wit
// fastedit:editor@1.1.0
interface tasks {
    use kiln:extension/types@1.1.0.{error};
    enum group { build, test, run }
    record task {
        label: string,
        /// A program the plugin holds `process.spawn:<program>` for: it runs at the bound path.
        program: string,
        args: list<string>,
        /// `/`-separated, relative to the root; checked as tasks.toml's `cwd` is.
        cwd: option<string>,
        env: list<tuple<string, string>>,
        group: option<group>,
        /// As tasks.toml's `problem_matcher`, such as "rustc".
        problem-matcher: option<string>,
    }
    /// Offers this plugin's tasks in the task picker. Needs `tasks`.
    register-provider: func() -> result<_, error>;
    /// Asks the host to call `provide-tasks` again, for a root whose tasks changed.
    changed: func(root: string) -> result<_, error>;
    // No `run`: a plugin can't start a task. Only the user does (below).
}

/// Exported; interactive tier. Called lazily, when the picker opens or after `changed`.
interface task-provider {
    use tasks.{task};
    use kiln:extension/types@1.1.0.{error};
    provide-tasks: func(root: string) -> result<list<task>, error>;
}
```

- **Only the user runs a task.** A plugin task is listed beside
  `.tenmoku/tasks.toml`'s tasks and the detected ones, as "from <plugin name>".
  It starts only from the task picker or a Run Task command dispatched with
  `Actor::User`. The task-running commands are not on the plugin-callable
  allowlist, so a plugin can't start one through `commands.invoke` either. It
  then runs as any task does, in a terminal tab with the user's environment,
  its problems going to the Problems panel (`task_run.rs`). Once T1570 lands,
  that makes it a Run through T1570's own task hook, with nothing
  plugin-specific.
- **What a plugin runs by itself** goes through a job (§3.4) and
  `process.start` or `process.spawn` (§3.3). There it gets the minimal
  environment, the wall time, the concurrency limits and tree kill, never the
  user's environment and an unbounded terminal. A task is the user choosing to
  run a command outside those bounds, and the picker shows its full program
  path and arguments before it runs.
- **Why a spawn grant is needed.** A task runs the user's program with the
  user's rights, outside the sandbox. If plugins could name any program as a
  task, they would have a way around `process.spawn`. So a task's `program`
  must be one the plugin holds a spawn grant for, and runs at the bound path.
  Arguments are a vector and never pass through a shell (W-b's `.cmd` rule
  applies). Tasks run only in trusted roots, as `task_defs.rs` requires.
- **Capability:** `tasks` (plus `process.spawn:<program>` for each program).
  **Tier:** `provide-tasks` is interactive, and its result is cached until
  `changed`. `root` is a trusted open root (a guest path); `provide-tasks` is
  asked per root, and `changed` with any other root is `invalid-argument`. **Limits:** 200 tasks per provider per root, 64 KiB of
  arguments per task, and 64 environment entries, whose names can't be `PATH`
  or, on Windows, `SystemRoot` or `PATHEXT` (the program is bound, but its
  children aren't).

### 4.4 Settings contributions

```toml
[contributes.plugin]
settings = "settings.schema.json"   # a JSON Schema, inside the extension
```

The schema is a JSON Schema subset: an object whose properties are of type
`string`, `number`, `integer`, `boolean`, `array` (of those) or `enum`, with
`default` and `description`, plus two keywords of ours:

- `"x-scope": "user" | "project"`. The default is `"user"`. A `project` key
  may also be set in a trusted project's settings.
- `"x-secret": true`. The value is a secret. It is never read from or written
  to a settings file; the settings UI stores it in the plugin's secret store
  under the key's name (decision 5), and the plugin reads it with
  `secrets.get`. A secret key found in a settings file is a warning, and is
  ignored.

The user writes them as

```toml
[plugins."acme.cargo"]
check_command = "clippy"
features = ["serde"]
```

in user settings, and a trusted project's `.tenmoku/settings.toml` may set the
`project`-scoped keys. Each layer is checked against the schema with kiln's
`Layer::check_schema`, which gives positioned diagnostics where config problems
are shown already. An untrusted project's keys are dropped, with the existing
"applies only in trusted folders" diagnostic, as for `[lsp]`.

```wit
// kiln:extension@1.1.0
interface settings {
    use types.{error};
    /// The plugin's effective settings as a JSON object: the schema's defaults, then the
    /// user's table, then, with `root`, that project's `project`-scoped keys if the root is
    /// trusted. `root` is an open root (a guest path); one that isn't open is
    /// `invalid-argument`. Secrets are absent.
    get: func(root: option<string>) -> result<string, error>;
}
```

- **Several projects.** The plugin host is app-wide and serves every window, so
  there is no single project. A plugin passes the root it is working for: the
  root of the buffer it was asked about, the job's `root` (§3.4) or a task
  provider's `root`. Without one, it gets the user's settings only.
- **Change notification** is the existing `config-reloaded` event.
- **Capability:** none. Reading its own settings is like reading its own
  secrets: nothing another plugin owns is reachable.
- **Tier:** interactive (and allowed in jobs).
- **Limits:** a schema of at most 64 KiB and 256 keys, and effective settings
  of at most 256 KiB. A manifest whose schema breaks these, or isn't the subset,
  is a load error.
- **Separate from the host's limits.** `[extensions.plugin_limits."<id>"]`
  (§3.2) is the host's table, not the plugin's, and a plugin can't read it.

### 4.5 Versions: what `compat.rs` has, what 1.1 needs, and when the worlds are stable

**What exists** (`kiln/crates/kiln-plugin/src/compat.rs`):

- `HOST_API` is `1.0`, and `DEPRECATED` is empty.
- `Compat::check` accepts the host's major with a minor at or below the host's,
  and a deprecated major only inside its window.
- `Adapter { serves, link }` and `link_adapters` add imports an older minor
  needs to the `Linker`, and `link()` links none.

wasmtime's linker already satisfies an import of `…@1.0.0` with a host
interface at `@1.1.0` when the change is additive. So the imports of an older
plugin need no adapter.

**What a 1.1 minor needs:**

1. **Additive changes only.** New interfaces (`jobs`, `job-context`, `data`,
   `settings`, `language`, `panels`, `navigation`, `tasks`, `lsp`) and new functions in
   existing ones (`process.start`) are allowed. **Never** a new case in an
   existing `variant` or `enum` (such as `error` or `event`), or a new field
   in an existing record. The component model treats those as different
   types, so a 1.0 guest would no longer link. New cases go into new types.
2. **Exports.** 1.1 adds exports (`job-runner`, `job-handler`,
   `language-handler`, `panel-handler`, `task-provider`), and a 1.0 component
   has none of them. Today's `Adapter` covers only imports. So the host keeps
   **one generated world per supported minor**: `extension@1.0` and
   `extension@1.1`, and fastedit's `editor-extension@1.0` and `@1.1`. It
   instantiates a component against the world its manifest's `engine` minor
   names (`compat::world_for(needed)`). A 1.0 plugin is never asked for a 1.1
   export.
3. **The bump.** `HOST_API` goes to `1.1`, and the WIT packages to `@1.1.0`.
   `tenmoku-plugin`'s template and the author guide name `engine.fastedit =
   "1.1"` for the new interfaces.
4. **A frozen record of each released minor.** `wit/released/1.0/*.wit` is a
   copy of every WIT file as released, and a test fails if a released minor's
   files differ from that copy. Without it, rule 1 is only a promise.
5. **Tests.** A checked-in 1.0 component (today's `tests/fixtures/commands.wat`,
   frozen) loads on the 1.1 host and runs its commands and events. A 1.1
   fixture that uses a new interface is refused by a host pinned at 1.0
   (`NeedsNewerHost`).

**The preview worlds are declared stable** (the "preview" wording dropped from
`world.wit`, the guide and `extensions.plugins_preview`'s description, and the
semver promise of the accepted record's *Versioning* in force) only when **all**
of these hold:

1. The 1.1 interfaces of §3 and §4 have shipped in a release. Each is used by
   at least one first-party plugin of §5.3 in that release, and that plugin's
   tests pass in CI. The coverage:
   - `jobs`, `job-context`, `job-runner`, `job-handler` and `process.start`:
     D1 and D2;
   - `data`: D1 (the last check's results);
   - `tasks`, `task-provider` and `lsp`: D1;
   - `panels`, `panel-handler` and `navigation`: D2;
   - `language` and `language-handler`: D3;
   - `settings`: all three.
2. The adapter path is proven: card C1's 1.0-on-1.1 test passes, and so does
   the frozen-WIT check of every released minor.
3. **Windows parity:** the kiln-plugin, fe-plugin and fe-workbench plugin tests
   pass in the `windows` job, with no `cfg(unix)` test lacking a named Windows
   twin (§2.3).
4. No incompatible WIT change for **two consecutive releases, and at least 8
   weeks**, after condition 1.
5. The author guide (`docs/plugins/authoring.md`) documents every interface
   with a working example, and the `tenmoku-plugin` template builds against
   1.1.
6. Signing (§4.6) is either built for registry installs, or the user decides
   that "stable" covers local installs only.
7. **The user signs off** on the declaration.

Whether `extensions.plugins_preview` then defaults to on is a separate
decision for the user, made at that sign-off.

### 4.6 Signing and the registry

**Where things stand.** The grammar registry is being built:

- **T1481**: one hashed artifact per first-party grammar, an unsigned
  `index.json` and per-pack tarballs, in a new crate `crates/fe-registry`
  (blocked: lent to argon).
- **T1482**: the human card, done. It created the public repo
  `tenmoku-editor/registry`, a write credential for it alone, and a new Ed25519
  key, separate from the update key.
- **T1483**: the index signed with Ed25519 (T1427's update-feed scheme, its
  `update_signature.rs` reused) and published from CI.
- **T1484**: the app fetches, verifies and caches the signed index, refusing
  unsigned, tampered or rolled-back ones.
- **T1485**: first-open install of a language or its pack from the verified
  index.
- **T1486**: the old `grammars-1` release retired.

The plugin manifest reserves a `signature` slot that nothing verifies
(`PluginManifest::signature_verified` is always false, `manifest.rs:154`).

**Proposal for the deferred question Q9:**

- **Plugins join the same index.** `fe-registry` gains a plugin entry: id,
  version, the archive's SHA-256 and size, `engine`, licence, the manifest's
  capabilities and its `background` limits. It is signed by the registry key of
  T1482, the same way (T1483). The app installs a plugin from the index only if
  the archive's hash matches the verified entry and the manifest's capabilities
  are the ones the entry lists. The install prompt then shows the
  registry-listed capabilities. Updates come from the index, and decision 3
  (asking again on escalation) still applies to every update.
- **No per-author signing keys in stage 3.** The registry's signature vouches
  for the bytes it reviewed. A publisher-key scheme (author keys, key rotation,
  revocation) is a later question. The manifest's `signature` slot stays
  reserved and unverified.
- **Local installs** (a zip or a dev link) stay allowed. They are listed as
  **"unsigned (installed from a file)"** in the extensions list and in the
  prompt.
- **Before the registry lists any plugin**, the licence policy of the accepted
  record §6 applies ("the registry case gets a counsel review before any
  registry ships"), and so does a review policy for plugin submissions
  (Q4).

**Decision (§4):**
- Plugin providers are T1645 add-ons, last in server order, under T1645's §5
  rows. If T1645 changes, they follow it; if it is rejected, A1 blocks.
- Panels are a flat-arena view tree with tones and icon names only, mandatory
  labels and host-owned focus.
- Plugin tasks run granted programs only, in trusted roots, and only when the
  user picks them. A plugin's own runs are jobs.
- `navigation.open` works only during a user action, inside a granted root.
- Settings come from a JSON Schema subset with user and project scope, and
  secret keys go to the secret store.
- 1.1 is additive, with one generated world per minor and a frozen copy of
  each released minor's WIT. The worlds are stable when the seven conditions
  above hold.
- Plugins join the signed grammar index of T1481–T1486, with no author keys
  yet, and local installs are marked "unsigned".

**Open question Q4:** who reviews plugin submissions to the registry, and do
unsigned local installs stay allowed after the API is stable, or move behind a
developer-mode switch?

## 5. Core vs plugin, measured

### 5.1 The cost model

The measurements were run on this repo's kiln-plugin and fe-plugin at the base:

- **Machine:** Apple M3 Max (Mac15,11, 14 cores, 36 GiB), wasmtime 48.0.8,
  `--release`.
- **Load:** a load average of about 10 from other lanes. So the medians are
  given with minimums, and the numbers are good to roughly ±30%.
- **Memory:** resident set size from `ps -o rss`.
- **Harness:** temporary, and not committed. Its full source is in
  Appendix A, so anyone can run it again.

**Commands:**

```zsh
# kiln-plugin's runner (thread hop, epoch ticker, canonical ABI), startup and the cache.
# Needs `wat = "1.262.0"` added to kiln-plugin's [dev-dependencies] for the run
# (already in Cargo.lock), and Appendix A's file as kiln/crates/kiln-plugin/tests/zz_measure.rs.
CARGO_TARGET_DIR=<scratch>/target cargo test --offline --release -p kiln-plugin --test zz_measure --no-run
cd kiln/crates/kiln-plugin && <the test binary it printed> --nocapture --test-threads 1

# A real Rust component (fe-plugin's editor guest), Appendix A's test appended to
# crates/fe-plugin/tests/editor.rs.
CARGO_TARGET_DIR=<scratch>/target cargo test --offline --release -p fe-plugin --test editor zz_measure_editor -- --nocapture
```

**Per-instance memory and threads**

| What | Measured | Note |
|---|---|---|
| A Rust component (`editor.wasm`, 92 KB), compiled once, 100 instances each instantiated with its `init` run | **65 KiB** resident per instance (22,256 → 28,784 KiB), and 66 KiB after one read each | The compiled code is shared, and wasmtime maps linear memory lazily, so untouched pages cost nothing. |
| A plugin through `Plugin::load` (its own compile, store and thread, `init`), 100 of them | **164 KiB** resident each (35,040 → 51,504 KiB), loaded in 0.67 ms each | A tiny WAT component, so this is mostly the runner's own cost. |
| A thread (every plugin has one) | **16 KiB** resident each; spawned in 10 µs | Its 2 MiB stack is address space, not memory. |
| Address space per linear memory | not resident | wasmtime reserves address space for each linear memory and its guards. It costs no RAM. |

**Startup: cold, and with the compile cache**

| Component | Cold compile (median of 5) | First cached load (compile + write) | Cached load (median of 5) | Cache entry |
|---|---|---|---|---|
| `kiln-plugin/tests/guests/fs/fs.wasm` (93,436 B, Rust) | **49.2 ms** | 50.7 ms | **1.26 ms** | 337,224 B |
| `fe-plugin/tests/guests/editor/editor.wasm` (92,010 B, Rust) | **46.2 ms** | 49.1 ms | **1.37 ms** | 299,200 B |
| A synthetic core module of 2,825,471 B (4,000 functions) | **997 ms** | — | **39.8 ms** (deserialize) | 5,994,536 B |

So Cranelift compiles about **2–3 MB of wasm a second** on this machine, and a
cached load is **35–40× faster** than a cold compile. On Windows today every
load is a cold one (W-g), which is why P7 matters.

These cold compiles ran on one thread. The workspace's `wasmtime` entry
(`Cargo.toml:225`, default features off) doesn't enable wasmtime's
`parallel-compilation` feature (`cargo tree -e features -i wasmtime` lists
`async`, `component-model`, `cranelift`, `runtime`, `std` and `wat` for
kiln-plugin). Turning it on should shorten the cold compile of a large
component on a multi-core machine. That wasn't measured, and it is left to the
card that next touches the engine.

**Per-call boundary cost**

| Call | Median | Min |
|---|---|---|
| `run-command` through the runner, 16 B of args | **6.5 µs** | 3.0 µs |
| … 4 KiB | 5.7 µs | 3.3 µs |
| … 64 KiB | 6.8 µs | 5.0 µs |
| … 1 MiB | **30 µs** | 25.5 µs |
| … 8 MiB | **388 µs** | 184 µs |
| `handle-events`, 1 `buffer-changed` event (4 ranges) | 5.0 µs | 2.7 µs |
| … 100 events | 10.2 µs | 8.5 µs |
| … 10,000 events | **569 µs** | 563 µs |
| `editor.wasm`: `buffer.read` of 100 B and `log` it back, in the store with no thread hop | **1.6 µs** | 1.5 µs |
| … 64 KiB | 20.9 µs | 17.5 µs |
| … 1 MiB | **292 µs** | 282 µs |
| … 8 MiB | **2.36 ms** | 2.28 ms |

The runner rows include the thread hop: a send on a channel and a reply. The
editor rows are a full round trip: the host copies the text in, the guest
formats it, and the guest copies it back out through `log`. For comparison, a
`String` clone of 8 MiB on the host took 156 µs.

**CPU, with and without fuel:** 5×10⁸ iterations of a tight loop took
**268.8 ms** on an engine with epochs only, and **426.8 ms** with epochs and
fuel (+59%). How much slower guest code is than native wasn't measured.

### 5.2 What it means for moving built-ins out

**The user's memory question: no.** A plugin's own overhead is 65–165 KiB
resident, plus 16 KiB for its thread. Ten first-party plugins would cost under
2 MiB. What a plugin really costs in memory is the data it has to **mirror**.

- A built-in shares the buffer's rope, the syntax tree and the project index by
  reference.
- A plugin must copy whatever it reads into its own linear memory. A plugin
  that keeps a copy of a 10 MiB file holds 10 MiB more than a built-in doing
  the same job.

So memory grows with the data a plugin mirrors, not with how many plugins
there are.

**The speed question: it depends on where the work sits.**

- **First launch:** 46 ms per 92 KB component, and about 1 s for a 2.8 MB one.
  Later launches take 1.3 ms per 92 KB. That is fine for features that
  wake up after the window shows. It is not fine for anything the first frame
  needs (none of today's plugins).
- **Per call:** 3–7 µs and a thread hop for a small payload. That is negligible
  for a command, an event batch or a background job. But every hop is a copy:
  a whole 1 MiB buffer costs about 0.3 ms to read and send back, and an 8 MiB
  one 2.4 ms. A 120 Hz frame is 8.3 ms. So a plugin can never be on the
  per-keystroke-per-frame path, and stage 2 already keeps it off the paint
  path.

**So the line is:** anything per keystroke, per frame, or holding the buffer
or the tree stays core. Anything that runs on a command, an event or in the
background can be a plugin.

| Built-in | Stays core? | Reason |
|---|---|---|
| Editing (fe-buffer, fe-editor) | **Core** | Every keystroke edits the rope, undo, anchors and selections in place, at frame rate. A hop and a copy per keystroke would cost more than the edit itself. |
| Vim (fe-vim) | **Core** | A per-key modal engine wired into editor state (marks, registers, jumps, the dot command), held to nvim by oracle tests. Each key would cross the boundary at least twice. |
| Rendering (GPUI, fe-display, kiln-ui) | **Core** | It runs every frame within an 8.3 ms budget, and stage 2's rule already keeps plugins off the paint path. Decorations are host-kept copies for this reason. |
| Syntax (fe-syntax, tree-sitter) | **Core** | Incremental parses on every edit, and highlights every frame. New *languages* already arrive at runtime as sandboxed wasm grammars (stage 1 §6); that is the extension path for syntax, and it stays. |
| LSP client (fe-lsp) | **Core** | Process management, trust, crash budgets and T1645's routing. It is the host plugins plug into (§3.6, §4.1), not a feature to move. |
| Terminal (fe-terminal) | **Core** | PTYs, OS handles and high-throughput output. A plugin would need native process control the sandbox exists to deny. |
| Search (project search and the finder) | **Core** | Multi-threaded scans over the whole tree. In a plugin every file would be copied into wasm memory (0.3 ms per MiB, as above), on one thread per plugin. |
| AI chat | **Core** | Unchanged: the stage-2 decision keeps it built in. |
| Detected tasks (`task_defs.rs`: Cargo, npm, Make) | **Could be a plugin** | They run on demand, are small and own their data, and §4.3's task provider is exactly their shape. They move only if dogfood D1 shows parity, and that is a separate decision, not this record's. |
| Built-in colour swatches, TODO badges | **Core** | Both are drawn from text in view every frame (`default_colors`, `editor.todo_comments`), so they belong with syntax. |

### 5.3 Dogfood: three first-party plugins

Each is a new first-party plugin under `extensions/plugins/<name>/`. Each is
built like the test guests (a prebuilt `.wasm` checked in beside its source and
a build script; CI never builds guests, per the stage-2 decision), and each is
shipped off by default while the API is in preview.

1. **`tenmoku.cargo`: Cargo tasks and a background check.** It provides
   Cargo's tasks (build, test, run, clippy, and each binary and example). It
   runs `cargo check --message-format=json` as a job on save, streams its
   output and reports "Checking 12/40 crates". It publishes the diagnostics
   and shows the run in Runs. Its command **Cargo: Test at Cursor** asks the
   built-in rust-analyzer's session for the buffer
   (`lsp.request("rust-analyzer", buffer(id), "experimental/runnables", …)`)
   which test is under the caret. It then runs that `cargo test` as a job,
   since a plugin can't start a task (§4.3).
   - **Exercises:** tasks (§4.3), jobs and `process.start` streaming with
     backpressure and tree kill (§3.3, §3.4), Runs (§3.5), `lsp.request` with
     `lsp:rust-analyzer` (§3.6), the `data` store (the last check's
     results, §3.2), diagnostics, and settings (`check_command`, `features`,
     §4.4). On Windows it also
     exercises W-a, W-b, W-c and W-d, since `cargo.exe` lives under
     `%USERPROFILE%` and paths have drive letters.
2. **`tenmoku.todos`: a TODO panel.** A background job scans the workspace for
   `TODO`, `FIXME`, `HACK` and the other keywords the built-in badges know. It
   fills a panel tree grouped by file and refreshes on `buffer-saved`.
   Activating an item opens it with `navigation.open`.
   - **Exercises:** panels (§4.2) with a big lazy tree, `set-view` from a
     worker store, `navigation.open` on a user action, jobs, `fs.read:workspace` through preopens (W-d on
     Windows), and settings (the keyword list and excluded globs).
3. **`tenmoku.paths`: path completion.** It completes relative paths inside
   string literals (`"./src/…"`) from the filesystem.
   - **Exercises:** language features (§4.1), with completion merged after
     the language servers' items under T1645's completion row, `resolve` for
     file details, inlay hints with `resolve-inlay-hint` (the size of the file
     a path names), `fs.read:workspace`, guest-path mapping on Windows, and
     settings (the languages and the trigger characters).

**Decision:** no built-in moves to a plugin in stage 3. Editing, vim,
rendering, syntax, the LSP client, the terminal, search and the AI chat stay
core, for the reasons in the table. The detected tasks may follow only after D1
proves parity, and only by a later decision. The three plugins above dogfood
every new interface. **Open question Q5:** once the API is stable, should those
three ship installed and on by default, or be offered as optional installs?

## 6. Implementation plan

The cards are of size **s** or **m** only, Windows cards first. Host: **Linux-OK**
means the work and its gate need no Mac (any Windows-only code in it is proven
by the `windows` CI job); **Mac-only** means a gate step needs this Mac.
"Win" says whether the card is Windows-relevant. T1645's cards are named as in
its §8 (A6, A7, W1); here they are written **T1645-A6** and so on, to keep them
apart from this plan's ids.

| Id | Size | What | Scope (globs) | Depends on | Host | Win | New test files |
|---|---|---|---|---|---|---|---|
| P1 | s | Portable names and paths (W-e): contributed paths refuse `\` on every OS, `process.spawn:` names refuse `/`, `\` and `:`, and a name ending in a `PATHEXT` extension is a warning. | `kiln/crates/kiln-command/src/capability.rs`, `kiln/crates/kiln-command/tests/capability_portable.rs`, `kiln/crates/kiln-core/src/extension.rs`, `kiln/crates/kiln-core/tests/extension_portable_paths.rs`, `kiln/crates/kiln-plugin/src/manifest.rs`, `kiln/crates/kiln-plugin/tests/manifest.rs` | — | Linux-OK | yes | `kiln/crates/kiln-command/tests/capability_portable.rs`, `kiln/crates/kiln-core/tests/extension_portable_paths.rs` |
| P2 | m | `kiln_core::platform::ProcessTree` (W-c): a Unix process group with SIGTERM, then SIGKILL after 2 s; on Windows a job assigned while the process is suspended, then resumed, and `TerminateJobObject`. kiln-plugin's `run` and `Running` use it. | `kiln/crates/kiln-core/src/platform/process_tree.rs`, `kiln/crates/kiln-core/src/platform/mod.rs`, `kiln/crates/kiln-core/Cargo.toml`, `kiln/crates/kiln-core/tests/process_tree.rs`, `kiln/crates/kiln-plugin/src/process.rs` | — | Linux-OK | yes | `kiln/crates/kiln-core/tests/process_tree.rs` (a `sh`/`.cmd` script starts a grandchild holding stdout: killing the tree ends both; SIGTERM grace on Unix; nothing escapes the job on Windows) |
| P3 | s | Spawn environment and batch files (W-a, W-b): `system_path()` and `minimal_environment` per OS, `ProgramIdentity.interpreter` for `.bat`/`.cmd` with the prompt's flag, `programs()`'s fallback, and `process.wit`'s doc comment (both environments, 10 s not 30 s). | `kiln/crates/kiln-core/src/platform/env.rs`, `kiln/crates/kiln-core/src/platform/mod.rs`, `kiln/crates/kiln-plugin/src/process.rs`, `kiln/crates/kiln-plugin/src/grants.rs`, `kiln/crates/kiln-plugin/wit/process.wit`, `kiln/crates/kiln-plugin/tests/environment.rs`, `crates/fe-workbench/src/plugin_host.rs`, `crates/fe-workbench/src/plugin_prompt.rs`, `crates/fe-workbench/src/plugin_prompt_tests.rs` | — | Linux-OK | yes | `kiln/crates/kiln-plugin/tests/environment.rs` (each OS's rules applied to a given environment, run on every OS) |
| P4 | m | kiln-plugin's Windows test twins (W-h): `tests/grants.rs`, `tests/process.rs`, `src/tests/prelaunch.rs`, `src/tests/program_budget.rs`, and the symlink tests in `tests/manifest.rs` and `tests/fs.rs`, with `.cmd` scripts and `System32` copies through one `script` helper. Only checks of Unix modes stay `cfg(unix)`. | `kiln/crates/kiln-plugin/tests/**`, `kiln/crates/kiln-plugin/src/tests/**` | P2, P3 | Linux-OK | yes | `kiln/crates/kiln-plugin/tests/support/script.rs` (the helper, declared from each test file) |
| P5 | s | fe-workbench's Windows twins: the five `cfg(unix)` tests of `plugin_host_tests.rs` and their helpers (`script`, `spawner`, `spawn`, `SPAWN`, `NAPPER`). | `crates/fe-workbench/src/plugin_host_tests.rs` | P4 | Linux-OK | yes | — (twins in place) |
| P6 | m | Guest paths (W-d): `kiln_plugin::paths` (host ↔ `/C:/…`), preopens under guest paths, every WIT path mapped both ways, `\` refused. | `kiln/crates/kiln-plugin/src/paths.rs`, `kiln/crates/kiln-plugin/src/lib.rs`, `kiln/crates/kiln-plugin/src/fs.rs`, `kiln/crates/kiln-plugin/src/events.rs`, `kiln/crates/kiln-plugin/tests/paths.rs`, `kiln/crates/kiln-plugin/tests/fs.rs`, `crates/fe-plugin/src/editor.rs`, `crates/fe-plugin/tests/editor.rs`, `crates/fe-workbench/src/plugin_events.rs` | P1 | Linux-OK | yes | `kiln/crates/kiln-plugin/tests/paths.rs` (drive paths, case, `\` refused, UNC and verbatim roots skipped, identity on Unix; all as pure functions on every OS) |
| P7 | m | The compile cache on Windows (W-g): a protected DACL from `create_private_dir`, and `owned_and_unshared` checking owner SID and DACL. `tests/cache.rs` runs on Windows, with a DACL-widened twin of the group-writable test. | `kiln/crates/kiln-core/src/platform/fs.rs`, `kiln/crates/kiln-core/src/platform/acl.rs`, `kiln/crates/kiln-core/src/platform/mod.rs`, `kiln/crates/kiln-core/Cargo.toml`, `kiln/crates/kiln-core/tests/private_dir.rs`, `kiln/crates/kiln-plugin/tests/cache.rs` | — | Linux-OK | yes | `kiln/crates/kiln-core/tests/private_dir.rs` (a new dir is owner-only; a widened one isn't trusted; on Unix, today's modes) |
| P8 | s | Dev links without privilege (W-f): a junction fallback on error 1314, and `linked_from`/`links_to`/uninstall handle junctions. A seam forces the fallback in tests. | `crates/fe-workbench/src/plugin_dev.rs`, `crates/fe-workbench/src/plugin_dev_tests.rs`, `crates/fe-workbench/src/plugin_manage.rs`, `crates/fe-workbench/src/plugin_manage_tests.rs` | — | Linux-OK | yes | — (cases added to `plugin_dev_tests.rs` and `plugin_manage_tests.rs`) |
| P9 | s | Secret size and replacing renames (W-i): a 4 KiB secret against the platform store, with a documented cap if Windows refuses it; `platform::replace_file` retrying a sharing violation for 1 s, used by the grant store and the cache. | `kiln/crates/kiln-core/src/platform/fs.rs`, `kiln/crates/kiln-core/src/platform/mod.rs`, `kiln/crates/kiln-core/src/secrets.rs`, `kiln/crates/kiln-core/tests/replace_file.rs`, `kiln/crates/kiln-plugin/src/grants.rs`, `kiln/crates/kiln-plugin/src/cache.rs`, `kiln/crates/kiln-plugin/src/secrets.rs`, `kiln/crates/kiln-plugin/wit/secrets.wit`, `kiln/crates/kiln-plugin/tests/secrets.rs` | — | Linux-OK | yes | `kiln/crates/kiln-core/tests/replace_file.rs` (a rename over a file held open; on Windows without delete sharing) |
| C1 | m | The 1.1 minor (§4.5): one generated world per minor, `compat::world_for`, `HOST_API` 1.1, `wit/released/1.0/` frozen with its check, the 1.0-on-1.1 test, and the pinned-1.0 refusal. No new interface yet. | `kiln/crates/kiln-plugin/src/compat.rs`, `kiln/crates/kiln-plugin/src/bindings.rs`, `kiln/crates/kiln-plugin/src/runner.rs`, `kiln/crates/kiln-plugin/wit/**`, `kiln/crates/kiln-plugin/tests/compat.rs`, `kiln/crates/kiln-plugin/tests/released_wit.rs`, `crates/fe-plugin/src/lib.rs`, `crates/fe-plugin/wit/**`, `crates/fe-workbench/src/plugin_host.rs`, `crates/tenmoku-plugin/template/**` | — | Linux-OK | no | `kiln/crates/kiln-plugin/tests/released_wit.rs` |
| K1 | s | The stage-3 capabilities in kiln-command's closed `Capability` enum: `jobs`, `language`, `panels`, `tasks` and `lsp:<server>` (named like `process.spawn:<program>`, no wildcard), with parsing, display, serde, `Grants::allows`, escalation in `grants::evaluate`, and the prompt's wording for each (`lsp:` naming the server and whether it is built in). | `kiln/crates/kiln-command/src/capability.rs`, `kiln/crates/kiln-command/tests/capability_stage3.rs`, `kiln/crates/kiln-plugin/src/grants.rs`, `kiln/crates/kiln-plugin/tests/grants.rs`, `crates/fe-workbench/src/plugin_prompt.rs`, `crates/fe-workbench/src/plugin_prompt_tests.rs` | P1 | Linux-OK | no | `kiln/crates/kiln-command/tests/capability_stage3.rs` (each parses and round-trips; `lsp:*` refused; adding one to an installed plugin is an escalation) |
| B1 | s | The tier model (§3.2): `Limits` split into interactive and background, `[contributes.plugin.background]` parsed with units, host ceilings from physical RAM, and background limits counted as escalations. | `kiln/crates/kiln-plugin/src/limits.rs`, `kiln/crates/kiln-plugin/src/manifest.rs`, `kiln/crates/kiln-plugin/src/grants.rs`, `kiln/crates/kiln-plugin/tests/tiers.rs` | — | Linux-OK | no | `kiln/crates/kiln-plugin/tests/tiers.rs` |
| B2 | s | The prompt line and `[extensions.plugin_limits."<id>"]` (schema, template, clamping diagnostics, user layer only), applied by the host. | `crates/fe-config/src/settings.rs`, `crates/fe-config/settings.schema.json`, `crates/fe-config/settings-template.toml`, `crates/fe-config/tests/plugin_limits_setting.rs`, `crates/fe-workbench/src/plugin_prompt.rs`, `crates/fe-workbench/src/plugin_prompt_tests.rs`, `crates/fe-workbench/src/plugin_host.rs` | B1 | Linux-OK | no | `crates/fe-config/tests/plugin_limits_setting.rs` |
| B3 | m | Jobs in kiln-plugin (§3.2, §3.4): `jobs`, `job-context`, `job-runner` and `job-handler`, worker stores without `init`, the job's `root` checked against open roots, wall time, the 5 s slice through the call hook, concurrency and the combined memory queue, the queue and spec limits, results held only until delivered and the per-plugin undelivered cap, cancellation (queued or running) with its 2 s grace, and everything dropped on unload. | `kiln/crates/kiln-plugin/Cargo.toml`, `kiln/crates/kiln-plugin/src/jobs.rs`, `kiln/crates/kiln-plugin/src/lib.rs`, `kiln/crates/kiln-plugin/src/bindings.rs`, `kiln/crates/kiln-plugin/src/runner.rs`, `kiln/crates/kiln-plugin/wit/jobs.wit`, `kiln/crates/kiln-plugin/wit/world.wit`, `kiln/crates/kiln-plugin/tests/jobs.rs`, `kiln/crates/kiln-plugin/tests/fixtures/jobs.wat` | B1, C1, K1 | Linux-OK | no | `kiln/crates/kiln-plugin/tests/jobs.rs` (a slice overrun kills the job, not the plugin; the wall time; the 9th job and an over-size spec refused; cancel before start and while running; a result reaching `job-finished` once, then `state` is `unknown`; undelivered results over 32 MiB; unload drops queued jobs and results) |
| B4 | m | Streamed programs (§3.3): `process.start` and `child`, background only, 1 MiB per-stream backpressure, 8 children per job, killed as a tree on cancel and at the wall time. | `kiln/crates/kiln-plugin/src/process_stream.rs`, `kiln/crates/kiln-plugin/src/process.rs`, `kiln/crates/kiln-plugin/src/lib.rs`, `kiln/crates/kiln-plugin/wit/process.wit`, `kiln/crates/kiln-plugin/tests/process_stream.rs`, `kiln/crates/kiln-plugin/tests/fixtures/stream.wat` | B3, P2, P4 | Linux-OK | yes | `kiln/crates/kiln-plugin/tests/process_stream.rs` (a fast writer blocks while the guest doesn't read; cancel kills grandchildren; `start` refused interactively; twins as P4) |
| B5 | s | Language servers from plugins (§3.6): a plugin extension's `[contributes.lsp]` under stage 1's consent, and `lsp.request` and `lsp.notify` to the session a buffer or a root names, for the extension's own or a built-in server. | `crates/fe-plugin/wit/lsp.wit`, `crates/fe-plugin/wit/world.wit`, `crates/fe-plugin/src/lsp.rs`, `crates/fe-plugin/src/lib.rs`, `crates/fe-plugin/tests/lsp.rs`, `crates/fe-workbench/src/plugin_host.rs` | C1, K1 | Linux-OK | no | `crates/fe-plugin/tests/lsp.rs` (a custom method reaches the fake server; two roots with the same server reach the right session; a routed `textDocument/*` method is denied; another extension's server is denied; a built-in server is reachable with its grant) |
| B6 | m | Jobs in the app (§3.5): fe-workbench runs worker stores through the cache, and plugin jobs appear in T1570's Runs with `RunOwner::Plugin`, `RunSummary::Text`, a timed-out reason and `RunStatus::Queued` (waiting reason, `queued_at`, cancel before start). Cancelled on unload, reload, disable and quit. | `crates/fe-workbench/src/plugin_jobs.rs`, `crates/fe-workbench/src/plugin_jobs_tests.rs`, `crates/fe-workbench/src/runs.rs`, `crates/fe-workbench/src/plugin_host.rs`, `crates/fe-plugin/src/lib.rs`, `crates/fe-plugin/wit/world.wit` | B3, B2, T1570 | Linux-OK | no | `crates/fe-workbench/src/plugin_jobs_tests.rs` (a queued job shows waiting and cancels from Runs without starting; queued to running sets `started`; progress text; timed out) |
| B7 | s | The plugin data store (§3.2): the `data` interface over `<state dir>/plugin-data/<id>/`, one locked byte and name counter per plugin shared by every store and charged before each write, the declared `background.data` quota with its ceiling, prompt line and escalation, the counter rebuilt from disk at load, Clear Data, and deletion on uninstall. | `kiln/crates/kiln-plugin/src/data.rs`, `kiln/crates/kiln-plugin/src/lib.rs`, `kiln/crates/kiln-plugin/src/bindings.rs`, `kiln/crates/kiln-plugin/wit/data.wit`, `kiln/crates/kiln-plugin/wit/world.wit`, `kiln/crates/kiln-plugin/src/limits.rs`, `kiln/crates/kiln-plugin/src/manifest.rs`, `kiln/crates/kiln-plugin/tests/data_store.rs`, `crates/fe-workbench/src/plugin_host.rs`, `crates/fe-workbench/src/plugin_manage.rs`, `crates/fe-workbench/src/plugin_manage_tests.rs` | B1, B3 | Linux-OK | yes | `kiln/crates/kiln-plugin/tests/data_store.rs` (a write past the quota is refused whole and leaves the file and the counter unchanged; a job writing in a loop stops at the quota while it runs, never past it; two worker stores and the interactive store writing at once never pass the quota together; `replace` charged before the rename; a crash-left file counted at load; over a lowered quota, reads and deletes work and growing writes fail; names outside the pattern refused; another plugin's files unreachable; Clear Data during a write; deleted on uninstall) |
| A1 | m | Language features I (§4.1): `language` and `language-handler` (completion, resolve, hover, signature help). Plugin providers join fe-lsp's merge as trailing add-ons through a non-LSP source trait, with owners by load generation, coalescing and limits, and `buffer.write` enforced on completion edits as answered and as resolved. | `crates/fe-plugin/wit/language.wit`, `crates/fe-plugin/wit/world.wit`, `crates/fe-plugin/src/language.rs`, `crates/fe-plugin/src/lib.rs`, `crates/fe-plugin/tests/language.rs`, `crates/fe-lsp/src/external.rs`, `crates/fe-lsp/src/lib.rs`, `crates/fe-workbench/src/plugin_language.rs`, `crates/fe-workbench/src/plugin_language_tests.rs` | C1, K1, T1645-A6, T1645-A7, T1645-W1 | Linux-OK | no | `crates/fe-plugin/tests/language.rs`, `crates/fe-workbench/src/plugin_language_tests.rs` (merged after a fake server's items; a final item deduped against an equal one; resolve to the plugin; a reload drops follow-ups; without `buffer.write`: an item whose edit is the host's completion span or an empty range at the caret is kept; one whose edit covers the whole line containing the caret, the word after the caret, or any other range, or that has `additional-edits`, is dropped; a resolved item that widens its edit or gains additional edits falls back to the item as answered) |
| A2 | m | Language features II: locations (the goto family, references), document and workspace symbols, code actions with resolve (`buffer.write` enforced), and inlay hints with `resolve-inlay-hint`, under their T1645 rows; workspace symbols asked per root. | `crates/fe-plugin/src/language.rs`, `crates/fe-plugin/tests/language_more.rs`, `crates/fe-lsp/src/external.rs`, `crates/fe-workbench/src/plugin_language.rs`, `crates/fe-workbench/src/plugin_language_tests.rs` | A1 | Linux-OK | no | `crates/fe-plugin/tests/language_more.rs` (an inlay hint resolved by its owner, never after a reload; two roots searched separately; code-action edits dropped without `buffer.write`) |
| A3 | s | The panels model (§4.2): `panels.wit`, `[[contributes.plugin.panels]]`, and a headless store validating limits, labels and depth. | `crates/fe-plugin/wit/panels.wit`, `crates/fe-plugin/wit/world.wit`, `crates/fe-plugin/src/panels.rs`, `crates/fe-plugin/src/lib.rs`, `crates/fe-plugin/tests/panels.rs`, `kiln/crates/kiln-plugin/src/manifest.rs` | C1, K1 | Linux-OK | no | `crates/fe-plugin/tests/panels.rs` |
| A4 | m | Rendering panels in docks with kiln-ui: tones and icons from the theme, virtualized lists and trees, events back, AccessKit roles, keyboard, and a screenshot golden. | `crates/fe-workbench/src/plugin_panel.rs`, `crates/fe-workbench/src/plugin_panel_tests.rs`, `crates/fe-workbench/src/dock/mod.rs`, `crates/fe-workbench/tests/screenshots/plugin_panel*` | A3 | Mac-only (screenshot golden) | no | `crates/fe-workbench/src/plugin_panel_tests.rs` |
| A8 | s | `navigation.open` (§4.2): only during a call that delivers a user action, one per action, the path mapped back, canonicalized and inside a trusted root the fs grant reaches. | `crates/fe-plugin/wit/navigation.wit`, `crates/fe-plugin/wit/world.wit`, `crates/fe-plugin/src/navigation.rs`, `crates/fe-plugin/src/lib.rs`, `crates/fe-plugin/tests/navigation.rs`, `crates/fe-workbench/src/plugin_bridge.rs`, `crates/fe-workbench/src/plugin_bridge_tests.rs` | C1, A1, P6 | Linux-OK | yes | `crates/fe-plugin/tests/navigation.rs` (allowed in a panel `activated` call and a user command; denied in an event handler and a job; a second open denied; a path outside the roots and a symlink out denied) |
| A5 | s | Settings contributions (§4.4): schema subset, scopes, secret keys, and `settings.get` with an optional root. | `kiln/crates/kiln-plugin/wit/settings.wit`, `kiln/crates/kiln-plugin/wit/world.wit`, `kiln/crates/kiln-plugin/src/settings.rs`, `kiln/crates/kiln-plugin/src/lib.rs`, `kiln/crates/kiln-plugin/src/manifest.rs`, `kiln/crates/kiln-plugin/tests/settings.rs`, `crates/fe-config/src/settings.rs`, `crates/fe-workbench/src/plugin_host.rs` | C1 | Linux-OK | no | `kiln/crates/kiln-plugin/tests/settings.rs` (defaults, then user, then trusted project; untrusted dropped; two open roots give each its own project keys; a root not open refused; a secret in a file warned and ignored) |
| A6 | s | Task providers (§4.3): `tasks` and `task-provider`, listed with the project's tasks, granted programs only, trusted roots only, run only from the picker or a user-dispatched Run Task (no plugin `run`, and the task commands stay off the plugin-callable allowlist). | `crates/fe-plugin/wit/tasks.wit`, `crates/fe-plugin/wit/world.wit`, `crates/fe-plugin/src/tasks.rs`, `crates/fe-plugin/src/lib.rs`, `crates/fe-plugin/tests/tasks.rs`, `crates/fe-workbench/src/task_defs.rs`, `crates/fe-workbench/src/plugin_tasks.rs`, `crates/fe-workbench/src/plugin_tasks_tests.rs` | C1, K1, P3 | Linux-OK | yes | `crates/fe-plugin/tests/tasks.rs`, `crates/fe-workbench/src/plugin_tasks_tests.rs` (listed per root; an ungranted program left out; `commands.invoke` of Run Task denied to the plugin; untrusted root offers none) |
| A7 | s | Plugins in the signed index (§4.6): `fe-registry` plugin entries, install verified against the entry, and "unsigned" for local installs. | `crates/fe-registry/src/plugins.rs`, `crates/fe-registry/src/lib.rs`, `crates/fe-registry/tests/plugin_index.rs`, `crates/fe-workbench/src/extension_install.rs`, `crates/fe-workbench/src/plugin_manage.rs`, `crates/fe-workbench/src/plugin_manage_tests.rs` | T1481, T1483, T1484, C1 | Linux-OK | no | `crates/fe-registry/tests/plugin_index.rs` |
| D1 | m | The `tenmoku.cargo` plugin (§5.3). | `extensions/plugins/cargo/**`, `crates/fe-workbench/tests/first_party_cargo.rs` | A5, A6, B4, B5, B6, B7, P6 | Linux-OK | yes | `crates/fe-workbench/tests/first_party_cargo.rs` (fixture crate: tasks listed; a check job streams progress and publishes diagnostics; cancel kills cargo; Test at Cursor asks a fake rust-analyzer for runnables and runs the test as a job) |
| D2 | m | The `tenmoku.todos` plugin (§5.3). | `extensions/plugins/todos/**`, `crates/fe-workbench/tests/first_party_todos.rs` | A4, A5, A8, B6, P6 | Linux-OK | yes | `crates/fe-workbench/tests/first_party_todos.rs` |
| D3 | s | The `tenmoku.paths` plugin (§5.3). | `extensions/plugins/paths/**`, `crates/fe-workbench/tests/first_party_paths.rs` | A2, A5, P6 | Linux-OK | yes | `crates/fe-workbench/tests/first_party_paths.rs` |
| S1 | s | Declare 1.1 stable once §4.5's conditions hold: the preview wording is dropped from the WIT, the guide and the setting's description, and the frozen copy of 1.1 is added. Filed only with the user's sign-off (condition 7). | `kiln/crates/kiln-plugin/wit/**`, `crates/fe-plugin/wit/**`, `docs/plugins/authoring.md`, `crates/fe-config/settings-template.toml` | all above | Linux-OK | yes | — |

**Order.**

- **Windows first:** P1, P2, P3, P7, P8 and P9 can run at once; P4 needs P2
  and P3; P5 needs P4; P6 needs P1.
- **Then the tiers:** C1, K1 and B1 at once, then B2, B3 and B5, then B4, B6
  and B7 (B6 after T1570 too).
- **The API:** A3, A5 and A6 after C1 and K1. A4 after A3. A1 and A2 after
  T1645's cards, then A8. A7 after the registry cards.
- **Last:** the dogfood plugins, then S1.

**Decision:** the plan above: 30 cards, all s or m, nine Windows cards first.

## 7. Out of scope and open questions

**Out of scope for stage 3:**

- **Webviews**, or any HTML or script surface. Panels are the view tree
  (§4.2).
- **Language servers in the sandbox** (§3.6, option b), and **memory64**.
- **Fuel metering** (§3.2: measured at +59%, and not needed).
- **Per-author signing keys**, publisher identities and key revocation (§4.6).
- **Auto-update outside the signed index.**
- **Debug adapters (DAP)**, a plugin API for the AI chat, and moving the chat
  out.
- **Code plugins in workspace extension directories.** Stage 2's rule stands:
  plugins install in the user directory only.
- **Moving any built-in out of core** (§5.2). The detected tasks need a later
  decision after D1.
- **Code actions that edit files that aren't open** (§4.1), and workspace-wide
  edits.
- **Persisting jobs across restarts.**
- **Moving fe-lsp's job object onto P2's `ProcessTree`.** That is a follow-up
  note, and it would also fix that job being assigned after the spawn.

**Open questions for the user:**

1. **Q1, Windows on ARM** (§2): is aarch64 Windows a target for plugins
   now, or x86_64 only until the app ships an ARM build?
2. **Q2, background ceilings** (§3): are 2 GiB per job (or a quarter of RAM)
   and 4 GiB for all jobs (or half of RAM) right, or should large machines
   allow more?
3. **Q3, downloaded language-server binaries** (§3.6): may a plugin fetch a
   server binary into its data directory and name it in its `lsp`
   contribution, with consent bound to the binary's SHA-256? Or do servers
   come only from `PATH` or the extension's own files?
4. **Q4, registry review and unsigned installs** (§4.6): who reviews plugin
   submissions, and do unsigned local installs stay allowed after the API is
   stable, or move behind a developer-mode switch?
5. **Q5, the first-party plugins** (§5.3): once the API is stable, are
   `tenmoku.cargo`, `tenmoku.todos` and `tenmoku.paths` installed and on by
   default, or optional installs?
6. **Q6, batch files** (§2, W-b): should `.bat` and `.cmd` programs be granted
   at all, flagged as `cmd.exe` interpreters as proposed, or refused, which
   would leave out every npm-installed CLI on Windows?
7. **Q7, network-share workspaces** (§2, W-d): should UNC roots
   (`\\server\share`) be preopened for plugins, under a reserved guest form
   such as `/UNC/server/share/…`, or stay unavailable to plugins on Windows?

**Decision:** the scope above. Q1 to Q7 go to the user with this document, and
the §6 cards are drafted only once the user has signed it off.

## Appendix A. The measurement harness (not committed)

`kiln/crates/kiln-plugin/tests/zz_measure.rs`, run with `wat = "1.262.0"` added
to kiln-plugin's `[dev-dependencies]`:

```rust
// TEMPORARY measurement harness for T1652's cost model. Not committed.
#![allow(clippy::all)]

use std::time::{Duration, Instant};

use kiln_command::Grants;
use kiln_plugin::{
    bindings::kiln::extension::{events::{BufferChanged, Event}, types::Range},
    cache::ComponentCache,
    engine::PluginEngine,
    limits::Limits,
    runner::{Call, Plugin, Reply},
};
use wasmtime::{Config, Engine, Instance, Module, Store, component::Component};

fn rss_kib() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().parse().unwrap()
}

fn component_text() -> String {
    let text = std::fs::read_to_string("tests/fixtures/empty.wat").unwrap();
    let realloc_start = text.find("    (func (export \"realloc\")").unwrap();
    let realloc_end = text.find("    (func (export \"init\")").unwrap();
    let realloc = r#"    (func (export "realloc") (param i32 i32) (param $align i32) (param $size i32) (result i32)
      (local $p i32) (local $end i32) (local $have i32)
      (local.set $p
        (i32.and
          (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
          (i32.sub (i32.const 0) (local.get $align))))
      (local.set $end (i32.add (local.get $p) (local.get $size)))
      (local.set $have (i32.mul (memory.size) (i32.const 65536)))
      (if (i32.gt_u (local.get $end) (local.get $have))
        (then (drop (memory.grow
          (i32.add (i32.shr_u (i32.sub (local.get $end) (local.get $have)) (i32.const 16)) (i32.const 1))))))
      (global.set $heap (local.get $end))
      (local.get $p))
"#;
    let mut text = format!("{}{}{}", &text[..realloc_start], realloc, &text[realloc_end..]);
    text = text.replace(
        "(func (export \"run-command\") (param i32 i32 i32 i32) (result i32) (i32.const 0))",
        "(func (export \"run-command\") (param i32 i32 i32 i32) (result i32) (global.set $heap (i32.const 1024)) (i32.const 0))",
    );
    text = text.replace(
        "(func (export \"handle-events\") (param i32 i32))",
        "(func (export \"handle-events\") (param i32 i32) (global.set $heap (i32.const 1024)))",
    );
    text
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn time_calls(plugin: &mut Plugin, make: &dyn Fn() -> Call, n: usize) -> (Duration, Duration) {
    for _ in 0..(n / 10).max(3) {
        plugin.call(make()).unwrap();
    }
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        let call = make();
        let t = Instant::now();
        let reply = plugin.call(call).unwrap();
        samples.push(t.elapsed());
        assert!(matches!(reply, Reply::RunCommand(Ok(())) | Reply::HandleEvents));
    }
    let min = *samples.iter().min().unwrap();
    (median(samples), min)
}

#[test]
fn zz_measure() {
    let engine = PluginEngine::shared().unwrap();
    let text = component_text();
    let bytes = wat::parse_str(&text).unwrap();
    let limits = Limits::default();
    let grants = Grants::default();

    // Per-call boundary cost through the runner (thread hop + epoch ticker + canonical ABI).
    let mut plugin = Plugin::load(engine, &bytes, "acme.measure", &grants, limits).unwrap();
    for size in [16usize, 4 << 10, 64 << 10, 1 << 20, 8 << 20] {
        let args = "x".repeat(size);
        let n = if size >= 1 << 20 { 50 } else { 2000 };
        let (med, min) = time_calls(
            &mut plugin,
            &|| Call::RunCommand { id: "acme.measure.x".into(), args: args.clone() },
            n,
        );
        println!("MEASURE run-command args {size} B: median {med:?} min {min:?} (n={n})");
    }
    for count in [1usize, 100, 10_000] {
        let events: Vec<Event> = (0..count)
            .map(|i| {
                Event::BufferChanged(BufferChanged {
                    buffer: 1,
                    version: i as u64,
                    ranges: vec![Range { start: 0, end: 1 }; 4],
                })
            })
            .collect();
        let n = if count >= 10_000 { 100 } else { 2000 };
        let (med, min) = time_calls(&mut plugin, &|| Call::HandleEvents(events.clone()), n);
        println!("MEASURE handle-events {count} events: median {med:?} min {min:?} (n={n})");
    }
    // The cost of cloning the payload on the caller side, to subtract.
    let args = "x".repeat(8 << 20);
    let t = Instant::now();
    for _ in 0..50 {
        std::hint::black_box(args.clone());
    }
    println!("MEASURE clone 8 MiB: {:?} each", t.elapsed() / 50);
    drop(plugin);

    // Per-instance memory: N plugins, each its own store and thread.
    let n = 100;
    let before = rss_kib();
    let mut plugins = Vec::new();
    let t = Instant::now();
    for i in 0..n {
        plugins.push(Plugin::load(engine, &bytes, &format!("acme.m{i}"), &grants, limits).unwrap());
    }
    let took = t.elapsed();
    std::thread::sleep(Duration::from_millis(200));
    let after = rss_kib();
    println!(
        "MEASURE {n} WAT plugins loaded (compile each, store, thread, init): {took:?} ({:?} each); RSS {before} -> {after} KiB, {} KiB each",
        took / n as u32,
        (after - before) / n as u64
    );
    drop(plugins);

    // Thread cost alone: N parked threads.
    let before = rss_kib();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
    let mut threads = Vec::new();
    let t = Instant::now();
    for _ in 0..n {
        let rx = rx.clone();
        threads.push(std::thread::spawn(move || {
            let _ = rx.lock().map(|r| r.recv_timeout(Duration::from_secs(5)));
        }));
    }
    let took = t.elapsed();
    std::thread::sleep(Duration::from_millis(200));
    let after = rss_kib();
    println!(
        "MEASURE {n} threads: spawn {:?} each; RSS {before} -> {after} KiB, {} KiB each",
        took / n as u32,
        (after.saturating_sub(before)) / n as u64
    );
    drop(tx);
    for t in threads {
        t.join().unwrap();
    }

    // Startup: the Rust guests, cold compile vs the component cache.
    for path in ["tests/guests/fs/fs.wasm", "../../../crates/fe-plugin/tests/guests/editor/editor.wasm"] {
        let wasm = std::fs::read(path).unwrap();
        let mut cold = Vec::new();
        for _ in 0..5 {
            let t = Instant::now();
            std::hint::black_box(Component::new(engine.engine(), &wasm).unwrap());
            cold.push(t.elapsed());
        }
        let dir = std::env::temp_dir().join(format!("zz-measure-cache-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let cache = ComponentCache::new(&dir);
        let t = Instant::now();
        cache.load(engine.engine(), &wasm).unwrap();
        let first = t.elapsed();
        let mut warm = Vec::new();
        for _ in 0..5 {
            let cache = ComponentCache::new(&dir);
            let t = Instant::now();
            std::hint::black_box(cache.load(engine.engine(), &wasm).unwrap());
            warm.push(t.elapsed());
            assert_eq!(cache.compiled(), 0);
        }
        let entry = std::fs::metadata(cache.entry_path(engine.engine(), &wasm)).unwrap().len();
        println!(
            "MEASURE {path} ({} B): cold compile median {:?}; first cached load (compile+write) {first:?}; cached load median {:?}; cache entry {entry} B",
            wasm.len(),
            median(cold),
            median(warm)
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // Compile rate on a synthetic ~1 MB module (Cranelift, the plugin engine's config).
    let mut wat = String::from("(module (memory 1)\n");
    for f in 0..4000 {
        wat.push_str(&format!("(func (export \"f{f}\") (param i32) (result i32) (local i32)\n"));
        for k in 0..20 {
            wat.push_str(&format!(
                "(local.set 1 (i32.add (i32.mul (local.get 0) (i32.const {})) (i32.load (i32.and (local.get 1) (i32.const 1020)))))\n(if (i32.gt_u (local.get 1) (i32.const {})) (then (local.set 0 (i32.xor (local.get 0) (local.get 1)))))\n",
                f * 31 + k,
                k * 7
            ));
        }
        wat.push_str("(local.get 1))\n");
    }
    wat.push(')');
    let module_bytes = wat::parse_str(&wat).unwrap();
    let t = Instant::now();
    let module = Module::new(engine.engine(), &module_bytes).unwrap();
    let took = t.elapsed();
    let serialized = module.serialize().unwrap();
    let t = Instant::now();
    // SAFETY: we just serialized it on this engine.
    std::hint::black_box(unsafe { Module::deserialize(engine.engine(), &serialized) }.unwrap());
    let de = t.elapsed();
    println!(
        "MEASURE synthetic module {} B: compile {took:?}, deserialize {de:?}, serialized {} B",
        module_bytes.len(),
        serialized.len()
    );

    // Fuel overhead: a tight loop on an epoch-only engine vs epoch + fuel.
    let looping = r#"(module (func (export "spin") (param $n i32) (result i32) (local $acc i32)
        (loop $l
          (local.set $acc (i32.add (local.get $acc) (i32.mul (local.get $n) (i32.const 3))))
          (local.set $n (i32.sub (local.get $n) (i32.const 1)))
          (br_if $l (local.get $n)))
        (local.get $acc)))"#;
    for fuel in [false, true] {
        let mut config = Config::new();
        config.epoch_interruption(true).consume_fuel(fuel);
        let engine = Engine::new(&config).unwrap();
        let module = Module::new(&engine, looping).unwrap();
        let mut store = Store::new(&engine, ());
        store.set_epoch_deadline(u64::MAX / 2);
        if fuel {
            store.set_fuel(u64::MAX / 2).unwrap();
        }
        let instance = Instance::new(&mut store, &module, &[]).unwrap();
        let spin = instance.get_typed_func::<i32, i32>(&mut store, "spin").unwrap();
        spin.call(&mut store, 1_000_000).unwrap();
        let mut samples = Vec::new();
        for _ in 0..5 {
            let t = Instant::now();
            std::hint::black_box(spin.call(&mut store, 500_000_000).unwrap());
            samples.push(t.elapsed());
        }
        println!("MEASURE spin 5e8 iterations fuel={fuel}: median {:?}", median(samples));
    }
}
```

Appended to `crates/fe-plugin/tests/editor.rs`, where it uses that file's
`Buffers` and `Plugin` helpers:

```rust
// TEMPORARY measurement for T1652; not committed.
#[test]
fn zz_measure_editor() {
    use std::time::{Duration, Instant};
    fn rss_kib() -> u64 {
        let out = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().parse().unwrap()
    }
    fn median(mut v: Vec<Duration>) -> Duration {
        v.sort();
        v[v.len() / 2]
    }
    // Compile once (the OnceLock), then instantiate + init N instances.
    let buffers = Buffers::with(1, "x");
    drop(Plugin::new(&["buffer.read"], &buffers));
    let n = 100;
    let before = rss_kib();
    let t = Instant::now();
    let mut plugins: Vec<Plugin> = (0..n).map(|_| Plugin::new(&["buffer.read"], &buffers)).collect();
    let took = t.elapsed();
    let after = rss_kib();
    println!(
        "MEASURE editor.wasm {n} instances (instantiate + init, compiled once): {:?} each; RSS {before} -> {after} KiB, {} KiB each",
        took / n as u32,
        (after - before) / n as u64
    );
    // Touch each: one small read, so each guest's allocator has run.
    for p in &mut plugins {
        p.read("1").unwrap();
    }
    let touched = rss_kib();
    println!("MEASURE editor.wasm after one small read each: RSS {touched} KiB, {} KiB each over before", (touched - before) / n as u64);
    let mem: Vec<u64> = plugins
        .iter_mut()
        .take(1)
        .map(|p| {
            let memory = p.store.data();
            let _ = memory;
            0
        })
        .collect();
    let _ = mem;
    drop(plugins);

    for size in [100usize, 64 << 10, 1 << 20, 8 << 20] {
        let text = "y".repeat(size);
        let buffers = Buffers::with(1, &text);
        let mut plugin = Plugin::new(&["buffer.read"], &buffers);
        let n = if size >= 1 << 20 { 30 } else { 1000 };
        for _ in 0..(n / 10).max(3) {
            plugin.read("1").unwrap();
        }
        let mut samples = Vec::new();
        for _ in 0..n {
            let t = Instant::now();
            std::hint::black_box(plugin.read("1").unwrap());
            samples.push(t.elapsed());
        }
        let min = *samples.iter().min().unwrap();
        println!(
            "MEASURE editor.wasm buffer.read + log echo of {size} B: median {:?} min {min:?} (n={n})",
            median(samples)
        );
    }
}
```
