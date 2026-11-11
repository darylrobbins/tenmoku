---
task_uid: "01m71xp7mpe495vp8z3r0jp840"
task: "T319"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "rtl_hit_testing_a_mixed_row_lands_on_char_boundaries sweeps from left of the row to past its painted right edge (origin plus the shaped line width) and passes on macOS"
    required: true
    manual: false
---

# Sweep rtl_hit_testing_a_mixed_row_lands_on_char_boundaries to the row's painted right edge, not the logical end caret, which is mid-row since T305 (T305 review F4)

Follow-up of T305 (review finding F4, low). In crates/fe-gpui/tests/non_latin_text.rs, rtl_hit_testing_a_mixed_row_lands_on_char_boundaries sweeps x from caret_x(0) - 10 to caret_x(RUST_RTL.len()) + 20. Since T305, the caret at the end of a row ending in right-to-left text is on that run's left edge (the logical predecessor rule), so the sweep stops mid-row and no longer covers the RTL run or the row's end. Take the right bound from caret_x(0) + line_width(RUST_RTL, cx) (the helper T305 added) instead. macOS-only test; verify on macOS.
