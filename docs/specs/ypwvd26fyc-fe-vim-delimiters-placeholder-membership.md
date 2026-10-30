---
task_uid: "01m62pxz9fzp3ndpypwvd26fyc"
task: "T1213"
rev: 4
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "Uppercase class atoms \\K \\I \\F \\P in a tag-name pattern behave as in Vim (they exclude digits): e.g. '\u003c1ZWJ\\K\u003ex\u003c/1ZWJ1ZWJ\u003e' pairs nothing, as nvim 0.12.4 does; recorded as oracle cases."
    required: true
    manual: false
  - id: "C2"
    text: "Escaped ranges, negated collections and class atoms (\\k \\i \\f \\p) in a tag-name pattern keep each grapheme placeholder's original membership (a placeholder is in [a-z], [^a-z] or \\k exactly when its cluster's base character is; review F1, F3 on T564); a trailing '-' in a collection is unchanged (F9); positive and negated range oracle cases recorded."
    required: true
    manual: false
  - id: "C3"
    text: "Work inside a single row is accounted: grapheme segmentation, name comparisons and pattern compilation charge the command clock at least every 4096 units, and deterministic tests in src/tests/delimiters_clock.rs expire the clock at every individual checkpoint (each row, name-processing, comparison and compilation check, review F6 on T564) and see the text object fail cleanly."
    required: true
    manual: false
  - id: "C4"
    text: "Nothing outside the scoped files changes, except this lane's own docs/specs file."
    required: true
    manual: false
---

# fe-vim delimiters: placeholder membership in collections and uppercase class atoms, and fine-grained clock accounting (T564 F1 F2 F3 F6 F9)

## Implementation notes

- C1/C2 (F1, F3): a stand-in (the private-use char a joined grapheme of the text is put as) is chosen to be among a collection's written members only if its base char is too (`choose`, a window of free pool chars at a time, each collection's members searched once per window); then a base's stand-ins are put among the members of every collection that has the base (so a `^` leaves them out). The pool is planes 15 and 16, then the BMP private-use area, so a range over all of planes 15-16 still leaves stand-ins. A class atom (`\k \i \f \p`, capitals without digits) is spelled as its ranges with every stand-in cut out and the stand-ins whose base it takes put back (`class_spelled`), which is what makes `<1‍\K>x</1‍1‍>` pair nothing.
- C2 (F9): a `-` last in a collection is written `\-` before stand-ins are put after it.
- Stand-ins are made only for the combinations of a base's clusters that graphemes of the text satisfy (`occurring`), not all 2^n.
- C3 (F6): every look at the clock records its line (`#[track_caller]`); grapheme passes look by bytes read (`Meter`, `metered`: never more than 4096 between looks, a grapheme being at most `SEGMENT` = 2048 bytes); pattern pairing finds `<`, `>` and name ends a stretch at a time. Every compilation goes through `compile`, which looks first; a pattern name spelling out to over `SPELLED` (4080 bytes) fails the object, so each compilation is one step of at most 4096 bytes (the synthetic compile accounting is gone). Tests: `expiring_at_any_look_*` run each command once, then expire the clock at every one of its looks in turn (the object fails, nothing changes, nothing compiles past the clock, no pass takes over 4096 steps between looks); `every_look_at_the_clock_is_taken_and_compiling_looks_first` checks that the commands reach every look site in `delimiters.rs` and that `Pattern::parse` is called only from `compile`.
- Out of scope, unchanged: the one-time keyword-table scan behind `\k` (a process-wide `OnceLock`), and a regex search or a span copy, which each count as one step after a look (the search's own deadline bounds it).
- Oracle: the new `delimiters.toml` cases were recorded with nvim 0.12.6 (the only nvim installed here) by the real recorder, `support::NVIM` set to 0.12.6 for the run and reverted, and the file keeps the corpus's `0.12.4` stamp, per docs/decisions/q3sndmknyh-line-end-motions-oracle-stamp.md; the pin is unchanged. The recording left every existing case as it was.
- The clock tests live in `src/tests/delimiters_clock.rs` (`mod clock_tests`), not `src/tests/delimiters_work.rs`: the regression check carries every `tests/**` file onto base, and base declares `delimiters_work.rs` itself, so that file is back to its base content (undeclared now) and base builds its own work tests beside the regression test.
- Review round 1 (fxk5kd): F1, a `^` right after a collection's negating `^` is a member, so the members pattern writes it `\^` (oracle cases for `[^^b]` and `[^^a]` with joined bases, recorded as above). F2/F3, the clock counts the steps since it was last looked at, one count for every pass (`Clock::ahead`, which replaced the per-pass `Meter` and every `% WORK` loop): each pass looks on its first step and whenever its next step could take the count past 4096, so two name streams read side by side share it, and `choose` budgets the `1 + holders` it charges. Composing sets (sorted) are compared in one pass (`subset`), and `satisfied` and `matches` look before a comparison could pass 4096. New expiring fixtures: long case-different ASCII names, graphemes of 600 marks compared in another order and satisfying clusters, and `[aR][bR]` over a 2,048-char private-use range; each fails if its fix is undone. nvim pairs no name with a char of more than 6 composing chars, which fe-vim doesn't follow (out of scope here); those fixtures test only accounting.
