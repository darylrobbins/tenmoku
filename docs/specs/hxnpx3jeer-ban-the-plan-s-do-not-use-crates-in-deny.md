---
task_uid: "01m4sfx32hnnd5svhxnpx3jeer"
task: "T69"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fastedit test deny_bans::plan_bans_are_listed passes: deny.toml's [bans] deny list names tree-sitter-language-pack, stack-graphs, nucleo, nucleo-matcher, tree-house and tree-house-bindings, each with a non-empty reason, and [licenses] allow still has no MPL-2.0 entry"
    required: true
    manual: false
  - id: "C2"
    text: "cargo deny check licenses bans sources passes (the gate's deny stage)"
    required: true
    manual: false
  - id: "C3"
    text: "deny.toml changes only inside [bans] (a comment and the deny list): no [licenses], exception, [advisories] or [sources] line changes, and kiln/deny.toml is unchanged"
    required: true
    manual: false
---

# Ban the plan's do-not-use crates in deny.toml: tree-sitter-language-pack, stack-graphs, nucleo and tree-house, each with its reason (Mac T593)

Filed on argon from Mac T593 (card ~/coordination/argon-cards/T593.json rev 1). Mac task uid 01m4sdx0xawpyzz097y6rrggdx. No dependencies. deny.toml is in regatta.toml's human_paths, so the user approves it at merge: say so in the decision file. Title the host commit "... (Mac T593)".

## Problem

The plan's reuse map ends with a "Do not use" list: the `tree-sitter-language-pack` bundle ("it includes GPL/WTFPL
grammars"), `stack-graphs` (archived), `nucleo` ("MPL; use `frizbee`") and `tree-house`. Nothing enforces it.
`deny.toml`'s `[bans]` has only `multiple-versions`, `wildcards` and `allow-wildcard-paths`, and no file mentions
these crates (`rg -i "nucleo|tree-house|stack-graphs|language-pack" deny.toml CLEAN_ROOM.md` is empty).

The license allowlist catches only some of them. `nucleo` and `tree-house` are MPL-2.0, which `[licenses]` rejects
unless someone adds an exception, and exceptions are added crate by crate (`deny.toml` lines 22-30), so one added
for an unrelated reason would let them through silently. `tree-sitter-language-pack` and `stack-graphs` declare
permissive licenses (MIT or Apache-2.0), so the gate's `cargo deny check licenses bans sources` passes them today,
although the bundle compiles in GPL and WTFPL grammar sources the crate's own license field doesn't describe. None
of them is in `Cargo.lock` now, and T62 used `nucleo` as a dev-dependency once before dropping it.

## Change

Add a `deny` list to `deny.toml`'s `[bans]`, one entry per crate with the `reason` field cargo-deny 0.20 supports
(`deny = [{ crate = "…", reason = "…" }]`), each reason quoting the plan's word for it:

- `tree-sitter-language-pack`: bundles GPL/WTFPL grammars; add grammars one crate each through fe-syntax features.
- `stack-graphs`: archived; navigation is fe-nav's tags index, then the LSP, then SCIP.
- `nucleo` and `nucleo-matcher`: MPL-2.0; fuzzy matching is `frizbee`.
- `tree-house` and `tree-house-bindings`: MPL-2.0 and needs ropey 1.x; highlighting is fe-syntax's own.

Add a comment above the list naming the plan's "Do not use" list as its source. Nothing else in `deny.toml`
changes: no license, exception, advisory or source edit. `kiln/deny.toml` doesn't change (kiln uses none of them).

`deny.toml` is in `regatta.toml`'s `human_paths`, so this candidate needs a person's approval at merge; say so in
the decision file.

## Tests

A new test file `crates/fastedit/tests/deny_bans.rs` reads `deny.toml` as text at run time (no new dependency) and
checks that the `[bans]` table's `deny` list names each of the six crates with a non-empty reason, and that the
`[licenses]` `allow` list still has no `MPL-2.0` entry. Line-based parsing is enough; the file's format is ours.

## Implementation notes

- deny.toml is in regatta.toml's human_paths: the user must approve this candidate at merge.
