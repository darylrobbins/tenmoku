# rust-analyzer memory defaults (T1904)

The built-in rust-analyzer now launches with `initializationOptions` of its
own: `files.exclude` lists `target` and every nested git worktree under the
root. Cache priming stays on and `lru.capacity` stays at rust-analyzer's
default (128): neither change met the thresholds the spec set. Measured on
fastedit, none of the three changes lowers rust-analyzer's memory.

## What ships

- **`files.exclude`** = `["target", <nested worktrees>]`, worked out by
  `crates/fe-lsp/src/rust_analyzer_defaults.rs` each time a routed instance
  starts (`Registry::launched_at`). A nested worktree is a folder whose
  `.git` is a file, as `git worktree add` makes them, among the root's
  children or their children (`.worktrees/<lane>`). Nothing deeper is read,
  and neither is anything under `target`, `.git` or a worktree. rust-analyzer
  1.98.1 takes `files.exclude` (`--print-config-schema`: "relative to the
  workspace root, and globs are not supported"); `files.excludeDirs` is the
  old name.
- **Cache priming stays on** and nothing is sent for it. Priming off cut the
  peak by 91%, but the first cross-crate go-to-definition took 26.3 s. The
  spec allows at most 2 s.
- **`lru.capacity` stays 128** and nothing is sent for it. At 64 the settled
  footprint was 5.31 GB, against 5.30 and 5.38 GB at 128: no change, where
  the spec asked for a drop of at least 15%.
- **Proc macros stay on** and nothing is sent for them.
- **The user's options merge on top.** `[lsp.servers.rust-analyzer]
  initialization_options` is deep-merged over the defaults: tables merge key
  by key at every depth, and any other value replaces the default at that
  key. A user `files.exclude` list replaces the default list, and a user
  `cachePriming = { enable = false }` is kept as written. Options that aren't
  a table are passed on unchanged. A rust-analyzer spec the host registers
  is the host's alone: it gets no defaults. jdtls's bundle merge
  (`with_bundles`) is unchanged; `launched_at` runs it first, as `launched`
  always did.

## Measurements

**Setup:** Apple M3 Max (14 cores, 36 GB), macOS 27.0, rust-analyzer 1.98.1
(48a229ce 2026-09-01). The root was the primary checkout
`/Users/daryl/code/fastedit` at 5f61e1cf5, the root a Tenmoku window on this
repository gets.

**Worktrees:** `.worktrees/` held 5 lane worktrees (`gate-stack-127`,
`mmjrn0hj4f`, `n5krsav71f`, `ztdenw59dh`, `zynd37pp73`) and 3 gate snapshots
under `.worktrees/.verify/`. With the primary, that made 9 entries in
`git worktree list`. The exclude list in the runs with excludes was `target`
plus those 5 lanes.

**Load:** other lanes were building throughout, with load averages of 20–30.
That made times to quiescent noisy, and resident memory fell whenever macOS
compressed rust-analyzer's pages. Footprint, which Activity Monitor's
"Memory" column shows, is the steady figure here.

**Harness:** a stdlib Python LSP client launched rust-analyzer in the root
with the run's options. It declared `experimental.serverStatusNotification`
and work-done progress, but not `workspace/configuration`, so the
initialization options are the only config. Every 5 s it sampled the
rust-analyzer process with `ps -o rss=` and `footprint -p <pid> -f bytes`.
It sampled from launch until the server was ready, and for 120 s after.
Ready means quiescent with no open progress except flycheck, as fe-lsp's
`ServerStatus` readiness counts it; with priming on, that is the end of the
"Indexing" (cache priming) progress. Up to that point no file was open.

Columns:

- **Settled:** the mean of the samples in the last 30 s of those 120 s.
- **Navigation:** after the 120 s, the client opened
  `crates/fe-lsp/src/registry.rs` and timed four requests: go-to-definition
  on `LspServerSettings` (into fe-config, which had no file open), then a
  hover on it, then the same two on `BufferId` in
  `crates/fe-lsp/src/manager.rs` (into fe-buffer). All four answered
  correctly in every run.
- **After navigation:** one more sample, 10 s after the last request.

The proc-macro server, sampled apart, peaked at 0.24–0.25 GB resident with
priming on and 0.03 GB with it off.

| Run | Options | Ready (s) | Peak footprint | Peak resident | Settled footprint | Settled resident | Go-to-def fe-config | Hover | Go-to-def fe-buffer | After navigation, footprint |
|---|---|---|---|---|---|---|---|---|---|---|
| A, before | none (today) | 85.6 | 8.22 GB | 6.14 GB | 5.30 GB | 3.86 GB | 44 ms | 58 ms | 8 ms | 5.30 GB |
| B | excludes | 71.0 | 8.41 GB | 7.00 GB | 5.30 GB | 0.80 GB ¹ | 21 ms | 91 ms | 24 ms | 5.31 GB |
| B2, B repeated | excludes | 73.7 | 8.60 GB | 7.65 GB | 5.38 GB | 1.06 GB ¹ | 25 ms | 40 ms | 17 ms | 5.39 GB |
| C | excludes, `cachePriming.enable = false` | 10.8 | 0.78 GB | 0.60 GB | 0.70 GB | 0.60 GB | **26 254 ms** | 2 957 ms | 2 ms | 3.78 GB |
| E | excludes, `lru.capacity = 64` | 55.4 | 7.53 GB | 8.53 GB | 5.31 GB | 8.23 GB | 13 ms | 34 ms | 14 ms | 5.35 GB |

¹ Resident fell while footprint held: macOS had compressed the process's
pages under the memory pressure of the concurrent builds.

Run D (priming off with `lru.capacity = 64`) was stopped unread once C ruled
priming out. The shipped defaults are B's options, so B and B2 are the
"after".

## Reading the numbers

- **The excludes change nothing on fastedit.** The root `Cargo.toml` is a
  virtual workspace whose members are `crates/*` and `kiln/crates/*`.
  rust-analyzer loads each package's own folder (minus its `target` and
  `.git`), its dependencies and the sysroot. Nothing under `.worktrees/` is
  in any of them, so nothing there was loaded in the first place. The
  excludes ship anyway, as the spec asks, at a cost of one directory listing
  per child folder at start. They do matter for a root that is itself a
  package with worktrees nested inside it, whose package folder would take
  those worktrees in.
- **Priming is most of the peak, but it buys the first navigation.** Without
  priming, the server was ready in 11 s at 0.78 GB. The first request then
  computed what priming would have, taking 26 s, and two navigations grew it
  to 3.78 GB. A 91% lower peak clears the 30% bar, but 26 s is far past the
  2 s bar, so priming stays on. The machine's load stretches that 26 s, but
  not by the 13× it would take to fit under 2 s.
- **lru 64 doesn't move settled memory** (5.31 GB, between B's 5.30 and
  B2's 5.38). The 128 parse trees it caps are a small part of 5.3 GB of
  analysis caches for the whole crate graph (run A's log numbers crates
  past 900). E's lower peak (7.53 GB) is
  within the A/B/B2 spread of 8.2–8.6 GB, and the spec's test is settled
  memory, so lru stays at its default.
- The lead's figures from the user's Tenmoku (8.3 GB resident, 5.1 GB
  footprint 1:40 after start, 241 MB for the proc-macro server) match run A.

## Known limits

- **A `workspace/configuration` answer replaces the defaults.** A user
  `[lsp.servers.rust-analyzer] settings = { rust-analyzer = { … } }` reaches
  rust-analyzer that way after any settings change (didChangeConfiguration).
  rust-analyzer takes that answer as its whole client config, so the
  defaults drop out until the next start. With no such section the answer
  is null, and rust-analyzer keeps the initialization options.
- **Only worktrees two levels down are found.** The 3 snapshots under
  `.worktrees/.verify/` are three levels down and aren't excluded. On
  fastedit they aren't loaded either, for the same reason as the lanes.
- **The list is taken at start.** A worktree made while the server runs
  counts from its next start.
- **A server that gave up stays stopped.** After too many crashes, fe-lsp
  compares the spec it gave up on (the launched one, defaults included)
  with the spec a new document would launch: `LspManager::target` now
  rebuilds that spec with `Registry::with_root_defaults`, just as it
  re-adds jdtls's bundles. If a worktree was added or removed since the
  give-up, the exclude list differs, and the next document gets a fresh
  crash budget, as any other new spec does.

## Not tried, for a follow-up

These levers aren't in this card's spec, and each would need the same
measurement:

- `cachePriming.numThreads`: fewer priming workers may lower the peak with
  priming kept on.
- `cargo.allTargets = false`: drops test, bench and example targets from
  analysis.
- Narrowing the workspace rust-analyzer loads through `linkedProjects`.
