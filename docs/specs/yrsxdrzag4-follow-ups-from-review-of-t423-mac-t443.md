---
task_uid: "01m4pyvxfcc4vq1syrsxdrzag4"
task: "T30"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T423 (Mac T443)

Filed on argon from Mac T443 (card ~/coordination/argon-cards/T443.json rev 1, Mac lead message ~/coordination/from-mac-lead-2026-10-12-argon-g.md). Mac task uid 01m4pbaka6mt0tresy890e8je2. Its only dependency, Mac T423 (3c4738f0), is on argon mac/main d941263. HOLD: ready only after argon trunk has Mac T424 (f700934d), which changes ex.rs and ex/lines.rs. Title the host commit with (Mac T443). Neovim 0.12.4 for the oracle/corpus recorder is at ~/.local/opt/nvim-0.12.4.

Findings of the review of T423 that don't block the merge (below merge.block_severity):

- F3 medium crates/fe-vim/src/ex.rs:698: The single-row sort shortcut uses an unvalidated, partly normalized range. On "b\na", `:0sort z`, `:0sort nn` and `:0sort /a` incorrectly fail, although nvim 0.12.4 normalizes line zero to the first line and ignores these arguments. Conversely, `:3sort z`, `:3sort nn` and `:3sort /a` incorrectly succeed instead of returning E16 because the shortcut bypasses counted_lines. These differences also change whether subsequent macro keys execute.
  fix: Recognize sort, validate and normalize its effective range before parsing arguments, then short-circuit only a valid single-row range. Add oracle regressions for zero-address and past-end ranges, including malformed arguments.
  instances: crates/fe-vim/src/ex.rs:698
