---
task_uid: "01m5psn6q3k0afn59b5zxjnarb"
task: "T222"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "A cancelling Backspace (on an empty line) produced by a map keeps every remaining key, and expansions run incrementally against the actual line text (the input's grapheme and selection deletion on the direct path) and the mode left; selection state in the platform-settle path and in \u003cCmd\u003e continuations is a documented limit (T261)"
    required: true
    manual: false
  - id: "C2"
    text: "Replay resolves a held prefix before a physical Enter/control key as the live line does (dropping the key if that closes the line), and sends a recursive map's special keys through the mapping stage; the held key's line boundary across a reopened line or a suspension is a documented limit (T262)"
    required: true
    manual: false
  - id: "C3"
    text: "A \u003cCmd\u003e map on the live line carries the visible input's text into the continuation and writes its edits back, including across suspended fe: and host commands, keystrokes typed meanwhile following it, and a line it reopens shown with its prefix and preview; non-keystroke edits during the wait, platform text ordering after the tail (T261) and a tail that leaves a prefix held (T262) are documented limits"
    required: true
    manual: false
  - id: "C4"
    text: "Edits the maps never saw (BS, paste, recall) that precede a map-ended line are recorded before the mapping's keys; grapheme-aligned recording diffs for combining characters are a documented limit (T262)"
    required: true
    manual: false
---

# Command-line cnoremap parity: live vs replay edge cases (T208 review follow-ups)

Follow-ups from T208's (cnoremap on the status bar line) review rounds 1-2, left as documented limits there:

- Expansions are all matched under CmdLine mode, then applied; keys after a terminating key go to the editor, but a cancelling `<BS>` on an empty line, and host-command continuations, are not handled incrementally.
- `replay_key`'s CmdLine arm asks `remap_insert` for a Char, or any key while a prefix is held; the live line resolves the prefix before a physical Enter. A recursive RHS special key bypasses the mapping stage.
- `<Cmd>` on the live line hands the rest to `replay_from` with no line text, so the visible input and replay's hidden line diverge.
- Edits the maps never saw are reconciled at the end of the line (diff against the echo), so they land after an already recorded terminating mapping.

## Implementation notes

- C1: matching on the live line stops at every `<BS>` a map types (`LineKeys::then`): the bar takes it out of the input as its own `<BS>` does (a grapheme, or the selection), then, if the line is still open, matches the keys after it from the input's actual text (`EditorView::remap_line_more`); if it cancelled the line, they go to the editor with `rest`, in the mode the cancel leaves (normal maps apply), and the cancel is recorded once. The settle path (`line_settle`) does the same against the text its edits leave. `StatusBar::type_line_keys` no longer pops a key once the line has closed. A submit that hands the workbench a file or window command (`:w`) suspends the keys after it on a `HostWait` (`EditorView::suspend_for_host`, split out of `wait_for_host`), which the workbench's report resumes, as a replay's do.
- C2: in a replay's cmdline arm a key goes through the `c` maps if it is a character or a map's right side (`remap_expansion_next`), so a recursive map's `<C-x>` is matched as on the live line. Any other key (a recorded `<CR>`, `<BS>`, `<C-c>`) first resolves a held prefix as a timeout does (`unmapped_after_held`, which generalises the T253 marker rule) and is held back until what they resolve to has run; if that closed the line, it is dropped, as the live line drops it. A replay that suspends meanwhile puts it back in its queue.
- C3: `replay_line_rest(rest, line, handed)` takes the visible input's text when the line is still open (after a `<Cmd>`), so the continuation edits the text the user sees; the hidden line's `<BS>` takes out a grapheme (`EditorView::line_backspace`). Its state is kept per editor in the `Vim` global (`live_lines`: running, then done with the text or ended), because `view.rs` is outside the card's scope. When it is over, after a suspended `fe:` or host command too, the bar shows the text (`take_line_text`; the echo is that text, so nothing is recorded twice), or, if the keys closed the line (and maybe opened another, of any kind), follows the editor's line (`take_line_ended`: reopened with its prefix, recall and preview from the prefill). While it waits, the bar holds keystrokes typed on the line with a keystroke interceptor and dispatches them once it is over, so they come after it and never take its keys' remap tags.
- C4: since T208 rev 4 and T253, bypassed edits (BS, paste, recall) are synced as they happen (`sync_line_text`). `bypassed_edits_are_recorded_before_a_map_that_ends_the_line` pins the order in the register, including a BS that takes out a held prefix.
- Limit: an edit that reaches the line while a continuation waits other than by a keystroke (a paste from a menu) is not held.
- Review round 2: a dropped held-back key is read (`record_key`) without running, so its replay place goes with it; a line a `<Cmd>`'s keys reopen is previewed; keys held after a map's `<BS>` arm the `CHORD_TIMEOUT` timer. The remaining medium findings are follow-up cards, per the zz49 process: T261 (F1 selection in settle and `<Cmd>` continuations, F2 non-keystroke edits during a wait, F6 platform text after a map's tail) and T262 (F4 the held key's line boundary across reopen and suspension, F5 grapheme-aligned recording diffs, F9 a continuation whose tail leaves a prefix held). The criteria name them as documented limits.
- Rev 2 review (F9): a continuation that closes the host's line stays running (`LiveLine::Running { closed: true }`) until its keys are over, through later `fe:` or host-command waits, so the bar keeps holding keystrokes and opens the line the keys opened only once its text is the prefill. F1, F2, F4, F5 and F6 of that review are T261's and T262's, as the criteria say.
- Rev 2 review round 2 (F10): `end_live_line` gives a line the continuation reopened its text (the prefill) on every end, a failed `fe:` or host command included, not only when its keys run to the end.
