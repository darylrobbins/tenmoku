---
task_uid: "01m79cbbwjs1mcr16y1b9hfwa1"
task: "T337"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-git --test blob_bom passes: head_text_drops_a_utf8_bom, index_text_drops_a_utf8_bom, a_blob_without_a_bom_is_unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-git passes"
    required: true
    manual: false
---

# Strip a UTF-8 BOM from the git diff base so a BOM file does not show its first line modified (Mac T1763)

# Strip a UTF-8 BOM from the git diff base so a BOM file doesn't show its first line modified

Opening a C# file (Visual Studio writes UTF-8 with a BOM) shows its first line marked modified in the git
change bar though the file is unchanged. `Buffer` drops the BOM on load (`fe-text/src/encoding.rs`
`decode_in`, `FileEncoding.bom`), but the diff base keeps it: `fe-git` `commit_text` (`lib.rs`, behind
`head_text` and blame) and `Repo::index_text` (`index.rs`) decode the blob with `String::from_utf8_lossy`, so
a leading `EF BB BF` becomes U+FEFF at the start of row 0 of the base. `fe-workbench/src/git.rs` `base()` hands
that string to `set_diff_base`, and row 0 differs.

**Fix.** In `fe-git`, strip one leading UTF-8 BOM from the blob bytes before decoding, in both `commit_text`
and `index_text` (share one small helper in `lib.rs`; `index.rs` already uses `crate::` items). The buffer
holds no BOM, so the base must hold none either; blame of a BOM file also gets row 0 right.

Known limit, leave it: a UTF-16 blob is not valid UTF-8 and the base is lossy UTF-8 anyway, so a UTF-16
file still shows a garbage base (the buffer decodes UTF-16, the base doesn't). Say so in the doc comments of
both functions; no code for it.

**Tests (base API only).** New file `crates/fe-git/tests/blob_bom.rs` (fe-git tests are one binary per file;
nothing to declare). Copy the `git(dir, args)` helper from `status_split_intent_to_add.rs`. Build a temp
repo, commit `\u{FEFF}x\ny\n` as `a.cs` (write the bytes `EF BB BF 78 0A 79 0A`), then:
`Repo::discover(root)` -> `head_text(&root.join("a.cs"))` is `Some("x\ny\n")`; `git add` a changed copy and
`index_text` returns the text without U+FEFF; a file with no BOM comes back byte-equal; a file whose text
starts with a BOM-looking non-BOM char is untouched. Use `assert_eq!` on the returned `Option<String>`
(do not put a message first), so the base run fails on an assertion.

No workbench-level test: `git.rs` `base()` passes the string through unchanged, and the fe-git test pins the
cause. Linux-OK.
