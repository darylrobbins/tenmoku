---
task_uid: "01m6c9t4cm58wt5tzhx9p5chme"
task: "T269"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test matchit_tags_pair (crates/fe-vim/tests/matchit_tags.rs) passes with nvim-probed expectations: from div's name % lands on the / of \u003c/div\u003e and back on d, nested same-name tags pair by depth, \u003c and \u003e pair, \u003c!-- and --\u003e pair, in html, xml and markdown"
    required: true
    manual: false
  - id: "C2"
    text: "Test matchit_tags_edges passes, each expectation probed in nvim: a self-closing tag, an html void element, an unmatched closing tag, a tag split over rows, a tag inside an attribute value; names compare as plain text"
    required: true
    manual: false
  - id: "C3"
    text: "Test matchit_tags_are_bounded passes: on a 30k-row file of nested tags % stays within the command's work budget, asserted by a work counter, not wall time, and a budget run out changes nothing"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-vim passes"
    required: true
    manual: false
---

# fe-vim: % and g% jump between an HTML, XML or Markdown tag's name and its closing tag, between < and >, and between <!-- and -->, as nvim's matchit does (Mac T1295)

## Problem

Second half of matchit, which the user approved on 2026-11-01 ("HTML/XML open/close tags"). The keyword card
(depends-on) gives `%` and `g%` per-language tables for Lua, bash and make. In HTML, XML and Markdown `%` still
only matches brackets: from `<div>`'s name it goes to its `>`.

Probed (nvim 0.12.6, `nvim --clean --headless --cmd 'packadd matchit' --cmd 'filetype plugin on'`, filetype set
per case):
- `<div class="a">` / `  <p>hi</p>` / `</div>`, caret on `i` of `div` (1:2): `%` lands on the `/` of `</div>`
  (3:1); `%` again lands on `d` (1:1).
- `<div><br/><div>x</div></div>`, caret on the first `<` (1:0): `%` lands on that tag's `>` (1:4): the `<:>`
  pair comes before the tag-name pair when the caret is on `<`.
- nvim's html `b:match_words` also has `<!--:-->`, and `ul`/`ol` … `li` … `/ul` and `dl` … `dt`/`dd` … `/dl`
  groups; xml adds `<![CDATA[:]]>` and `<?name:?>`.

## Change

In `crates/fe-vim/src/feature/matchit.rs`, with the tag scanner in a new `crates/fe-vim/src/feature/matchit/tags.rs`,
for the languages `html`, `xml` and `markdown`:

- `%` on a tag's name (an opening tag `<name …>` not ending `/>`) goes to the `/` of its closing `</name>`,
  counting nested tags of the same name; from a closing tag's name or `/` it goes back to the opening tag's name.
  `g%` goes the other way (the same pair, two words, so the same jumps).
- On `<` or `>` of a tag: the `<:>` pair.
- `<!--` and `-->`: each other's start.
- Names compare as plain text, exactly (no case folding in xml; html as nvim's probe shows), never as a pattern:
  fe-vim's `at`/`it` emulate nvim's regex-in-names and that took T417 seven revisions; this card doesn't.
- A self-closing tag (`<br/>`) opens nothing. Probe and match nvim for an html void element with no closing tag
  (`<br>`), a closing tag with no opener, and a tag split over rows; document each.
- Out of scope (follow-ups): the `ul/ol/li` and `dl/dt/dd` groups, CDATA, processing instructions, JSX tags in
  javascript/tsx, skipping tags inside comments or strings.
- Work is bounded: scan with a per-command limit as `delimiters.rs` does (`Clock`), never the whole buffer per
  key on huge files; a budget run out fails the motion and changes nothing.

Tests: new `crates/fe-vim/tests/matchit_tags.rs`, every expectation probed in nvim; include pathological markup
(tags in attribute values, `<` in text, deep nesting, a 30k-row file) as codex fuzzes it.

## Implementation notes

- Probes need `--cmd 'syntax off'`: `nvim --clean` turns syntax on, and then matchit's default `b:match_skip`
  skips matches in html strings and comments (`<a title="<a>">x</a></a>` lands on the first `</a>` with syntax,
  the second without). Skipping is out of scope, so the tests follow nvim with syntax off; `tags.rs` says so.
- `searchpair()` counts every match start, overlapping ones too (it ignores `cpoptions` `c`), so a same-name tag
  in an attribute value nests and `<a <a <a >x</a>` fails, as in nvim.
- Divergences, documented in `tags.rs` and pinned by its unit tests rather than the nvim-probed file: names are
  plain text (nvim's `<\@<=div[^>]*` also counts `<divx>` and html's `<div/>`; `<a.b>` closes at `</axb>`); on a
  match with no partner nvim's `d%` deletes the char under the caret (matchit's `v` stays), where fe-vim fails and
  changes nothing, as for the keyword groups.
- A self-closing tag opens nothing, as the spec says: in html a tag whose attributes end with `/>`
  (`<div class="x"/>`, `<div/>`, `<div title=">"/>`) is passed over when counting `div`s and `%` from its name
  fails. Its end is the first `>` outside `"`/`'` quotes after its name, so a tag in an attribute value
  (`<a title="<b/>">`) ends at its own `>` and its host at the last; one split over rows is read on from the
  next rows, each row read once per quote state a `%` enters it in (memoized, so linear; a test counts it).
  Attributes running to the end of the text open. nvim counts all these as openers (from the outer `<div>` of
  `<div><div class="x"/></div>` it fails); the nvim-probed test file says so where it pins this.
- html folds case a char at a time as nvim's `\c` does (`Σ` and `ς`, the Kelvin sign and `k`, `ı` only itself),
  with `delimiters::fold` (made `pub(crate)`, a one-word change outside the card's files). nvim's NFA compares a
  char with composing chars exactly (probed: `<á>` doesn't close at `</Á>`, `<Ká>` closes at `</ká>`, `<a‍b>`
  at `</a‍B>`), so a grapheme of more than one char compares exactly here (UAX 29 graphemes stand in for nvim's
  composing chars). Names are compared only when their char counts agree, which keeps the work linear: names
  that start apart with as many chars end apart.
- The search time limit is checked at a `%`'s first step and again before it lands, besides every 4,096 steps, so
  a short `%` past the limit fails too; the test makes each deadline past at once with
  `search::set_call_limit(Some(0))`, which is per thread where `set_time_limit` is process-wide.
- The work counter and a test budget are `fe_vim::{take_tag_work, set_tag_budget, TAG_BUDGET}` (doc-hidden, as
  `take_row_bytes` is), re-exported from `lib.rs`. `%` runs twice a key (`marks.rs` asks whether it lands for
  `''`), each run with its own budget, so the test bounds a key at twice one run's linear work.
- Carried T266 F1: make's and nmake's placement checks read a row's leading blanks once a row (`Lead`), with a
  unit test counting the bytes they look at on 100,000-blank rows.
