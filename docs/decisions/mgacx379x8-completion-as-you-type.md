# Completion as you type (T668, Mac T1918)

The completion menu (`crates/fe-gpui/src/completion_menu.rs`) opened only on
`ctrl-space` or a server's trigger character, and offered nothing without a
language server. Now it opens as you type a word (quick suggestions,
`completion_menu/auto.rs`), and the words of open buffers back it, after a
server's items or alone (`completion_menu/words.rs` over
`fe_editor::word_completion`). References: VS Code's `editor.quickSuggestions`,
`quickSuggestionsDelay`, `acceptSuggestionOnEnter` and `wordBasedSuggestions`,
and JetBrains' auto-popup completion.

## Defaults: `[editor.completion]`

| Key | Default | Why |
| --- | --- | --- |
| `auto_trigger` | `true` | As VS Code and JetBrains. `ctrl-space` and trigger characters ask either way. |
| `delay_ms` | `50` | Between VS Code's 10 ms and JetBrains' 100 ms: one ask per pause, not per key, at typing speed. 0 to 2000. |
| `min_word_chars` | `1` | As VS Code: the first word char asks. 1 to 20. |
| `in_comments` | `false` | As VS Code's `quickSuggestions.comments = "off"`. |
| `in_strings` | `false` | As VS Code's `quickSuggestions.strings = "off"`. |
| `accept_on_enter` | `"explicit"` | See the key rules. `"always"` is VS Code's `"on"`, `"never"` its `"off"`. |
| `words` | `"open_buffers"` | VS Code's `wordBasedSuggestions = "matchingDocuments"`: the open buffers in the language. Also `"current_buffer"` and `"off"`. |

The settings reach each editor with the rest of `[editor]`
(`EditorView::set_wrap_settings` and `apply_settings`, which a reload runs), so
a reload applies at once. A view made over a buffer an open editor with them
shows (a split: `PaneItem::split` builds it without the pane's settings) takes
that editor's, read from a shared cell in the editors' registry, as the split
is made inside the source's own update. An editor no host has given them to
(fe-gpui's own tests, previews) never asks by itself and offers no words.
Turning `auto_trigger` off drops the ask waiting out its delay and a menu
that opened by itself still awaiting its first answer; a menu asked for stays.

## When a quick suggestion asks

1. **A word char typed** (letters, digits, `_`, as the menu's typed word counts
   them) into a word of at least `min_word_chars` chars, with no menu open or
   awaited: an open menu narrows as it always did.
2. **After the delay, nothing typed since.** Each key typed drops the waiting
   ask and a word char starts a new one. Any keystroke in the editor, and each
   sync after a command or a click, that finds the text's version or the caret
   changed since drops it, so moving away and back within the delay drops it
   too; when it fires, the version and caret must still be as they were.
   `escape` and losing focus drop it.
3. **As invoked.** The provider is asked with `CompletionTrigger::Invoked`
   (LSP `triggerKind = 1`, no trigger character), as VS Code does. No new
   trigger kind: a server can't tell them apart anyway.
4. **Not**: outside vim's insert mode (normal and visual mode keys are
   commands; insert mode left within the delay drops it), with more than one
   caret (a session needs a lone caret), while a snippet's tabstops are under
   way, while an input method composes (its marked text has `enter` and
   `tab`: a composition starting drops the ask waiting out its delay, a menu
   that opened by itself and one still awaiting its answer, each frame and
   before each keystroke is matched; an answer landing then opens nothing), or in a comment or a string unless `in_comments` or `in_strings`. Two
   readings at the char just typed decide, either one enough. The syntax's
   highlights: a `comment*` capture; a `string*` one other than
   `string.special.symbol` (Ruby's `:name`) and `string.special.key` (TOML's,
   JSON's and YAML's keys, which are names); or an escape's (`"\na`, after
   `\n`, is in the string). And the host tree, whose nodes an injected
   language's captures hide from the highlights (a Rust doc comment's
   Markdown link text is `markup.link`): a node there or enclosing it whose
   kind names a comment, or a string (`string_literal`, `template_string`)
   with no interpolation (`interpolation`, `template_substitution`) between,
   unless the highlights call the char a key. Plain text has neither.
   **Unknown context asks nothing:** the reading waits until the tree has
   caught up with the text (its version, reparsed: an edited tree's nodes
   can stop short of text typed at their end) and the highlights there are
   complete (`Syntax::checked_highlights`: no query out of time, no injected
   language still compiling), 16 ms at a time, up to 30 times, then gives
   up. A buffer whose language is still loading has no syntax yet but isn't
   plain text: it waits the same way. A file too big to parse whole, with
   only a window's tree, never asks by itself; `ctrl-space` and trigger
   characters still do.
