---
task_uid: "01m55mj8m3ee191sqf7s68nptx"
task: "T161"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test set_search_options_report_as_nvim_does (top level, in crates/fe-vim/tests/ex_set_search.rs) passes: through HeadlessVim, :set ic?, :set noic, :set invscs, :set is!, :set ignorecase smartcase and :set incsearch? leave nvim 0.12.x's exact messages and Registers' values, and :set ic\u0026 and :set ic=1 are refused as T825 refuses such forms."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test ex_set_search_matches_neovim (top level, in crates/fe-vim/tests/ex_set_search_oracle.rs) passes on crates/fe-vim/tests/corpus/ex_set_search.toml, at least 8 cases recorded with nvim 0.12.x, each a :set then a search it changes."
    required: true
    manual: false
---

# fe-vim: :set ic, scs and is (with no, inv, ! and ? forms) set and report 'ignorecase', 'smartcase' and 'incsearch' as nvim does (Mac T895)

Filed on argon from Mac T895 (card ~/coordination/argon-cards/zzp/T895.json rev 1; note zzp, the Vim parity track part 2). Mac task uid 01m55meg7sjxtbxxjw440rm8vz. Base: argon trunk (mac/main synced). Corpus cases are recorded with nvim 0.12.x; unit tests are named by full module path, integration tests by bare fn name. Argon deps: T147 (Mac T825). Held in the inbox: waits until Mac T894 (another host's or the Mac's) is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T895)".

## Problem

T825 (lent) adds `:set` for wrap, tabstop, shiftwidth, expandtab and clipboard only; `:set ic`, `:set scs`
and `:set is` stay unknown options there, though nvim users type them constantly. The user approved
'ignorecase', 'smartcase' and 'incsearch' in the "Vim parity" track on 2026-10-17; the search-settings card
puts them on the shared `Registers`.

## Change

In T825's `crates/fe-vim/src/ex/set.rs`: `ignorecase`/`ic`, `smartcase`/`scs` and `incsearch`/`is` take
every boolean form T825 supports (`opt`, `noopt`, `invopt`, `opt!`, `opt?`, several in one line), with nvim's
messages (`  ignorecase`, `noignorecase`), changing `Registers`' values for every editor as nvim's global
options do (not settings.json, as T825's `:set wrap` doesn't write it either). `opt&` and value forms are
refused as T825 refuses them.

Test: a new `crates/fe-vim/tests/ex_set_search.rs` (messages and effect on a following `/` through
`HeadlessVim`), and with the oracle pattern a new corpus `crates/fe-vim/tests/corpus/ex_set_search.toml`
(`:set ic scs` then a search, `:set invic`, `:set noic` after the `set` field turned it on), recorded into
`ex_set_search.expected.json` by the ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a new
`crates/fe-vim/tests/ex_set_search_oracle.rs` modelled on `tests/line_motions_oracle.rs`.
