---
task_uid: "01m4nyq8j48ayqa8w7kwn7j8st"
task: "T365"
rev: 1
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "crates/fe-vim/src/feature.rs defines Feature (key, g, z and register hooks) and FEATURES listing increment::FEATURE, folds::FEATURE and scroll::FEATURE; src/increment.rs, src/folds.rs and src/scroll.rs are declared pub mod in src/lib.rs and each holds only a module doc and FEATURE = Feature::NONE; src/tests/increment.rs, src/tests/folds.rs and src/tests/scroll.rs are declared in src/tests/mod.rs and hold only a module doc"
    required: true
    manual: false
  - id: "C2"
    text: "src/reducer.rs asks the features' key hooks for every key it would otherwise reject (the count ended), their g hooks for any key after g it doesn't take, and opens a z prefix (PendingKind::Z, vim.pending_kind \"z\") only when some feature has a z hook; registry() runs every feature's register after its own commands; commands::follow, Pending::count, Pending::register, Pending::is_block and the reducer's invocation helper are pub(crate)"
    required: true
    manual: false
  - id: "C3"
    text: "Behaviour doesn't change: every existing fe-vim unit test, tests/host.rs and every nvim oracle corpus pass with their files unchanged (src/tests/mod.rs gains only the three mod lines); cargo clippy --workspace --all-targets -- -D warnings is clean; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim"
    required: true
    manual: false
---

# Let fe-vim feature modules register their own keys and commands, with empty increment, folds and scroll stubs

# fe-vim: feature modules register their own keys and commands

## Problem

Every fe-vim feature edits the same hub files, so the M7 cards run in a serial chain they don't
semantically need (the user approved taking them off it on 2026-10-11). `rgt task conflicts T227 T228`
lists `crates/fe-vim/src/lib.rs`, `args.rs`, `reducer.rs`, `commands.rs` and `src/tests/mod.rs`. What
each queued card adds to them today:

- `src/reducer.rs` (`step`, `Pending::complete`): T227 the `<C-a>`/`<C-x>` arms and `g<C-a>`/`g<C-x>`
  after the `g` prefix; T228 a `z` prefix with `a c o A C O R M v`; T229 `<C-d> <C-u> <C-e> <C-y>
  <C-f> <C-b>` and `z z`, `z t`, `z b`, `z<CR>`, `z.`, `z-` on that same `z` prefix.
- `src/args.rs`: an args struct each (`vim.increment {"delta","count"}`, `vim.fold {"action"}`,
  `vim.scroll {"action","count"}`).
- `src/commands.rs` (`registry()`): one `register_with_args` block each.
- `src/lib.rs`: a `mod` line, `pub use` of the args, and crate-doc lines each.
- `src/tests/mod.rs`: a `mod` line each (`increment`, `folds`, `scroll`), and T359 one more.

Keys the reducer doesn't take all end in `Pending::reject` (reducer.rs ~186: `Forward` with nothing
pending, else `Cancel`), and a key after `g` that isn't `g` or `v` ends in `Step::Cancel` in
`Pending::complete`.

## Change

Add one registration point, wire the three queued feature modules into it as empty stubs, and change
no behaviour. After this, T227, T228 and T229 each change only their own module, its test file and
files no other of them touches.

### `src/feature.rs` (new, private `mod feature;`)

```rust
pub(crate) struct Feature {
    /// A key `step` would otherwise reject, no prefix waiting (the count already ended).
    pub(crate) key: fn(&Pending, VimKey) -> Option<Step>,
    /// A key after `g` that `step` doesn't take.
    pub(crate) g: fn(&Pending, VimKey) -> Option<Step>,
    /// The key after `z`; `None` if the feature takes no `z` key.
    pub(crate) z: Option<fn(&Pending, VimKey) -> Option<Step>>,
    /// Adds its commands to vim's registry.
    pub(crate) register: fn(&mut Registry<VimCommands>),
}
```

with `Feature::NONE` (hooks that take nothing, `z: None`, `register` that adds nothing) and
`FEATURES: [&Feature; 3] = [&increment::FEATURE, &folds::FEATURE, &scroll::FEATURE]`, plus the small
helpers reducer.rs calls (first `Some` wins, in that order). Module docs say how a feature registers:
its own `pub(crate) const FEATURE`, keys it doesn't own return `None`.

### `src/reducer.rs`

- `Pending::reject` takes the key and asks every feature's `key` hook before falling back to
  `Forward`/`Cancel`, with the count ended. Every current caller passes its key.
- A key after `g` that `complete` doesn't take (any `VimKey`, not only chars) asks the `g` hooks
  before `Step::Cancel`.
- `z` with no operator pending, in normal or a visual mode, opens a new `Prefix::Z`
  (`PendingKind::Z`, `vim.pending_kind` `z`) **only if some feature has a `z` hook**; the next key
  (any `VimKey`, so `<CR>` reaches it) goes to the `z` hooks, else `Cancel`. With today's stubs no
  feature has one, so `z` is still rejected as now.
- `pub(crate)` accessors for the hooks: `Pending::count()` (the finished count), `register()`,
  `is_block()`, and the module's `invocation(command, args) -> Step` helper.

### `src/commands.rs`

`registry()` calls each feature's `register` after its own commands. `follow` becomes `pub(crate)`
so a feature's handler that edits text can catch the marks up as the others do. Its doc comment says
feature modules add their commands there.

### Stubs

- `src/increment.rs`, `src/folds.rs`, `src/scroll.rs`, declared `pub mod` in `src/lib.rs` (so a
  feature's public args are `fe_vim::folds::FoldArgs`, with no `pub use` in lib.rs), each only a
  module doc line and `pub(crate) const FEATURE: Feature = Feature::NONE;`.
- `src/tests/increment.rs`, `src/tests/folds.rs`, `src/tests/scroll.rs`, declared in
  `src/tests/mod.rs`, each only a module doc line, so the feature cards' test names
  (`tests::increment::numbers`, `tests::folds::…`, `tests::scroll::…`) need no `mod` line.

### `src/lib.rs` crate docs

One paragraph: keys and commands that live in a module of their own register through
`feature::FEATURES`, and each such module's docs describe its keys, its commands and any divergence
from nvim. The `PendingKind` docs gain `z`.

## Out of scope

Any key or command of the three features; `args.rs`, `repeat.rs`, `state.rs`, `headless.rs`,
`visual.rs`; any change outside `crates/fe-vim`; moving existing keys into feature modules.

## Size

About 120 non-test lines.
