---
task_uid: "01m47kbry2ttjd8npjvcv29g2r"
task: "T92"
rev: 2
type: "chore"
complexity: "l"
criteria: []
---

# Follow-ups from review of T71

Findings of the review of T71 that don't block the merge (below merge.block_severity):

- F7 medium crates/fe-config/src/watch.rs:62: Target mappings are refreshed before directory registration, rather than afterward. If another layer's symlink changes during watch(), FSEvents can lose its notification; the all-files batch reloads current settings but leaves the tracker following the old target, so subsequent edits are missed. Startup registration has the same ordering problem. The regression test changes the second link before tracker.batch(), so it does not exercise the registration gap.
  fix: After actual directory registration, re-resolve every configured file and register newly discovered directories, repeating reconciliation after further registrations before delivering the batch. Cover startup and dynamic registration with an injected registration hook that retargets another layer during registration, then verify notifications from its new target.
  instances: crates/fe-config/src/watch.rs:62, crates/fe-config/src/watch.rs:45, crates/fe-config/src/watch.rs:70, crates/fe-config/src/watch.rs:171, crates/fe-config/src/tests/watch.rs:213

- F8 low crates/fe-config/src/watch.rs:199: The refactor silently skips missing parent directories and removes the previous informational log. The spec requires a log explaining that these directories are not watched; without it, users receive no explanation for why creating their settings directory requires restarting hot reload.
  fix: Log skipped missing or inaccessible directories when establishing registrations, naming the directory and explaining that it is not watched.
  instances: crates/fe-config/src/watch.rs:199

## Implementation notes

- F7: `Tracker::register` now does the registering, through a closure, and resolves every
  file's links again after each round of real registrations, watching whatever new directories
  they lead to until none remain. Startup (`ConfigWatcher::with_hook`) and `Tracker::batch` both
  go through it, so the last resolution always follows the last registration.
- `ConfigWatcher::with_hook` (crate-private) calls a hook just after each directory is watched;
  `new` passes a no-op. `tracker_resolves_links_after_registering` retargets another layer's link
  from that hook at startup and during a later batch, without feeding the lost event, and checks
  edits to the new targets are reported. `watcher_follows_links_changed_while_registering` does the
  startup case with a real watcher. Both fail with the re-resolution removed.
- F8: a directory a chain needs that's missing or inaccessible is logged with `log::info!`, once
  per directory, saying it isn't watched and that creating it takes a restart.
