---
task_uid: "01m7j3axeaw1e5mfztdenw59dh"
task: "T1904"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-lsp test rust_analyzer_defaults::defaults_exclude_nested_worktrees_and_target: for a temp root with target/, .worktrees/a (with a .git file) and src/, the rust-analyzer spec's initializationOptions files.exclude lists target and .worktrees/a and not src; a root with no worktrees lists only target"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test rust_analyzer_defaults::user_options_merge_over_defaults: user initialization_options { checkOnSave = false, cachePriming = { enable = true } } keep the default excludes, set checkOnSave false and override cachePriming; a user files.exclude list replaces the default list; jdtls with_bundles tests unchanged and passing"
    required: true
    manual: false
  - id: "C3"
    text: "The decision file records rust-analyzer's peak and settled resident memory and footprint, time to quiescent and the first cross-crate go-to-definition time on this repo, before and after, for cache priming on/off and lru 128/64; the shipped defaults follow the spec's thresholds; PERF.md gains the before/after row"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp and clippy --workspace pass; the real_server rust-analyzer tests still pass"
    required: true
    manual: false
---

# Memory-conscious rust-analyzer defaults: exclude nested git worktrees and target/, cache priming and lru capacity set from measurements on this repo, deep-merged under the user's initialization_options

The user (2026-11-16), looking at Activity Monitor: "Anything we can do about the language server memory usage? Fastedit is a big Rust project but this is pretty expensive". Measured by the lead: rust-analyzer under the user's Tenmoku on this repo was at 8.3 GB resident / 5.1 GB footprint 1:40 after start (cache priming), plus 241 MB for rust-analyzer-proc-macro-srv. The user then said: "Go ahead and card the recommended rust analyzer optimizations."

## What to change

Ship memory-conscious rust-analyzer defaults in crates/fe-lsp/src/registry.rs's rust-analyzer ServerSpec (today it sends no initialization options), merged under the user's settings, never replacing them:

1. **Exclude what rust-analyzer never needs.** Nested git worktrees under the root (a directory whose `.git` is a file, as `git worktree add` makes them; this repo keeps a full copy of the workspace per lane in `.worktrees/`) and `target/`, through rust-analyzer's `files.exclude` (check the setting name the rust-analyzer shipped with toolchain 1.98.1 accepts; older ones used `files.excludeDirs`). Computed per root at server start: list the root's immediate children and `.worktrees/*`-style children with a `.git` file. Don't walk the whole tree.
2. **Cache priming.** Measure the fastedit workspace with `cachePriming.enable` true (today) and false: peak and settled resident memory and footprint of the rust-analyzer process, time to quiescent, and the first go-to-definition into a crate with no open file. Ship `false` as the default if it cuts the peak by at least 30% and that first go-to-definition stays under 2 s on this Mac; otherwise keep priming and say why in the decision file.
3. **`lru.capacity`:** measure the default (128) against 64. Change it only if settled memory drops by at least 15% with no visible slowdown in the same go-to-definition and hover checks.
4. **Proc macros stay on** (gpui's macros need them).

## Merging with the user's settings

`spec_with_settings` (registry.rs ~L497) replaces `initialization_options` wholesale today, so a user who sets `[lsp.servers.rust-analyzer] initialization_options = { checkOnSave = false }` would lose these defaults. Deep-merge instead: defaults first, then the user's table on top (a user key wins, including an explicit `cachePriming = { enable = true }`; a user `files.exclude` list replaces the default list). Keep jdtls's `with_bundles` behaviour unchanged.

## Record

A decision file under docs/decisions/ with the measurements (method: `footprint -p` and `ps -o rss` sampled every 5 s from launch to quiescent and for 2 minutes after, on this repo with a clean `.worktrees/` count noted), and a PERF.md row for rust-analyzer memory on fastedit before and after.
