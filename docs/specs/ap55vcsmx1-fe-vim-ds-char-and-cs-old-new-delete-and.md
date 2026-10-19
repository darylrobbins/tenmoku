---
task_uid: "01m55mj3xx6jht96ap55vcsmx1"
task: "T149"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test surround_deletes_pairs (top level, in crates/fe-vim/tests/surround.rs) passes: ds( ds) dsb ds{ dsB ds[ dsr ds\u003c dsa ds\" ds' ds` and dst on vim-surround's documented examples, ds( trimming the white space inside, 2ds( taking the outer pair, and a missing pair changing nothing."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test surround_changes_pairs (top level, same file) passes: cs\"' , cs'( (adding inner spaces), cs') (not), cst\" , cs]{ , . repeating on the next pair, one u undoing a change, and two carets each changing their pair."
    required: true
    manual: false
  - id: "C3"
    text: "The integration test feature_prefix_waits_and_cancels (top level, same file) passes: after d then s, vim.pending_kind names the feature's prefix and Esc cancels with nothing changed; ds followed by a char naming no pair cancels; and cargo test -p fe-vim passes with any changed existing test named in the finish notes."
    required: true
    manual: false
---

# fe-vim: ds{char} and cs{old}{new} delete and change surrounding brackets, quotes and tags as vim-surround does, through a feature-owned key prefix (Mac T885)

Filed on argon from Mac T885 (card ~/coordination/argon-cards/zzp/T885.json rev 1; note zzp, the Vim parity track part 2). Mac task uid 01m55me5zy15ncshf9qz1sd713. Base: argon trunk (mac/main synced). Corpus cases are recorded with nvim 0.12.x; unit tests are named by full module path, integration tests by bare fn name. Held in the inbox: waits until Mac T836, Mac T834 (another host's or the Mac's) is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T885)".

## Problem

At Mac main 6f032b12 fe-vim has no surround keys: `ds"` and `cs"'` cancel (`s` isn't a motion after `d` or
`c`). The user approved built-in plugin emulations in the "Vim parity" track on 2026-10-17, surround
(vim-surround, as VSCodeVim and IdeaVim emulate it) first. fe-vim's feature seam (`crates/fe-vim/src/feature.rs`)
can't hold a feature's own multi-key prefix: only the reducer's `Prefix` enum (`crates/fe-vim/src/reducer.rs`)
waits for keys, and features get one key at a time with no state of their own (the jump's `gs{c1}{c2}` needed
a `Prefix::Jump` variant for this).

## Change

1. Seam, in `crates/fe-vim/src/reducer.rs` and `crates/fe-vim/src/feature.rs`: a `Prefix::Feature` variant
   carrying a small `Copy` value a feature owns (which feature, plus the keys and operator state it has
   gathered), `Pending::with_feature_prefix(..)` for a hook to wait on it, a `prefix` hook on `Feature`
   (`Option<fn(&Pending, FeaturePrefix, VimKey) -> Option<Step>>`) that gets the next key, and a
   `PendingKind::Feature` for `vim.pending_kind`. `<Esc>` and keys the hook refuses cancel, as other
   prefixes do. Document it in feature.rs's module docs.
2. New feature module `crates/fe-vim/src/feature/surround.rs`, declared in feature.rs and listed in its
   `FEATURES`, registering a `vim.surround` command:
   - `ds{c}` deletes the nearest pair around the cursor: `( ) b`, `{ } B`, `[ ] r`, `< > a`, `' " ``,
     `t` (an HTML tag); `ds(` (an opening bracket) also trims the white space just inside. A count picks the
     count-th enclosing pair, as `2di(` does.
   - `cs{old}{new}` replaces the pair; an opening bracket as `{new}` adds a space inside each side, a closing
     one doesn't, and `{old}` `t` keeps the tag's contents. (A tag as `{new}` comes with the `ys` card.)
   - Pairs are found as fe-vim's `a(`/`a"`/`at` objects find them (`text_object::select`), so `ds"` follows
     their quote rules. No pair fails, changing nothing.
   - One undo step; `.` repeats (vim-repeat's behaviour); multiple carets each act.

Behaviour reference: vim-surround's documented behaviour (its `doc/surround.txt` examples), not its source;
nvim has no surround, so there's no oracle corpus: the expectations come from that documentation and are
written in a new integration test file `crates/fe-vim/tests/surround.rs`. Check `src/tests` for tests that
assert `ds` or `cs` cancel (`rg 'run\("[dc]s' crates/fe-vim/src/tests`) and say if one must change.

reducer.rs is changed by the lent T800, T803, T805 and T836 (T836 last) and feature.rs by T798, T801 and
T834 (T834 last): this card depends on T836 and T834.

## Implementation notes

- The seam: `Prefix::Feature(FeaturePrefix)` in reducer.rs, with `PendingKind::Feature(name)` (its
  `vim.pending_kind` is the feature's name, `surround`), `Pending::with_feature_prefix`, and
  `Feature::prefix` hooks dispatched by `feature::prefix`. `FeaturePrefix` (feature.rs) holds the
  feature's name and up to four keys; the operator and count stay in the `Pending`.
- `.` needs a `Change` variant, so `crates/fe-vim/src/repeat.rs` (outside the card's scope) gains
  `Change::Surround(SurroundArgs)`: five lines, replaying through `surround::surround`.
- Every opening bracket as `{old}` trims the blanks inside (`ds{`, `ds[`, `ds<` too, as
  vim-surround's doc says), not only `(`. `{new}` takes the brackets and their letters, and any other
  ASCII punctuation as itself on both sides; `<` and `t` cancel, left for the `ys` card's tags.
- A count picks the count-th enclosing bracket or tag; quotes ignore it.
- With `set_jump_on_s` on, `s` after an operator stays the jump's (`ds{c1}{c2}`).
- No existing test asserted that `ds` or `cs` cancel (`rg 'run\("[dc]s' crates/fe-vim/src/tests`
  finds none); no existing test changed.
