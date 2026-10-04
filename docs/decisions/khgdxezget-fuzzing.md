# Fuzzing the grammars and fe-buffer: first runs

Status: **run**. No memory errors or aborts. One **hang**: a TSX incremental
reparse that never finishes and keeps allocating, which in the editor would
pin a core and exhaust memory. Six grammars' incremental reparses disagree
with a fresh parse. All are upstream bugs in tree-sitter or its grammars
and get their own bugfix cards; none is fixed here. Run 2026-10-03. Source
card T12.

## Setup

- cargo-fuzz 0.13.2, rustc 1.99.0-nightly (2026-07-16), Apple clang 21.0.0,
  M-series Mac with 14 cores.
- `-s address` for the Rust code. `CFLAGS="-fsanitize=address,fuzzer-no-link
  -mllvm -asan-guard-against-version-mismatch=0"` for the grammars' C code and
  tree-sitter's runtime, so ASan and libFuzzer's coverage reach them too.
  Without these flags, syntax_edits sees only the Rust (see fuzz/README.md).
  With them it reached 25–28k coverage edges, against about 2k for
  buffer_edits.
- The grammar versions match the root `Cargo.lock`: tree-sitter 0.27.0,
  tree-sitter-bash 0.25.1, tree-sitter-md 0.5.3, tree-sitter-typescript
  0.23.2, tree-sitter-java 0.23.5, tree-sitter-python 0.25.0,
  tree-sitter-cpp 0.23.4 and the others. Every finding below was reproduced
  against exactly these versions.
- YAML is the vendored `vendor/tree-sitter-yaml` the editor ships. The root
  workspace's `[patch.crates-io]` doesn't reach `fuzz/`, so `fuzz/Cargo.toml`
  repeats it. Runs before the last one used registry 0.7.2, without the
  vendored scanner fix.
- syntax_edits builds fe-syntax with its `no-query-budget` feature, so
  highlight queries always run to completion, as in fe-syntax's own tests.

## Runs

| Harness | Target | Options | Time | Executions | Coverage (edges / features) | Crashes |
|---|---|---|---|---|---|---|
| final | buffer_edits | default | 601 s | 1,710,874 | 1,993 / 13,234 | 0 |
| first | syntax_edits | default (inputs ≤ 38 bytes) | 601 s | 1,667,645 | 24,581 / 105,147 | 0 |
| first | syntax_edits | `-len_control=0 -max_len=4096` | ~110 s | 274,742 | 25,188 / 111,339 | 1 (stopped) |
| first | syntax_edits | same, plus `-fork=4 -ignore_crashes=1` | 607 s | 1,563,491 | 28,495 / 138,268 | 53 |
| final | syntax_edits | same as above | 604 s | 699,036 | 29,526 / 147,978 | 16, plus 1 out-of-memory |
| final, vendored YAML | syntax_edits | same as above | 614 s | 755,653 | 30,516 / 157,584 | 13, plus 1 out-of-memory |
| final, all 22 languages | syntax_edits | same as above | 605 s | 253,365 | 33,669 / 168,498 | 8 |
| final, all 27 languages | syntax_edits | same as above | 620 s | 378,752 | 40,030 / 199,471 | 8 in its log, 11 artifacts |

The final run was at commit f2ec93a, the vendored-YAML run at 13f6e28, and
the 22-language run at aaa0333, after rebasing onto main's six new grammars
(Ruby, Lua, PHP, C#, Scala, XML, from T19). The 27-language run was at
f1fe5fd, after rebasing again onto T20's five (Swift, Kotlin, SQL,
Dockerfile, Make). The vendored-YAML run found nothing in YAML, and neither
later run found anything in the eleven new grammars.
The 22-language run's rate is lower because the gate's fast stages ran
alongside it. Its run rate is lower than the first
harness's because highlight queries now run unbudgeted, and inputs reach
4 KB. buffer_edits hasn't changed since its run. None of the runs hit an
ASan report or an abort.

