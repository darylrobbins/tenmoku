---
task_uid: "01m46tytbn6gy69vd51t523tk1"
task: "T72"
rev: 4
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-keymap test tests::keystroke::parse_and_normalise: cmd-shift-left equals shift-cmd-left and both display as cmd-shift-left; cmd--, cmd-[, f12 and space parse; \"cmd-k cmd-0\" is a two-keystroke sequence; an unknown modifier or key name is an error"
    required: true
    manual: false
  - id: "C2"
    text: "fe-keymap test tests::keymap::layers_and_unbinding: a user entry with the same keys and context as a default replaces it; command = \"\" removes the earlier binding with the same keys and context and leaves the same keys in another context bound; each binding keeps its file and line"
    required: true
    manual: false
  - id: "C3"
    text: "fe-keymap test tests::keymap::entry_errors_keep_last_good: a bad keystroke, a bad context predicate, a malformed command ID and args rejected by Registry::validate each give an error diagnostic with line and column, and the layer’s previous version stays in effect; an unknown command (registry supplied) is a warning and only that binding is dropped"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap test tests::matcher::chords_and_pending: with cmd-k cmd-0 bound, cmd-k gives Pending and cmd-0 then gives Dispatch(editor.fold_all) with source keyboard and the stack stamped as context; cmd-k then x gives Unmatched([cmd-k, x]); with cmd-k and cmd-k cmd-0 both bound, cmd-k is Pending and flush() dispatches the cmd-k binding; flush() with nothing pending gives Unmatched([])"
    required: true
    manual: false
  - id: "C5"
    text: "fe-keymap test tests::matcher::context_precedence: in the stack [workbench, editor] an editor binding beats a workbench binding, which beats a context-free one, for the same keys; at equal depth the user layer beats the default and a later entry beats an earlier one; editor \u0026\u0026 !read_only never matches when the editor frame has read_only; vim.mode == normal matches only with that value"
    required: true
    manual: false
  - id: "C6"
    text: "fe-keymap test tests::keymap::report_finds_overrides_shadows_and_prefix_delays: report() lists Overridden for a user override, Shadowed for two same-key bindings whose contexts both hold in [[editor]], and PrefixDelay for cmd-k against cmd-k cmd-0, each naming both locations; a keymap without these reports nothing"
    required: true
    manual: false
  - id: "C7"
    text: "fe-keymap test tests::load::hot_reload_swaps_the_keymap: KeymapStore over a temp-dir keymap.toml fed by a real ConfigWatcher (recv_timeout(10 s) as an upper bound) changes what Matcher::push resolves after a valid edit, and keeps the previous bindings with diagnostics after an invalid one; no duration is asserted"
    required: true
    manual: false
  - id: "C8"
    text: "scripts/check-no-gpui.sh prints ok: fe-keymap; Cargo.lock gains no [[package]] entry other than fe-keymap; nothing under crates/fe-gpui or crates/fastedit changes"
    required: true
    manual: false
---

# Add fe-keymap: keystroke parsing, chords, context-aware matching and layered keymap files

# Add fe-keymap: keystroke parsing, chords, context-aware matching and layered keymap files

Foundation card 4a. Depends on card 1a (`fe-command`'s `CommandId`, `Invocation`,
`ContextPredicate`, `ContextStack` and `Registry::validate`) and card 3 (`fe-config`'s layer
loading, diagnostics and `ConfigWatcher`). The plan's Architecture table says `fe-keymap` handles
"chords, context predicates", GPUI-free.

This card is the headless engine. Card 4b connects it to GPUI and replaces card 1b's binding table
with a default keymap file.

## Problem

After card 1b, bindings are a hard-coded table handed to GPUI's own keymap:

- Users can't rebind or unbind keys.
- Nothing reports two bindings that shadow each other.
- Matching, including the `cmd-k cmd-0` chords, can't be tested without GPUI.

The keymap also has to be ready to route keys to `fe-vim` in vim contexts later.

## Change

New crate `crates/fe-keymap`. `scripts/check-no-gpui.sh` must print `ok: fe-keymap`.

- Add `fe-keymap = { path = "crates/fe-keymap" }` to `[workspace.dependencies]`.
- Use the workspace `serde` and `toml`.
- `Cargo.lock` gains no `[[package]]` except `fe-keymap`.

Files: `crates/fe-keymap/Cargo.toml`, `src/lib.rs`, `src/keystroke.rs`, `src/keymap.rs`,
`src/matcher.rs`, `src/report.rs` and `src/load.rs`.

**Keystrokes (`keystroke.rs`).**

- `Keystroke { modifiers, key }`. The modifiers are `cmd`, `ctrl`, `alt`, `shift` and `fn`.
  `Keystroke::parse("cmd-shift-left")` accepts them in any order.
- Key names:
  - a single printable character, including `-` (as in `cmd--`), `[` and `]`
  - `left`, `right`, `up`, `down`, `home`, `end`, `pageup`, `pagedown`
  - `backspace`, `delete`, `enter`, `tab`, `escape`, `space`
  - `f1` to `f20`
- `Display` prints a normalised form: modifiers in the fixed order above, lowercase. Parsing that
  form gives an equal keystroke.
- `KeySequence` is a whitespace-separated list of keystrokes, such as `"cmd-k cmd-0"`.

**Keymap files (`keymap.rs`).** The format is TOML:

```toml
[[bindings]]
keys = "cmd-k cmd-0"
command = "editor.fold_all"
context = "editor"                                 # optional; omitted = everywhere
args = { motion = "word_left", extend = true }     # optional

[[bindings]]                                       # unbind
keys = "alt-z"
context = "editor"
command = ""
```

- `Keymap::from_layers([default_layer, user_layer])` processes entries in order: each layer top to
  bottom, defaults first.
- An entry with `command = ""` **removes** every earlier binding with the same normalised keys and
  the same normalised context.
- Entry errors are diagnostics with the entry's line and column, reusing `fe_config::Diagnostic`.
  The errors are:
  - a bad keystroke
  - a bad context predicate
  - a malformed ID
  - with a `Registry` or command catalogue supplied, an unknown command or args that `validate`
    rejects
- Any error keeps that layer's last good version, matching card 3's rule. An unknown command is a
  **warning**: the binding is dropped, and a command from a later extension may not exist yet.

**Matching (`matcher.rs`).** `Matcher` holds a `Keymap` and the pending keystrokes.
`push(keystroke, &ContextStack) -> KeyOutcome` returns one of three outcomes:

- `Dispatch(Invocation)` (`source: Keyboard`, `context` stamped): the keys pressed so far exactly
  match a binding, and no longer binding in the current context starts with them.
- `Pending`: the keys are a strict prefix of a binding in context. The matcher remembers the best
  exact match so far, if there is one.
- `Unmatched(Vec<Keystroke>)`: nothing matches. It returns every swallowed keystroke plus this one
  so the host can deliver them as ordinary input. Pending is cleared.

More rules:

- `flush()` is called by the **host** when its chord timer fires. It returns `Dispatch` of the best
  exact prefix match, or `Unmatched(pending)`. `fe-keymap` has no clock.
- **Precedence** among bindings for the same keys, highest first:
  1. The deeper context match wins. A binding's depth is the length of the longest prefix of the
     stack, from the outermost frame inward, whose evaluation satisfies its predicate. A binding
     with no context has depth 0.
  2. Then the later layer.
  3. Then the later entry.
- `bindings_for(command)` lists the keys bound to a command, for menus and the palette later.
- `Matcher::set_keymap` swaps the keymap on hot reload and clears pending.

**Report (`report.rs`).** `Keymap::report() -> Vec<KeymapIssue>` has three kinds:

- `Overridden`: same keys and context, where a later layer or entry wins. Name the winner, the
  loser and both locations.
- `Shadowed`: same keys, and the predicates of both bindings hold in some context listed in
  `report_contexts`. The default is `[[editor]]`, and card 4b passes the real ones.
- `PrefixDelay`: a binding whose keys are a strict prefix of another's in an overlapping context,
  so it fires only on timeout.

**Loading and hot reload (`load.rs`).**

- `load_keymap(default_source: &str, user: Option<&Path>) -> (Keymap, Vec<Diagnostic>)` loads
  through `fe_config::load_layer`.
- `fe_config::paths::config_dir()/keymap.toml` is the user file.
- `KeymapStore::reload(&changed_paths)` mirrors `SettingsStore::reload`, with last good per layer.
  The host feeds it from the same `ConfigWatcher`.

### Tests (new files)

`crates/fe-keymap/src/tests/mod.rs`, declared from `lib.rs`, with `tests/keystroke.rs`,
`tests/keymap.rs`, `tests/matcher.rs` and `tests/load.rs`. All tests are headless. Keymap sources
are inline strings or `tempfile` files, and none of them uses a clock or sleeps. The hot reload
test uses `ConfigWatcher` with `recv_timeout(10 s)` as an upper bound only.

## Out of scope

- GPUI wiring and the default keymap file (card 4b).
- Vim key routing: per the vim spike's requirements, a later card adds a "mode handler" outcome.
- Platform-specific keymaps other than macOS.
- A keymap editor UI.

Clean room: don't read Zed's `settings`, `workspace` or `vim` crates, or Zed's keymap JSON
files. gpui's `keymap/` module is Apache-2.0 and may be read.

## Decided (lead defaults; the user may revisit)

- The format is a flat `[[bindings]]` list with `keys`, `command`, optional `context` and optional
  `args`. It's one entry per binding, easy to diff and grep.
- Unbinding is `command = ""`, removing earlier bindings with the same keys and context.
- Layers are built-in defaults < user `~/.config/fastedit/keymap.toml`. There's no project-level
  keymap, because keymaps are personal.
- Precedence is the deepest context match, then the later layer, then the later entry.
- The chord timeout is the host's job. Card 4b uses 1 s.
- An unknown command is a warning and the binding is dropped. Other entry errors keep the last good
  layer.

## Implementation notes

- **Context depth.** `fe_command`'s identifiers hold if *any* frame has them, so every matching
  binding also holds on the whole stack, and "the longest satisfying prefix" would tie `editor` with
  `workbench` in `[workbench, editor]`, against C5. `Binding::depth` is therefore the length of the
  *shortest* prefix that satisfies the predicate — the frame the binding needs — so `editor` is at
  depth 2 and `workbench` at depth 1. A binding with no context is at depth 0, and a binding whose
  predicate fails on the whole stack doesn't match.
- **Chords and changing context.** `push` returns only the three specified outcomes. While a chord
  is pending, the best exact match of a prefix of the keys pressed (the longest such prefix, then
  precedence) is recomputed on every keystroke in that keystroke's stack, so `flush()` dispatches
  what applies in the latest context, stamped with it. As specified, a key that doesn't continue
  the chord gives `Unmatched` with every pending key, even when a prefix had an exact match, and
  `flush()` drops any keys pressed after the prefix it dispatches.
- **Prefix delays.** `PrefixDelay` is reported only in a report context where the prefix binding
  would actually run, not where another binding for the same keys shadows it.
- **Command catalogue.** `CommandCatalog::check(command, args)` is implemented for `Registry<T>`
  (and through `&`, `Box`, `Rc` and `Arc`). `Registry::validate` checks a command's context before
  its args, and a binding has no stack to check against, so `fe-command` gains
  `Registry::validate_args(command, args)`: the same unknown-command and typed args checks, whatever
  context the command applies in. `validate` is unchanged.
- **Signatures.** `load_keymap(default_source, user)` is as specified and consults no catalogue.
  `KeymapStore::load(default_source, user, catalog)` takes an optional boxed catalogue, kept for
  every `reload(&changed_paths)`. `Keymap::report()` uses `[[editor]]`; `report_with(report_contexts)`
  takes the host's contexts. `bindings_for` is on `Keymap` and forwarded by `Matcher`, and leaves
  out bindings a later one with the same keys and context overrides.
- **Locations.** A binding's location is its `[[bindings]]` header (or inline table). Keystroke and
  predicate errors point inside the string when it's written without escapes, else at its start.
