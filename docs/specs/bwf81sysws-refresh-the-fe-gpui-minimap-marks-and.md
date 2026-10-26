---
task_uid: "01m5v2remnwhppc6bwf81sysws"
task: "T1175"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "On main 2e865112, cargo run -p fe-gpui --example screenshots fails only minimap_marks and minimap_left (about 2558 px, 0.167% over the 0.100% limit, found gating stack 58); after this task it passes every scene."
    required: true
    manual: false
  - id: "C2"
    text: "The goldens are rewritten with UPDATE_SCREENSHOTS=1 for those two scenes only, and the worker compares old and new PNGs and states in the hand-in that the only differences are T1154's render_whitespace dots and/or T1124's code-span colour; any other difference is reported as a bug instead of committed."
    required: true
    manual: false
---

# Refresh the fe-gpui minimap_marks and minimap_left screenshot goldens for T1154's whitespace dots and T1124's code-span colour

Main's fe-gpui screenshot comparison is red: `minimap_marks` and `minimap_left` differ from their goldens by 2558 px (0.167%, limit 0.100%). Stack 58's gate found it and reproduced it on a detached 2e865112, so it comes from main, most likely T1154 (editor.render_whitespace dots) and T1124 (code-span colour) landing without a golden update; the Regatta gate does not run the screenshot examples.

Do: on macOS, `cargo run -p fe-gpui --example screenshots` to confirm only those two scenes fail; `UPDATE_SCREENSHOTS=1 cargo run -p fe-gpui --example screenshots`; keep only the two PNG changes; compare old and new visually; rerun the comparison clean. No code changes. See TESTING.md "Screenshots".