**The first syntax_edits harness had a flawed highlight comparison.**
Outside fe-syntax's own tests, `Syntax::highlights` stops after 2 ms
(`QUERY_BUDGET`). A host query that runs out of time is still cached as
complete, so from outside fe-syntax a cut-short result can't be told from a
whole one. Under load, which included my own triage replays during the fork
run, truncated highlights made 43 of the fork run's 53 crashes. Only 10
reproduced alone: 1 Bash and 9 Markdown. A stopgap that compared 256-byte
chunks with retries was wrong too. It could pass on two equally truncated
results, and its run of chunk queries evicted the incremental injection
layers before they were compared (`Layers` keeps those near the 8 most
recent ranges).

The final harness enables fe-syntax's `no-query-budget` feature, which only
fuzz/ uses, and compares one whole-file highlight per side. It also rechecks
`failed()` after highlighting, since highlighting can panic. Replaying all
53 crash inputs three times with 14 running at once gave the 3 Bash and 27
Markdown failures, every time, and nothing else.

### Every crash, minimized

Every artifact from the fork runs was replayed alone with the final harness.
Of the first fork run's 53, the 10 that reproduce are counted below; the
other 43 were budget truncation. The four final runs' 50 artifacts all
reproduce. Each was minimized with libFuzzer (`-minimize_crash=1`,
3,000 runs), checking that the language and kind of failure held. `tmin`
couldn't shrink the two TSX hangs, so I minimized those with a greedy
character-deletion search, using tree-sitter alone, and hand-encoded them as
fuzz inputs.

To pin each cause down rather than infer it from the symptom, each
minimized input was replayed with tree-sitter alone: the same edits, one
covering `InputEdit` per batch, comparing incremental and fresh parses. All
61 reproduce without fastedit. For Markdown, the replay layers the inline
grammar as fe-syntax does: one layer per `inline` node, over its bytes
minus its named children. Every Markdown case differs that way. 9 contain
NUL or control characters that change the block structure, so they don't
differ when the whole text is parsed as one inline layer. All 61 replay as
listed with the final harness, after each rebase too.

