---
task_uid: "01m7bky37p34b3hxe6n2jrava7"
task: "T358"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "WIT `jobs`, `job-context`, `job-runner` and `job-handler` exist; a job runs in a worker store without `init`; the job's `root` is checked against open roots and refused otherwise."
    required: true
    manual: false
  - id: "C2"
    text: "Limits: wall time per job, the 5 s slice enforced through the call hook (a slice overrun kills the job, not the plugin), concurrency limit and a combined memory queue, queue length and spec size limits; every queue and result buffer is host-capped."
    required: true
    manual: false
  - id: "C3"
    text: "New tests in kiln/crates/kiln-plugin/tests/jobs.rs: `jobs_slice_overrun_kills_the_job_not_the_plugin`, `jobs_wall_time_ends_the_job`, `jobs_ninth_job_is_refused`, `jobs_oversize_spec_is_refused`, `jobs_cancel_before_start`, `jobs_cancel_while_running_with_2s_grace`, `jobs_result_reaches_job_finished_once_then_state_unknown`, `jobs_undelivered_results_over_32mib_refused`, `jobs_root_not_open_is_refused`, `jobs_unload_drops_queued_jobs_and_results`."
    required: true
    manual: false
  - id: "C4"
    text: "A result is held only until delivered; the per-plugin undelivered cap (32 MiB) is checked before the result is stored (quota before growth), and a cap hit fails the job with a reason."
    required: true
    manual: false
  - id: "C5"
    text: "The `jobs` capability from K1 is required; a plugin without it cannot link the jobs interface."
    required: true
    manual: false
---

# Jobs in kiln-plugin: background worker stores, queue, cancel, results (B3, 3.2, 3.4) (Mac T1700)

# Jobs in kiln-plugin: background worker stores, queue, cancel, results (B3, 3.2, 3.4)

Draft id B3 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Sections 3.2 and 3.4. Fixture is a .wat so the test needs no wasm toolchain. Codex rules: quota before growth, root context (job root must be an open root), caps on host queues.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- **Worlds.** `wit/jobs.wit` holds `jobs`, `job-context`, `job-runner` and `job-handler`. They are
  not added to the `extension` world: fe-plugin's `editor-extension` includes it, and every
  existing 1.1 load would then have to export `run-job` and `job-finished`. Instead `world.wit`
  adds `world jobs-extension { include extension; import jobs; import job-context; export
  job-runner; export job-handler; }`, generated in `bindings::jobs_world` over the current
  interfaces' types. A plugin with the `jobs` capability links the two imports
  (`bindings::add_jobs_to_linker`) and is checked against `jobs-extension`; one without it links
  neither, so a component importing them fails to load (C5,
  `jobs_without_the_capability_cannot_link`).
- **Delivery.** `runner::Call` is matched exhaustively in fe-workbench, so delivery isn't a new
  `Call`: `Plugin::deliver_jobs` sends a runner-internal request to the plugin thread, which
  calls `job-finished` under the interactive deadline. A result leaves the host (`Jobs::take_ended`)
  as it is handed over. `JobHost::on_ended` tells the app when to deliver (for B6).
- **Slice.** wasmtime calls the call hook around its own libcalls too (the epoch check,
  `memory.grow`, bulk memory), so only a real import renews the slice: the bindings' getter and
  `WasiView::ctx` mark the store as having entered an import, and `ReturningFromHost` renews only
  then (`jobs_memory_grow_does_not_renew_the_slice`).
- **Waiting imports.** On each entry to the host a worker sets `HostState::deadline` and arms the
  program budget to when the job must end: its wall time, or its grace once it is being stopped.
  So `commands.invoke`, `net.fetch` and `process.spawn` wait no longer than that, and a job's
  programs get its remaining wall time rather than 10 s. A result returned after the wall time
  is `timed-out`. Each of the three also watches the job's `JobControl` while it waits (§3.3):
  once the job is cancelled, timed out or unloaded it fails with `failed("cancelled")`, every
  10 ms at worst. `invoke` stops waiting for the answer (and the app sees the request
  cancelled), `spawn` kills its program tree, and a worker's `fetch` sends from a thread of its
  own, so it is left to finish there with the backend. Tests cancel and unload a job while
  each import already waits (`jobs_cancel_ends_imports_already_waiting`,
  `jobs_unload_ends_imports_already_waiting`). `spawn` reads the program to check it on a
  thread of its own in a job, so the job's stop and the share's end are seen while it reads; at
  most 8 jobs' checks read at once (`process::MAX_JOB_CHECKS`), each counted until its thread
  ends, and with no room, or no thread, the spawn fails. The wait for a suspension to be
  written watches the job too, and a job stopped during setup starts nothing. Left open: the
  reading itself isn't cancelled (`grants::check_program_by` watches only its deadline), so a
  stopped job's check reads on until done or the deadline, under the cap.
