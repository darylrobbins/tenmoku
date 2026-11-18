# A lead not found launches once per open (T691)

`LspManager::open`'s closing pass over documents waiting for the servers it
placed used `place_waiting`, which prunes and then starts a server. A lead whose
launch failed at once had already ended, so it was pruned and relaunched within
the same open, and its status read `Starting` after its `Stopped`. The pass now
only moves waiting documents onto an instance still running there
(`waiting_for` + `running_place` + `set_place`) and starts none.

## Evidence

- `missing_lead_names_binary_and_install` alone: 20 of 20 consecutive passes.
- Whole `cargo nextest run -p fe-lsp`: 5 of 5 passes.
- `one_launch::a_lead_not_found_launches_once_per_open` with manager.rs at
  base: failed 3 of 3; with the fix: passes. Its second trust ask always waits
  for ty's stop plus 100 ms, so ty's thread has ended before the pass.
