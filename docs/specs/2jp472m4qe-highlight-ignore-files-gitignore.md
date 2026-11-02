---
task_uid: "01m6c8gvafxb64y02jp472m4qe"
task: "T1282"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "vendor/tree-sitter-gitignore holds the upstream MIT grammar at a pinned commit with its LICENSE and a README saying how to update it; no konomanoasa-* crate in Cargo.lock"
    required: true
    manual: false
  - id: "C2"
    text: "A .gitignore, .dockerignore, .npmignore, .prettierignore, .eslintignore, .ignore, .rgignore and .fdignore each detect as the gitignore language (fe-syntax test)"
    required: true
    manual: false
  - id: "C3"
    text: "crates/fe-syntax/tests/golden/gitignore.gitignore covers comments, negation, wildcards, **, directory slashes, bracket expressions and escapes, and its .golden shows each highlighted"
    required: true
    manual: false
  - id: "C4"
    text: "THIRD_PARTY.md lists the grammar; cargo deny check bans licenses sources passes, and cargo deny check advisories reports nothing for tree-sitter-gitignore (other advisories present at the base are out of scope: T1301 covers them); cargo nextest run -p fe-syntax passes"
    required: true
    manual: false
---

# Highlight ignore files (.gitignore, .dockerignore, .npmignore, .prettierignore, .ignore, .rgignore and the like) with a vendored upstream tree-sitter-gitignore grammar

Ignore files open as plain text today. Add gitignore-syntax highlighting as a built-in grammar.

Grammar: crates.io has no upstream tree-sitter-gitignore crate (only a third-party republish,
konomanoasa-tree-sitter-gitignore, which we don't take). Vendor the upstream grammar instead,
shunsambongi/tree-sitter-gitignore (MIT), as vendor/tree-sitter-gitignore: the generated
src/parser.c (and scanner if any), queries/highlights.scm, LICENSE, and a minimal Cargo crate with a
cc build.rs and a LANGUAGE/HIGHLIGHTS_QUERY like the crates.io grammars expose, the way
vendor/tree-sitter-yaml sits beside its patch. Pin the upstream commit and say how to update it in a
vendor/tree-sitter-gitignore/README.md. Check its ABI works with tree-sitter 0.27; regenerate with the
tree-sitter CLI only if it must, and say so.

Files that use the syntax (fe-syntax language.rs `file_names` / path rules): .gitignore, .dockerignore,
.npmignore, .prettierignore, .eslintignore, .stylelintignore, .vscodeignore, .helmignore,
.gcloudignore, .ignore, .rgignore, .fdignore, and .git/info/exclude and git's global
~/.config/git/ignore (path-based, if language detection can match a path tail; else note it).
Not .hgignore (different syntax).

Highlight captures map onto our capture names (comment, negation `!`, wildcards, directory slash,
bracket expressions, escapes); adapt the upstream query in crates/fe-syntax/queries/gitignore.scm if
its names differ from ours. Add a golden like the other grammars. Record the grammar in THIRD_PARTY.md.
The grammar is tiny; note the release binary size change in the decision file.