| Regression | Source artifact | Bytes | Reproduces with tree-sitter and |
|---|---|---|---|
| `bash-heredoc-374eec01` | final run, `crash-374eec01…` | 44 → 21 | bash grammar |
| `bash-heredoc-8d746dea` | fork run 1, `crash-8d746dea…` | 64 → 19 | bash grammar |
| `markdown-inline-1e9685c3` | final run, `crash-1e9685c3…` | 202 → 29 | inline grammar, whole text |
| `markdown-inline-22fb92b5` | final run, `crash-22fb92b5…` | 106 → 25 | inline grammar, whole text |
| `markdown-inline-38651f2b` | final run, `crash-38651f2b…` | 206 → 19 | inline grammar, layered ranges only |
| `markdown-inline-46c6a7b2` | final run, `crash-46c6a7b2…` | 141 → 41 | inline grammar, whole text |
| `markdown-inline-5e60dea1` | final run, `crash-5e60dea1…` | 510 → 28 | inline grammar, whole text |
| `markdown-inline-6785c9e3` | final run, `crash-6785c9e3…` | 114 → 85 | inline grammar, whole text |
| `markdown-inline-7dd0cecd` | final run, `crash-7dd0cecd…` | 325 → 34 | inline grammar, whole text |
| `markdown-inline-abde4377` | final run, `crash-abde4377…` | 150 → 26 | inline grammar, whole text |
| `markdown-inline-b90400cd` | final run, `crash-b90400cd…` | 269 → 31 | inline grammar, whole text |
| `markdown-inline-da150089` | final run, `crash-da150089…` | 189 → 30 | inline grammar, layered ranges only |
| `markdown-inline-db6a7920` | final run, `crash-db6a7920…` | 271 → 27 | inline grammar, whole text |
| `markdown-inline-e23f24a5` | final run, `crash-e23f24a5…` | 69 → 21 | inline grammar, whole text |
| `markdown-inline-e2a78be6` | final run, `crash-e2a78be6…` | 127 → 22 | inline grammar, whole text |
| `markdown-inline-1bcf6d3d` | fork run 1, `crash-1bcf6d3d…` | 277 → 33 | inline grammar, whole text |
| `markdown-inline-389dcaa7` | fork run 1, `crash-389dcaa7…` | 131 → 28 | inline grammar, layered ranges only |
| `markdown-inline-4838e049` | fork run 1, `crash-4838e049…` | 156 → 35 | inline grammar, whole text |
| `markdown-inline-strikethrough-incremental` | fork run 1, `crash-4ad4ccc3…` | 44 → 21 | inline grammar, whole text |
| `markdown-inline-53d5ff20` | fork run 1, `crash-53d5ff20…` | 78 → 30 | inline grammar, layered ranges only |
| `markdown-inline-6ac4a4ed` | fork run 1, `crash-6ac4a4ed…` | 160 → 57 | inline grammar, whole text |
| `markdown-inline-ab5bca43` | fork run 1, `crash-ab5bca43…` | 122 → 37 | inline grammar, layered ranges only |
| `markdown-inline-c3ff7a22` | fork run 1, `crash-c3ff7a22…` | 232 → 18 | inline grammar, whole text |
| `markdown-inline-da4aecdb` | fork run 1, `crash-da4aecdb…` | 408 → 41 | inline grammar, whole text |
| `bash-heredoc-0a96463b` | YAML-patched run, `crash-0a96463b…` | 121 → 67 | bash grammar |
| `bash-heredoc-a2a6852d` | YAML-patched run, `crash-a2a6852d…` | 215 → 19 | bash grammar |
| `bash-heredoc-d2f53688` | YAML-patched run, `crash-d2f53688…` | 592 → 32 | bash grammar |
| `cpp-error-recovery-87a5be26` | YAML-patched run, `crash-87a5be26…` | 2303 → 272 | cpp grammar |
| `markdown-inline-504b6c6f` | YAML-patched run, `crash-504b6c6f…` | 282 → 32 | inline grammar, whole text |
| `markdown-inline-69859c09` | YAML-patched run, `crash-69859c09…` | 507 → 31 | inline grammar, whole text |
| `markdown-inline-6aab62f9` | YAML-patched run, `crash-6aab62f9…` | 1068 → 27 | inline grammar, layered ranges only |
| `markdown-inline-76221bfd` | YAML-patched run, `crash-76221bfd…` | 269 → 41 | inline grammar, whole text |
| `markdown-inline-7fc273cb` | YAML-patched run, `crash-7fc273cb…` | 76 → 26 | inline grammar, whole text |
| `markdown-inline-a0275e11` | YAML-patched run, `crash-a0275e11…` | 1076 → 21 | inline grammar, whole text |
| `markdown-inline-b688980c` | YAML-patched run, `crash-b688980c…` | 263 → 20 | inline grammar, whole text |
| `markdown-inline-d3002708` | YAML-patched run, `crash-d3002708…` | 1389 → 32 | inline grammar, whole text |
| `markdown-inline-e08d239f` | YAML-patched run, `crash-e08d239f…` | 338 → 19 | inline grammar, whole text |
| `bash-heredoc-in-test-incremental` | long run, `crash-7227baf7…` | 19 (tmin) | bash grammar |
| `tsx-incremental-parse-hang` | final run, `oom-601f219c…` | 370 → 36 (hand-encoded) | typescript (TSX) grammar, hangs |
| `tsx-incremental-parse-hang-from-empty` | YAML-patched run, `oom-4eeeb7f5…` | 46 (hand-encoded) | typescript (TSX) grammar, hangs |
| `java-error-recovery-incremental` | final run, `crash-414ffe13…` | 450 → 27 | java grammar |
| `python-error-recovery-incremental` | final run, `crash-8b15e3d9…` | 1043 → 145 | python grammar |
| `javascript-comment-placement-919dc7ea` | 22-language run, `crash-919dc7ea…` | 581 → 33 | javascript grammar |
| `markdown-inline-03e6892d` | 22-language run, `crash-03e6892d…` | 244 → 29 | inline grammar, whole text |
| `markdown-inline-1c420011` | 22-language run, `crash-1c420011…` | 1262 → 35 | inline grammar, whole text |
| `markdown-inline-2095a7aa` | 22-language run, `crash-2095a7aa…` | 67 → 48 | inline grammar, layered ranges only |
| `markdown-inline-34a63a1a` | 22-language run, `crash-34a63a1a…` | 1626 → 21 | inline grammar, whole text |
| `markdown-inline-3ff9fc65` | 22-language run, `crash-3ff9fc65…` | 508 → 21 | inline grammar, whole text |
| `markdown-inline-6f131f43` | 22-language run, `crash-6f131f43…` | 273 → 28 | inline grammar, whole text |
| `markdown-inline-87e85f34` | 22-language run, `crash-87e85f34…` | 217 → 45 | inline grammar, layered ranges only |
| `markdown-inline-0a229ec1` | 27-language run, `crash-0a229ec1…` | 1517 → 43 | inline grammar, layered ranges only |
| `markdown-inline-172e910f` | 27-language run, `crash-172e910f…` | 1083 → 25 | inline grammar, whole text |
| `markdown-inline-1e1068b7` | 27-language run, `crash-1e1068b7…` | 234 → 21 | inline grammar, whole text |
| `markdown-inline-4da0a191` | 27-language run, `crash-4da0a191…` | 1257 → 48 | inline grammar, whole text |
| `markdown-inline-81dff7f0` | 27-language run, `crash-81dff7f0…` | 87 → 31 | inline grammar, whole text |
| `markdown-inline-87af5e0e` | 27-language run, `crash-87af5e0e…` | 416 → 31 | inline grammar, whole text |
| `markdown-inline-894ec676` | 27-language run, `crash-894ec676…` | 627 → 43 | inline grammar, whole text |
| `markdown-inline-b6b9fb30` | 27-language run, `crash-b6b9fb30…` | 79 → 26 | inline grammar, whole text |
| `markdown-inline-ce56179d` | 27-language run, `crash-ce56179d…` | 1993 → 38 | inline grammar, whole text |
| `markdown-inline-da067df9` | 27-language run, `crash-da067df9…` | 542 → 32 | inline grammar, whole text |
| `markdown-inline-e3c400e8` | 27-language run, `crash-e3c400e8…` | 508 → 34 | inline grammar, whole text |

