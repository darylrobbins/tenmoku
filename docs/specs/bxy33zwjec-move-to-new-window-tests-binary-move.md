---
task_uid: "01m7ppx5edj18nzrbxy33zwjec"
task: "T1949"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The binary-file move test goes through a real destination-opening callback instead of the fake opened() loader, and asserts the destination window keeps the hex viewer and the source tab closes"
    required: true
    manual: false
  - id: "C2"
    text: "The destination-preservation test lives in a *_tests.rs file declared from crates/fastedit/src/main.rs, not inline in main.rs mod tests"
    required: true
    manual: false
  - id: "C3"
    text: "The decision file records that each of the two tests fails with T1930's fix reverted"
    required: true
    manual: false
---

# Move to New Window tests: binary move test through a real destination-opening callback, and the main.rs destination-preservation test in a base-declared *_tests.rs (T1930 F3, F4)

Mac twin of neon T385: follow-up of T1930 (neon T382) review findings F3/F4 (medium, after round 2).

- F3: the binary-file Move to New Window test uses `opened()`, whose fake loader succeeds unconditionally, so it never exercises the real destination-opening path. Drive it through a real destination-opening callback and assert that the destination window keeps the moved hex viewer and that the source tab closes.
- F4: the main.rs destination-preservation test is inline in main.rs `mod tests`, which the regression check cannot see at base. Move it into a `*_tests.rs` file declared from main.rs (base-declared module) so it can be base-verified.

Test-only: no production change expected. Verify by hand that each moved/rewritten test fails with T1930's fix reverted, and record that in the decision file.
