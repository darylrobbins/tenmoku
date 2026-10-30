---
task_uid: "01m4sb68qnmyw3d8ek9jc2zd0s"
task: "T582"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "After T564 lands, the oracle case yit_count_2_over_pattern_names (keys 2yit at (0,153) over '\u003cb\u003e' + 30 '\u003ca.b\u003e' + 'x\u003c/axb\u003e\u003c/b\u003e') takes well under the 2 s search time limit under heavy load: 28 concurrent copies of 'cargo test -p fe-vim --test delimiters_oracle' all pass, with the command, load average and the case's slowest per-call time recorded in the notes. Baseline at 3eec614a (debug, load ~55 on 14 cores): alone 0.24-1.1 s; 28 concurrent copies 1.8-2.2 s, 24 of 28 failed with the text object returning nothing."
    required: true
    manual: false
  - id: "C2"
    text: "If T564's compile-once/prefilter (its C9) does not bring the case under that bound, fix the remaining superlinear cost in crates/fe-vim/src/delimiters.rs pattern-name pairing (not by raising the limit or weakening the oracle case), with a deterministic step- or allocation-counting test rather than a wall-clock one."
    required: true
    manual: false
  - id: "C3"
    text: "Nothing outside the scoped files changes, except this lane's own docs/specs file."
    required: true
    manual: false
---

# Flake: fe-vim delimiters_oracle delimiters_matches_neovim fails under load on yit_count_2_over_pattern_names (2yit over 30 <a.b> pattern-name tags hits the 2 s search clock): verify T564's C9 makes it fast under load

## Implementation notes

- T564's compile-once/prefilter (C9) is already in `delimiters.rs`, so C2 needed no code change.
- Removed the interim 60 s `set_time_limit` from `delimiters_matches_neovim`; the oracle now runs under the default 2 s search clock.
- C1: 28 concurrent copies of the debug `delimiters_oracle` test binary (the one `cargo test -p fe-vim --test delimiters_oracle` builds), run from `crates/fe-vim`, load average 12 before and 28 after. All 28 passed (exit 0, `1 passed`).

### C1 measurement (round 2)

Command, run from the repo root after `cargo test -p fe-vim --test delimiters_oracle --no-run`; `$B` is the debug test binary that build printed, `target/debug/deps/delimiters_oracle-<hash>`, run from `crates/fe-vim`. The loop was `for i in $(seq 28); do ( $B --nocapture > $D/$i.out 2>&1; echo $? > $D/$i.rc ) & done; wait`, with a temporary, uncommitted `Instant` around each `check(case)` printing `CASETIME <elapsed> <name>`.

- Load average 15.05 before, 31.54 after (14 cores). 28 of 28 exited 0.
- `yit_count_2_over_pattern_names`, 28 samples: slowest 297 ms, the next two 296 ms and 291 ms (baseline: 1.8-2.2 s and 24 of 28 failing at 3eec614a). That is about 7x under the 2 s limit.
- Not part of C1 but seen in the same run: the tiny `\K`/keyword-atom pattern-name cases (`dit_pattern_name_keyword_atom_not_digit`, `dit_pattern_name_starting_with_a_keyword_atom`) took 1.27-2.07 s under that load, so the cost is in compiling the Unicode-wide class, not in the pairing search. They passed in all 28 runs.
