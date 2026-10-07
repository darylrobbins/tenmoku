---
task_uid: "01m49ewb4nbncgm2wsnfwv82b3"
task: "T136"
rev: 2
type: "chore"
complexity: "l"
criteria: []
---

# Follow-ups from review of T105

Findings of the review of T105 that don't block the merge (below merge.block_severity):

- F8 medium crates/fe-nav/src/search.rs:895: The scanner treats every depth-zero `from` as the module-clause keyword, including a binding named `from`. Valid statements such as `import from from './util'`, `import * as from from './util'`, and `export * as from from './util'` now produce no module path. These previously recognized imports lose Imported ranking and can change automatic-jump decisions.
  fix: Track binding positions so a contextual identifier named `from` is distinguished from the subsequent module-clause keyword; add regressions covering default bindings, namespace imports, and namespace re-exports.
  instances: crates/fe-nav/src/search.rs:895

- F9 medium crates/fe-nav/src/search.rs:864: After finding one module, advancing to the following line discards the remainder of the module's line. Consequently, `import a from './a'; import { helper } from './util';` recognizes only './a'; additional re-exports and require calls on that line are also skipped. This regresses Imported ranking for valid source that the previous whole-line heuristic recognized.
  fix: Preserve the token or byte position after each module clause and continue scanning the remaining line for additional statements and require calls; test multiple imports, multiple re-exports, and mixed import/require statements on one line.
  instances: crates/fe-nav/src/search.rs:864

- F10 medium crates/fe-nav/src/search.rs:584: Cancellation during import scanning/ranking or workspace fuzzy matching is not checked before returning success. These existing paths can return an uncancelled answer after the caller has cancelled the stale request, and large workloads continue without cancellation checks.
  fix: Check cancellation during expensive processing where practical and immediately before both successful returns; add regressions cancelling during these phases.
  instances: crates/fe-nav/src/search.rs:584, crates/fe-nav/src/search.rs:602


Scope notes (triage, main 1e99299): F8, F9 and F10 all still true. `module_of` treats any depth-0 `from` word as the keyword; `js_imports` does `ix += at + 1` after a module; `definitions` and `workspace_symbols` don't check `cancel` after their scan/rank/fuzzy phases. Waits for T106, which is rewriting search.rs: re-check the line numbers against T106's result. Fix in `crates/fe-nav/src/search.rs` only. Put the regressions in a new `crates/fe-nav/src/search_followup_tests.rs`, registered from search.rs with `#[cfg(test)] #[path = "search_followup_tests.rs"] mod followup_tests;`, so the scanner and cancellation internals can be tested directly.
