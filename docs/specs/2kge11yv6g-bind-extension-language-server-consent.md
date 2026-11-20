---
task_uid: "01m7td2svc8bwy712kge11yv6g"
task: "T1978"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Invocation carries a package_hash over the canonical extension dir (sorted relative paths + contents, no symlinks followed out of it); stored grants without it fail closed and re-ask."
    required: true
    manual: false
  - id: "C2"
    text: "A changed module imported by the named script re-asks consent with script and arguments unchanged; an unchanged package does not; an added or removed file re-asks. This is shown through the real extension watcher and reload path (crates/fastedit/tests/extension_consent.rs), not a direct set_extension_servers call. package_files has a traversal cap; an over-cap package is incomplete (never granted), with cap-specific tests for the file-count and size limits. A package with a file over PACKAGE_MAX_FILE_BYTES (256 MiB) is incomplete at consent and never granted, and package_files lists such files (no size filter), so adding or removing one is seen; kiln may still read such a file whole while hashing it before consent refuses (known limit). Bounding kiln's hash reads and fingerprinting oversized files by size and mtime are follow-up T2026 (T1978 F8/F9), as are directory-symlink retargets, empty-directory changes and moving the walk off the UI thread; running granted servers from package snapshots is follow-up T2024."
    required: true
    manual: false
  - id: "C3"
    text: "Tests in new crates/fe-workbench/src/lsp_consent_package_tests.rs cover the cases above; existing lsp_consent tests pass."
    required: true
    manual: false
  - id: "C4"
    text: "Decision file vmtnmkay62 updated consistently (consent text, risk table, node_modules/size caps choice) with a revision-disclosure line; hashing stays off the main thread."
    required: true
    manual: false
---

# Bind extension language-server consent to the whole installed package, not just named scripts (review follow-up of T839 F5)

Review follow-up of T839 F5 (high, overridden at approval): extension language-server launch consent binds only the script files named in the arguments. `Invocation::of` (crates/fe-workbench/src/lsp_consent.rs) fills `script_hashes` per named file inside the extension directory. So `node server.js` keeps the same invocation and server.js hash while an update replaces a module server.js imports, and the changed code runs with no renewed consent. Decision record: docs/decisions/vmtnmkay62-extensions-and-plugins.md (line ~80, plus the risk table and revision disclosure). Checked main: nothing later (T1038, T1083, T1645, T1701) widened this; T1701 does not touch lsp_consent.rs.

Do: bind consent to the whole installed extension package. Add a `package_hash` to `Invocation` (serde default so stored grants fail closed): a deterministic SHA-256 over the sorted relative paths and contents of every file under the canonical extension dir, not following symlinks out of it, skipping nothing that could execute (decide on node_modules and caps on size/file count, and record the choice). Any change to it counts as a changed specification: the running server keeps the old files, the prompt asks again, and the new execution starts only after acceptance. Keep script_hashes for the prompt's changed-script warning. Update the decision file's text at ~80, ~386 and ~468 and the risk table consistently, with a revision-disclosure line.

Tests (new file crates/fe-workbench/src/lsp_consent_package_tests.rs, declared from lib.rs next to the other lsp_consent test modules): a changed module imported by server.js re-asks consent while the named script and arguments are unchanged; an unchanged package does not; a new or removed file re-asks; a symlink escaping the dir is not followed; a stored grant without package_hash re-asks.

Hashing reads files, so keep it off the main thread as `Invocation::of` already requires.