- **Lead decision (2026-11-18), review findings F9/F12: option B.** That bounded remainder is
  accepted for this card, and the scope is not widened (cancelling the check itself needs
  `grants.rs` and the test seams). The lead files a follow-up card for the import-level
  hardening.
- **Invocations the app holds** are capped before dispatch, whether the plugin still waits for
  them or not: 64 per plugin load and 256 in all (a load's worker stores invoke on its own
  attachment, `commands::State::for_jobs`, whatever the app's worker factory makes, but a
  worker waiting in `invoke` doesn't mark the interactive store as busy), holding at most 16 MiB and 64 MiB as
  `commands::invocation_bytes` counts them (each JSON node and its strings' and containers'
  room). The charge lasts until the app drops the request.
- **Results** are lifted as `runner::KeptText`, a `string` lift of its own (over wasmtime's
  `__internal` lift API, pinned with the workspace's wasmtime): reading the pointer and length,
  it checks the length against 16 MiB and reserves it against the 32 MiB of undelivered results
  (`jobs::Reservation`, reached through the worker thread's `RESERVATION`) before copying, all
  during the lift, so before the guest's post-return may free or reuse the bytes
  (`jobs_results_are_kept_before_post_return_reuses_their_memory`). It reads UTF-8 in place,
  so a plugin with `jobs` must be a binary component whose lifts all use UTF-8
  (`jobs::lifts_utf8`, read with wasmparser at load); a UTF-16, compact UTF-16 or text one is
  refused at load.
- **Failures.** A worker's failure is written through a bounded writer, at most 4 KiB
  (`jobs::MAX_DIAGNOSTIC_BYTES`), as a trap's backtrace names functions the guest chose. Its
  4 KiB are reserved against the undelivered results before it is written; with no room it ends
  as `result dropped` unwritten. And
  `finish` holds every end, whatever made it, to 16 MiB and then to the 32 MiB.
- **Fetches in a job** send from a thread of their own (the backend's `send` blocks), the
  request moved there and back, not copied. The worker waits until the fetch's deadline (so
  within the job's wall time) or the job's stop, whichever comes first. A send left running then
  finishes on its thread, still counted: at most 16 jobs' sends are in flight, holding at most
  32 MiB of requests counted with their containers' spare room (`net::MAX_JOB_SENDS`,
  `net::MAX_JOB_SEND_BYTES`), whatever the job slots, and past either a job's fetch fails at
  once; a job's request carries at most 256 headers. A send the worker stops waiting for (the
  job stopping or unloading, or the fetch's deadline) is called off:
  `NetBackend::send_cancellable` takes a `SendCancel`, and fe-plugin's `FeNet` drops the request
  and its connection within 10 ms of it, so the thread and the request go too
  (`jobs_cancel_calls_off_a_send_the_backend_lets_go_of`). Only a backend that says it does
  (`NetBackend::cancellable`, false by default) fetches for a job; through another a job's fetch
  fails. A send called off hands back only the backend; one that answered is counted until its
  answer is read or dropped. A send that timed out keeps the backend until it returns, and the
  job's next fetch waits for it within its own deadline.
- **Delivery** hands over only the jobs ended as `deliver_jobs` starts, so a `job-finished` that
  starts and waits for another job can't keep it delivering.
- **Queue.** A queued job's root is checked again as it would start; closed, it ends failed
  without starting. A cancelled job's name, args and root are dropped at once, and a started
  one's are moved to its launch.
- **Priority.** Job threads call `kiln_core::platform::lower_thread_priority` first: QoS utility
  on macOS, `nice` 10 (or the thread's own, if higher) on Linux, below normal on Windows. It lives
  in kiln-core's platform module because kiln branches on the OS only there
  (`scripts/check_kiln_boundary.py`); its tests read the priority back on each OS.
- **Wall time** stops a job as cancelling does (`cancelled()` true, then 2 s grace), ending it
  `timed-out`.
- **Queue.** One `JobHost` per app (`JobHost::shared()` by default; tests make their own). Queued
  jobs start in global order; one held back by the 4-in-all or the all-jobs memory limit holds back
  every later job. Ended undelivered jobs are capped at 64 per plugin as well as 32 MiB, so
  dropped-result entries can't pile up.
- **Messages** to plugins (`too many jobs`, `result dropped: too many undelivered`, …) are the
  API's own strings, constants in `jobs.rs` under `verbatim!`, untranslated, so plugins can match
  them (the catalogue is outside this card's scope anyway).
- **Not done here:** `denied("not in a background job")` from the registering imports in a
  worker (`commands.rs`; a worker never runs `init`, so `register` is refused anyway).
  `jobs.start`/`cancel` in a worker are `denied("not in a background job")`.
