---
task_uid: "01m4p2yr79manxkk24bfq74ftp"
task: "T414"
rev: 1
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "crates/fe-vim/src/feature.rs FEATURES lists increment, folds, scroll, jump, prose, line_motions, delimiters, g_operators and jumplist (the five new ones Feature::NONE), and Feature has an object hook that src/reducer.rs asks for a char after i or a that its own text_object() maps to nothing, in operator-pending and visual modes"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::feature_stubs::new_variants_change_nothing: vim.move with each new Motion variant, vim.operate with each new TextObject and with lowercase, uppercase, toggle_case, rot13, format and format_keep over a motion, and vim.select with each new TextObject leave the text, the caret and the undo stack unchanged on HeadlessVim; each new variant round-trips through serde_json under its snake_case name"
    required: true
    manual: false
  - id: "C3"
    text: "src/reducer.rs gains only the hook API (Pending::operator, with_operator, operate, motion), the object-hook call, Pending::command_line leaving ? to step while Rot13 is pending, and the m and mark prefixes asking marks::settable and marks::jumpable, which return today's names"
    required: true
    manual: false
  - id: "C4"
    text: "Behaviour does not change: every existing fe-vim unit test and tests/*.rs passes with its file unchanged; after re-recording with nvim 0.12.4 (oracle.lua now also clearing the jumplist, marks A-Z0-9 and the search, cmd and input histories before each case) every tests/corpus/*.expected.json is byte-identical; scripts/check-no-gpui.sh prints ok: fe-vim"
    required: true
    manual: false
---

# Let fe-vim feature modules add motions, text objects and operators, with empty stubs for the M7 breadth cards

# fe-vim: feature modules can add motions, text objects and operators (stubs for the M7 breadth cards)

## Problem

The M7 exit criterion is met (1,956 nvim-checked cases), but common Vim keys are still missing
(gap analysis 2026-10-11, `crates/fe-vim/src/reducer.rs` `motion()` ~600–676, `text_object()`,
`args.rs` `Motion`/`TextObject`/`Operator`):

- motions `{ } ( ) H M L ge gE _ + - | gj gk`;
- text objects `ip ap is as it at i[ a[ i< a< i` a``;
- operators `gu gU g~ g? gq gw` (fe-vim's `Lowercase`, `Uppercase`, `ToggleCase` act only on a
  visual selection: `operator::operate` refuses them on a motion);
- the jumplist (`<C-o>`, `<C-i>`) and special marks (`lib.rs` "Known limits", ~575).

The user's standing priority (2026-10-11) is "the work that unblocks parallel vim work". T365 let a
feature module own its *keys and commands* (`src/feature.rs`), but a new motion, object or operator
still has to edit the hubs: a variant in `args.rs`, an arm in `motion.rs` `motion()`,
`text_object.rs` `select()`, `operator.rs` `operate()`, `visual/operate.rs`, `Motion::class()` /
`is_jump()` / `Target::writes_numbered()`, a `mod` line in `lib.rs` and `src/tests/mod.rs`, and a
line in `FEATURES`. Every breadth card would collide there and run in a serial chain. This card makes
those edits once, as stubs, so the next cards each change only their own module, test file and
corpus.

Builds on T366, which holds the same hub files (`feature.rs`, `lib.rs`, `reducer.rs`, `args.rs`,
`motion.rs`, `src/tests/mod.rs`, `tests/support/oracle.lua`): this card depends on it.

## Change

No behaviour changes: no key reaches the new variants until the feature cards take them.

### New modules (stubs), each `mod x;` in `src/lib.rs` and `pub(crate) const FEATURE = Feature::NONE`

| module | owner card | what it will hold |
|---|---|---|
| `src/prose.rs` | paragraph/sentence | `{ } ( )`, `ip ap is as` |
| `src/line_motions.rs` | line/screen motions | `ge gE _ + - \| H M L gj gk` |
| `src/delimiters.rs` | delimiter objects | `i[ a[ i< a< it at i\` a\`` |
| `src/g_operators.rs` | g-operators | `gu gU g~ g? gq gw` |
| `src/jumplist.rs` | jumplist | `<C-o> <C-i> <Tab>` |

`feature::FEATURES` lists them after T366's `jump`, in that order. Empty test stubs
`src/tests/{prose,line_motions,delimiters,g_operators,jumplist,marks_special}.rs` are declared in
`src/tests/mod.rs`.

### `src/feature.rs`

`Feature` gains `object: fn(&Pending, char, bool) -> Option<TextObject>` (the char after `i`/`a`,
and whether it was `i`); `Feature::NONE`'s returns `None`. `reducer.rs` asks it (first `Some` wins)
when its own `text_object()` maps nothing, in operator-pending and visual modes, then commits or
selects exactly as for a built-in object.