Bash, Markdown and Java/Python/C++ regressions each share a cause within each
grammar as far as the evidence goes: the same grammar, the same symptom, and
each reproducing without fastedit. Proving that two inputs hit the same line
of a scanner would need a debugger session per input, which I didn't do.

## Findings

### 1. tree-sitter-bash: a heredoc start inside `[` at end of file

`fuzz/regressions/syntax_edits/bash-heredoc-in-test-incremental`

This reproduces with tree-sitter and tree-sitter-bash alone, without
fastedit or a sanitizer. Not a memory error.

Draft upstream issue (tree-sitter/tree-sitter-bash):

> **Incremental reparse differs from a fresh parse for `[ ab<<EOF` at end of input**
>
> tree-sitter 0.27.0, tree-sitter-bash 0.25.1 (also 0.25.0). Parse `[ ab<<EOF` (no
> trailing newline), then insert `c` at byte 3 (`InputEdit` start 3, old
> end 3, new end 4) and reparse with the old tree:
>
> ```
> before:      (program (ERROR (binary_expression left: (word) right: (word))))
> incremental: (program (test_command (binary_expression left: (word) right: (word)) (MISSING "]")))
> fresh:       (program (ERROR (binary_expression left: (word) right: (word))))
> ```
>
> Any insertion inside either word does it (`[ a<<J` + `x` at 2 or 3). With a
> trailing newline, or with `[[`, `<` or `< J`, both parses agree. The
> heredoc-start state seems to survive into the reused subtree.

Five more Bash regressions (`bash-heredoc-*`) came from later runs. Four
are the same `[ …<<` shape. `bash-heredoc-d2f53688` shows that the `[` isn't
needed: in `.A\u{FFFD}\u{1}\u{FFFD}\u{b}<<\u{FFFD}\u{7}$`, deleting a
trailing U+FFFD gives `(program (ERROR (command …)) (command …))`
incrementally but `(program (command …) (ERROR (heredoc_start)))` fresh.
So the common factor is an unterminated heredoc start at end of input;
the upstream issue should include both.