5. **Bounded reads.** Each keystroke reads at most about 400 bytes either
   side of the caret (`word_at`) and the char before it, never the row: a
   run longer than any word (100 chars) is cut.

## Key rules

- **`enter`** on a menu that opened by itself, its selection not moved, types
  its new line and closes the menu, under `"explicit"`: someone typing a word
  at the end of a line and pressing `enter` at speed means the line. Moving the
  selection, or opening the menu with `ctrl-space` or a trigger character, says
  they're choosing, and `enter` accepts. A trigger character counts as
  explicit, keeping `enter` accepting member completion after `.` as it did.
  `"always"` accepts whenever the menu shows; `"never"` never does.
- **`tab`** accepts either way. `escape` closes as before.
- **How.** The keymap matches first (kiln's interceptor) and runs
  `completion.accept` for `enter` and `tab` alike, so that command holds the
  accept where `enter` mustn't take it; the menu's own interceptor, which runs
  next for the same keystroke, settles it: `enter` closes the menu and is
  typed again as the editor's (a new line, in vim's insert mode too); any other
  key accepts. A held accept no keystroke settles (the palette) accepts once
  the dispatch is over.

## Words

- **What.** Runs of word chars of 3 to 100 chars not starting with a digit,
  from the buffer typed in and, for `"open_buffers"`, every other open editor's
  buffer in the same language (by the syntax's language, plain text with plain
  text; a buffer whose language is still loading shares with none, either
  way). There's no per-language word char set to read, so it's the menu's
  own (letters, digits, `_`), which is what an item's edit replaces.
- **Not** the word being typed: the word around the caret is left out.
- **Bounds.** Each buffer's scan reads at most 1 MiB (around the caret in the
  buffer typed in) and keeps at most 10 000 distinct words; at most 10 000
  items are offered. Scans run on a background thread and are kept per buffer
  and version, so a buffer not edited since isn't read again; the items are
  made back on the main thread (they aren't `Send`). A long buffer's scan is
  reused only while the caret stays a quarter of the window from an edge that
  isn't the text's; a word the window's edge cuts through isn't one.
- **With a server**, words come after all its items (group `u8::MAX`, ranked
  before matching, as extension snippets rank after a server's), and only words
  it didn't offer: none whose text equals an item's label, filter text or
  plain-text insert text. Without one, or if it answers nothing, words alone
  back the menu, so quick suggestions work in plain text and config files.
- **When.** Words join an invoked ask (`ctrl-space`, quick suggestions) and an
  incomplete list's re-ask; after a trigger character only if it's a word
  char, the rule extension snippets follow: after `.` a server's members are
  wanted, not every word in the file.
- **Rows.** Kind "text", as a server's text items are named in en-US; detail
  "Word in `<file>`" for a word only in another file, none for this buffer's.
  This buffer's words rank before other files' at an equal match.

## Known limits

As the spec's Known limits (lead respec after review round 2) set them:

- **Comment and string detection is best effort where highlights are layered
  or incomplete.** The check reads the captures at the char typed, and the host
  tree's nodes beneath an injected language's captures. Where those are
  incomplete or ambiguous (an injected language's captures over a host comment
  or string, a query out of time, an injected grammar still compiling), it
  waits briefly for complete highlights and otherwise asks nothing. It doesn't
  promise a right answer for every injection layering; hardening that is a
  follow-up (review F8/F9).
- **The same wherever the syntax context isn't known yet:** a buffer whose
  language or grammar is still loading, a tree that hasn't caught up with the
  text, or highlights still computing. The ask waits briefly and otherwise
  asks nothing, and the word source keeps a loading buffer's words apart from
  other buffers' (review round 3 F1). Beyond that, how quick suggestions and
  words behave while a language loads is best effort, not a criterion.
- **Files too big to parse whole** (only a window's tree) never open the menu
  by themselves; `ctrl-space` and trigger characters still work there.
