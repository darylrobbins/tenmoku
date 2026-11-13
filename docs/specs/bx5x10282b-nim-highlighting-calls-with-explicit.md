---
task_uid: "01m7ats4qtnjn7rhbx5x10282b"
task: "T371"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "extensions/grammars/nim/test/sample.nim holds generic calls (a generic proc call such as initTable[string, int](), a generic method call and a generic command call) and its golden colours each callee as a function, with field accesses still properties; wasm_parity every_extension_highlights_its_sample passes for nim"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Nim highlighting: calls with explicit generic arguments are functions (follow-up from review of T366, Mac T1629)

# Nim highlighting: calls with explicit generic arguments are functions (follow-up from review of T366, Mac T1629)

T366's review (codex, round 2, finding F4, medium): the call patterns in extensions/grammars/nim/highlights.scm exclude explicit generic arguments. The pinned grammar (tenmoku-editor/tree-sitter-nim d7408cc) represents `initTable[string, int]()` as an index_suffix between the callee and the call suffix, so the callee is highlighted as a variable (golden test/sample.nim.golden line ~111). The same restriction is in the direct-call, method-call, command-call and block-call patterns.

## Change
Handle explicit type-argument suffixes when identifying the callee in each of those patterns, keeping the field-access and receiver corrections T366 made (item.name and s.w are properties, the receiver of name.toUpperAscii() is not a function). Add representative generic-call coverage to test/sample.nim (a generic proc call, a generic method call, a generic command call), regenerate the golden and inspect it.

Mind the tree-sitter anchor trap in memory: write the call-capture variants out, dump the tree first. The wasm itself does not change; only highlights.scm, the sample and its golden do.