Impact on fastedit: a half-typed `[ x<<EOF` at the end of a shell file can
highlight differently from a fresh open until a later edit reparses that
part. Highlighting is wrong only cosmetically, and nothing crashes.

Proposed card (bugfix, `fe-syntax`): "Bash: incremental reparse disagrees
with a fresh parse for `[ ab<<EOF` at end of file". File the upstream issue.
Then either pin a grammar release with the fix, or add the minimized input
as an fe-syntax test, `#[ignore]`d until upstream fixes it. Recovering
locally, say by reparsing from scratch whenever the tree has an error near
the edit, would cost every keystroke in a broken file, so it isn't
proposed.

### 2. tree-sitter-md (inline): strikethrough lost on incremental reparse

`fuzz/regressions/syntax_edits/markdown-inline-strikethrough-incremental`

Markdown's block tree matches. The difference is in the injected
`markdown_inline` layer, which fe-syntax keeps and edits between
highlights. It reproduces with the inline grammar alone, so fe-syntax's
layer handling isn't at fault. All 9 Markdown crash inputs fail the same way
(highlights differ, trees equal); I minimized and analyzed one of them.

Draft upstream issue (tree-sitter-grammars/tree-sitter-markdown):

> **Inline grammar: incremental reparse drops a strikethrough a fresh parse finds**
>
> tree-sitter 0.27.0, tree-sitter-md 0.5.3, `INLINE_LANGUAGE`. Parse
> `<~<1/1~1`, replace the last byte with `/` (`InputEdit` 7..8 → 7..8), and
> reparse with the old tree:
>
> ```
> before:      (inline (strikethrough (emphasis_delimiter) (emphasis_delimiter)))
> incremental: (inline)
> fresh:       (inline (strikethrough (emphasis_delimiter) (emphasis_delimiter)))
> ```
>
> The opposite also happens: from `<~<<~1`, editing either end so a fresh
> parse finds the strikethrough, the incremental parse still has none.
> Without the leading `<` (`~a~1`, `x~a~1`) both parses agree.

Impact on fastedit: strikethrough highlighting near a `<` can go stale
after an edit, until something forces a fresh parse of that layer.

Proposed card (bugfix, `fe-syntax`): "Markdown inline: incremental reparse
drops strikethrough after `<`". File upstream. Inline layers are small, so a
cheap local fix may exist: parse injected inline layers from scratch rather
than incrementally. Measure that before choosing it.

### 3. tree-sitter (TSX): an incremental reparse that never finishes

`fuzz/regressions/syntax_edits/tsx-incremental-parse-hang` and
`tsx-incremental-parse-hang-from-empty`

The second run found it again from a different starting point: an empty
document, with `(\0[/(\0[/]]]]]]\u{FFFD}#` inserted. A fresh parse of the
result takes 11 ms, and the incremental one doesn't finish.

