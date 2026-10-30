---
task_uid: "01m668xmvk949rr8sejgj0v44d"
task: "T1246"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "In the it and at objects, an opening tag whose name holds a char with more than 6 distinct composing chars (counted after dedup, a ZWJ counting as one) pairs with no tag, as nvim 0.12.6 does; only the opening tag's name counts (an opening name with 6 and a closing name with 7 still pair), and a name whose chars each have 6 or fewer pairs as before. e.g. \"\u003ca\" + 7 distinct composing marks + \"\u003ex\u003c/a\" + the same marks + \"\u003e\" selects nothing for yit, while 7 identical U+0301 marks or 6 distinct marks select \"x\". These rules come from probing nvim 0.12.6 (lane notes), not assumption."
    required: true
    manual: false
  - id: "C2"
    text: "Oracle cases are added to delimiters.toml and recorded with nvim 0.12.6 (the only nvim installed; temporarily point support::NVIM at it, never download nvim), keeping the 0.12.4 stamp in delimiters.expected.json as docs/decisions/q3sndmknyh-line-end-motions-oracle-stamp.md (T802) describes: at least 6 marks (pairs), 7 marks (pairs nothing), a mismatching closing name, and both it and at; any other case in the file keeps its recorded result."
    required: true
    manual: false
  - id: "C3"
    text: "The existing clock tests (src/tests/delimiters_clock.rs) still pass, with only the fixtures of the two that used 600-mark graphemes as load (expiring_at_any_look_reading_names..., ...preparing_a_pattern...) changed to 5 marks per char, since such names now correctly pair nothing, and the new check adds no unbounded work on a row (it charges the command clock as the neighbouring name scan does)."
    required: true
    manual: false
  - id: "C4"
    text: "Nothing outside the scoped files changes, except this lane's own docs/specs file."
    required: true
    manual: false
---

# fe-vim delimiters: a tag name with a char of more than 6 composing chars pairs no tag, as nvim 0.12.6 does for it/at
