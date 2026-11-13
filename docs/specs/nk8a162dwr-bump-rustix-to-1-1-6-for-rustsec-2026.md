---
task_uid: "01m796pexk8e7g1rnk8a162dwr"
task: "T591"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Cargo.lock resolves rustix \u003e= 1.1.6 and no other package version changes"
    required: true
    manual: false
  - id: "C2"
    text: "cargo deny check advisories reports no RUSTSEC-2026-0486"
    required: true
    manual: false
---

# Bump rustix to 1.1.6 for RUSTSEC-2026-0486 (CI deny) (Mac T1755)

CI's `deny` job fails on RUSTSEC-2026-0486: rustix 1.1.5's xattr functions mark uninitialised
bytes as initialised for a zero-length buffer (UB reachable from safe code). Patched in >= 1.1.6
(advisory: "try `cargo update -p rustix`"). rustix 1.1.5 comes in via alacritty_terminal 0.26.0
-> fe-terminal; the 0.38 series is unaffected.

Change: in Cargo.lock only, `cargo update -p rustix --precise 1.1.6` (or the newest 1.x that
the lock resolves, if 1.1.6 is yanked; check `cargo search`/crates.io). No Cargo.toml edits.
kiln has no Cargo.lock of its own. Do not touch any other package's version (`-p rustix` only).

Check: `cargo deny check advisories` is clean for RUSTSEC-2026-0486. Do not use a bare
`cargo deny check` as a criterion (other advisories the gate skips would fail it).
Linux-buildable: lockfile only. rustix has no Windows code path of note, but `cargo check
--workspace` must still resolve offline-clean.