The first is a 36-byte input, hand-encoded: language `tsx`, text `` x)3,`x ``,
and one batch inserting `x""[[)}` at 0. `cargo fuzz tmin` couldn't shrink
the 370-byte original much, and once it drifted to a different crash. The
original input hit libFuzzer's 2 GB RSS limit. Replayed, it was still in
`ts_parser_parse` at 62 s with 1.15 GB resident. It reproduces in a release
build with tree-sitter and tree-sitter-typescript alone, with no sanitizer.
A run without a limit reached 4.6 GB before I killed it.

Draft upstream issue (tree-sitter/tree-sitter, cc tree-sitter-typescript):

> **Incremental reparse never terminates (memory grows without bound) for a 13-byte TSX edit**
>
> tree-sitter 0.27.0, tree-sitter-typescript 0.23.2, `LANGUAGE_TSX`. Parse
> `` x)3,`x ``. Then insert `x""[[)}` at byte 0 (`InputEdit` start 0, old
> end 0, new end 7), and reparse `` x""[[)}x)3,`x `` with the old tree. The
> parse doesn't return. With a progress callback it's still going after 30
> s, and without one its RSS passes 4 GB. A fresh parse of the same text
> takes microseconds, although it nests `non_null_expression` very deeply.
> The same happens with `é` or U+FFFD in place of `x`. Without the leading
> `x` in the insertion, both parses agree at once.

Impact on fastedit: **high**. `fe-gpui/src/view.rs` runs
`ParseJob::run()` on a background thread with no time limit. A TSX
buffer edited into this state pins a core and allocates until the editor
is killed. The foreground `parse_within` path has a budget, so the UI keeps
running until memory runs out.

Proposed cards:

- Bugfix, `fe-syntax` and `fe-gpui`: "Bound background parses so a
  runaway reparse can't hang or exhaust memory". Give `ParseJob::run` a
  cancellation or time cap (tree-sitter's progress callback). On overrun,
  retry once from scratch and then fall back to plain text, as `failed()`
  does. Use the regression input as the test.
- Upstream: file the issue above, then pin or patch once it's fixed.

### 4. tree-sitter error recovery: Java, Python and C++ on garbage input

`fuzz/regressions/syntax_edits/java-error-recovery-incremental`,
`fuzz/regressions/syntax_edits/python-error-recovery-incremental`

Also `cpp-error-recovery-87a5be26`: five edits through U+FFFD and dashes,
after which the incremental and fresh trees nest an ERROR region's
`update_expression`/`unary_expression` chain in a different order. All three
reproduce with tree-sitter alone (tree-sitter-java 0.23.5,
tree-sitter-python 0.25.0, tree-sitter-cpp 0.23.4). Java: start from empty, insert
`@\u{FFFD}g\u{FFFD}g==`, then delete the `@`. The incremental tree has
`name: (identifier)` inside the ERROR node, while the fresh one has a bare
`(identifier)`. Python: three edits over control characters and U+FFFD
leave a differently shaped ERROR region. Only text that is already invalid
is affected, and only inside ERROR nodes. This looks like tree-sitter's core
error recovery reusing subtrees, not either grammar.

Draft upstream issue (tree-sitter/tree-sitter):

> **Incremental reparse labels a field inside an ERROR node that a fresh parse doesn't**
>
> tree-sitter 0.27.0, tree-sitter-java 0.23.5. Parse an empty string, insert
> `@\u{FFFD}g\u{FFFD}g==` and reparse, then delete the leading `@` and
> reparse incrementally. Result: `(program (ERROR (UNEXPECTED 65533)
> (type_identifier) (ERROR (UNEXPECTED 65533)) name: (identifier)))`. A
> fresh parse gives the same tree without the `name:` field. A Python case
> shows the same kind of difference in an ERROR region.

Impact on fastedit: low. Highlighting differs only inside text that's
already a syntax error. Proposed card: none of our own. Add this to the
upstream issue list, and have the fuzz README treat ERROR-only tree
differences as known if they keep recurring.

### 5. tree-sitter (JavaScript): a trailing comment moves on incremental reparse

`fuzz/regressions/syntax_edits/javascript-comment-placement-919dc7ea`

Unlike finding 4, this is on text without errors. tree-sitter 0.27.0 with
tree-sitter-javascript 0.25.0: parse `{\n\n\n\n/\u{1}/,A///\u{1}`, delete
the leading `{\n\n` and reparse incrementally:

```
incremental: (program (expression_statement (sequence_expression (regex …) (identifier))) (comment))
fresh:       (program (expression_statement (sequence_expression (regex …) (identifier)) (comment)))
```

The `//` comment, an extra, ends up outside the statement incrementally and
inside it fresh. Impact on fastedit: low. The comment is highlighted the
same in either place, and only a tree-shape consumer (folding, structural
selection) would see the difference. Proposed card: none of our own. Add it
to the tree-sitter issue alongside finding 4, since reused subtrees and
extras placement are core behaviour.

## Not done

- buffer_edits found nothing. A refused undo or redo never changed the
  text, and with a single origin nothing was refused.
- Fuzzing stays by hand, as the card says. An on-demand CI job like `soak`
  remains an open question for a person to decide.