### `src/reducer.rs` (hook API only)

- `pub(crate)` `Pending::operator() -> Option<Operator>`, `Pending::operate(self, Operator, Target)
  -> Step` (sets the operator and commits, as `visual_key` does) and `Pending::motion(self, Motion)
  -> Step` (what `commit(Target::Motion(..))` does), so a key or `g` hook can commit a motion, a
  doubled operator (`gUU`) or start one (`gU` returns `Step::Wait` with the operator set; add
  `Pending::with_operator(self, Operator) -> Step` for that).
- `Pending::command_line` leaves `?` to `step` while `Operator::Rot13` is pending, so `g??` can reach
  a hook.
- The `m` and `'`/`` ` `` prefixes ask `marks::settable(ch)` and `marks::jumpable(ch)` (new
  `pub(crate)` fns in `src/marks.rs` returning today's sets: `a-z ' \`` and `a-z ' \` < >`).

### `src/args.rs`: variants, delegating

- `Motion`: `ParagraphForward`, `ParagraphBackward`, `SentenceForward`, `SentenceBackward`
  (prose); `WordEndBackward`, `BigWordEndBackward`, `NextLineFirstNonBlank` (`+`),
  `PrevLineFirstNonBlank` (`-`), `Column` (`|`), `WindowTop`, `WindowMiddle`, `WindowBottom`,
  `DisplayDown`, `DisplayUp` (line_motions). `_` is the existing `Motion::Line`.
- `TextObject`: `InnerParagraph`, `AParagraph`, `InnerSentence`, `ASentence` (prose);
  `InnerBracket`, `ABracket`, `InnerAngle`, `AAngle`, `InnerTag`, `ATag`, `InnerBacktick`,
  `ABacktick` (delimiters).
- `Operator`: `Rot13`, `Format` (`gq`), `FormatKeep` (`gw`). `gu gU g~` reuse `Lowercase`,
  `Uppercase`, `ToggleCase`.
- Serde names are snake_case as the others (`paragraph_forward`, `inner_tag`, `format_keep`).
- `Motion::class()`, `is_jump()` and `Target::writes_numbered()` arms for the new motions call
  `prose::class/is_jump/writes_numbered` and `line_motions::…` (stubs: `Exclusive`, `false`,
  `false`); the owning card sets the real values in its module.

### Dispatch arms, delegating to stubs

- `motion.rs` `motion()`: the new motions call `prose::motion(..)` / `line_motions::motion(..)`,
  stubs returning `Err(goal)` (a failed motion).
- `text_object.rs` `select()`: the new objects call `prose::select(..)` / `delimiters::select(..)`,
  stubs returning `Span::Failed`.
- `operator.rs` `operate()`: an operator other than `d y c > <` on a motion or object goes to
  `g_operators::operate(target, actor, args)` (stub: today's "acts only on a visual selection"
  error) instead of failing inline.
- `visual/operate.rs` and `visual/block.rs`: `Rot13` maps each char through `g_operators::rot13`
  (stub: identity) beside `Lowercase`/`Uppercase`; `Format`/`FormatKeep` over a selection go to
  `g_operators::format_selection(..)` (stub: the same error).

### `tests/support/oracle.lua`

Before each case also run `:clearjumps`, `:delmarks A-Z0-9` and `histdel` of the `search`, `cmd`
and `input` histories: the recorder shares one nvim (the buffer is wiped per case, but the jumplist,
file marks and histories are not), and the jumplist, marks and history cards need clean ones.

### Docs

`src/feature.rs`'s module docs say how a feature adds a motion, object or operator (variant here,
behaviour in its module). `src/lib.rs`'s crate docs say the feature modules' docs describe their
keys.

**Clean room** as every M7 card: `nvim --clean` (0.12.4) and `:help`; never source, hjkl, Zed or
Helix.

## Tests (new file)

`src/tests/feature_stubs.rs`: `vim.move` with each new motion, `vim.operate` with each new object
and with `lowercase`, `uppercase`, `toggle_case`, `rot13`, `format`, `format_keep` over a motion,
and `vim.select` with each new object, on `HeadlessVim`: the text, the caret and the undo stack
don't change. Each new variant round-trips through serde_json.

## Size

About 250 non-test lines, all plumbing.

## Out of scope

Any behaviour of the new keys (the next cards). `=` (needs a decision: nvim's is C-indenting).
`src/state.rs`, `src/headless.rs`, `src/macros.rs` (held by T367).
