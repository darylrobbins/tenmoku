---
task_uid: "01m417z3r49djatkxbqvt39khj"
task: "T6"
rev: 1
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "docs/decisions/mmap-read-only.md gives measured load time and memory at 100 MB, 1 GB and 4 GB for the rope vs a memmap2 prototype, the SIGBUS handling, the first-edit cost and a build/defer/reject decision with its threshold"
    required: true
    manual: false
---

# Record a decision on a memory-mapped read-only mode for very large files

## Problem

The source card (T-18) asks for a decision on a read-only, memory-mapped view
for very large files: "browsing matters more than editing for this product. It
would turn into a rope on first edit."

Today every file becomes a rope: `Buffer::from_reader`, then
`fe_text::Text::from_reader`. PERF.md has a 100 MB file loading in 43 ms at
1.46× the file size in memory. A multi-GB log would need several GB of RAM and
seconds of load before it could be browsed. `memmap2` (MIT/Apache-2.0, already
in Cargo.lock 0.9 through GPUI) would make browsing cost nearly nothing up
front.

## Change

Write `docs/decisions/mmap-read-only.md` recording the decision. It covers:

- **Costs:** load time and resident memory for 100 MB, 1 GB and 4 GB files with
  today's rope, from the `fe-buffer` `load_memory` example run at those sizes
  by hand, against a throwaway `memmap2` prototype that maps the file and
  builds a line index.
- **Surface:** what a mapped view would have to provide to `fe-display`,
  `fe-syntax` and `fe-editor`, which currently take `fe_text::Text` or
  `BufferSnapshot`.
- **Safety:** what happens when another process truncates or rewrites a
  mapped file (SIGBUS), and how to guard against it (copy-on-open threshold,
  a lock, or detecting the change through file metadata).
- **First edit:** the cost of turning the view into a rope.
- **Decision:** build, defer or reject, with the threshold file size if
  build, and what would change the decision.

The prototype is not committed. If the decision is "build", draft the follow-up
card in the doc.

## Open questions

- Is editing files above the threshold required at all, or is read-only with
  "convert to editable" enough? The product direction in the source card
  favours browsing.
