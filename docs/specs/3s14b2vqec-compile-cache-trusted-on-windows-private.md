---
task_uid: "01m73yn3qmt3rq7w3s14b2vqec"
task: "T570"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "On Windows `create_private_dir` sets a protected DACL (full control for the user's SID and SYSTEM, inheritance off) and `owned_and_unshared` checks the owner SID is the current user's and the DACL grants write to no one but that user, SYSTEM and Administrators."
    required: true
    manual: false
  - id: "C2"
    text: "New test `private_dir_new_dir_is_owner_only` in kiln/crates/kiln-core/tests/private_dir.rs: a freshly created dir passes `owned_and_unshared` on every OS (Unix: mode 0700 as today)."
    required: true
    manual: false
  - id: "C3"
    text: "New test `private_dir_widened_dir_is_not_trusted`: a dir widened to group/other write (Unix) or with an Everyone write ACE added (Windows) fails `owned_and_unshared`."
    required: true
    manual: false
  - id: "C4"
    text: "kiln/crates/kiln-plugin/tests/cache.rs no longer carries `#![cfg(unix)]`; its group-writable test has a DACL-widened twin on Windows; only mode checks stay cfg(unix)."
    required: true
    manual: false
  - id: "C5"
    text: "Unsafe Windows API calls are confined to platform/acl.rs; `cargo check --target x86_64-pc-windows-msvc -p kiln-core` type-checks where the build scripts allow it."
    required: true
    manual: false
---

# Compile cache trusted on Windows: private DACL and owner check (P7, W-g) (Mac T1690)

# Compile cache trusted on Windows: private DACL and owner check (P7, W-g)

Draft id P7 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 2.2 row W-g. Today every Windows launch recompiles every plugin because owned_and_unshared is always false there.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- `owned_and_unshared(&fs::Metadata, &fs::Metadata)` became `owned_and_unshared(&File, &Owner)`:
  std's Windows `Metadata` holds no owner, access list, path or handle, so the check needs the
  open file. Directories open with the new `platform::open_dir` (on Windows `READ_CONTROL` and
  backup semantics). `Owner::of(&file)` takes over from the probe file's `Metadata`; on
  Windows it holds the token's user and, if different, its default owner (`TokenOwner`), and
  fails if the probe is owned by anyone else. The default owner is accepted because an elevated
  process's files can be owned by `Administrators`, which the access-list rule trusts anyway.
  This needed `kiln/crates/kiln-plugin/src/cache.rs`, added to the scope in revision 2.
- The access-list check skips deny entries and inherit-only entries (they apply only to what is
  created inside, which is checked itself). It fails closed on a null list or any allow entry of a
  kind other than `ACCESS_ALLOWED_ACE_TYPE`. Write rights are write data/append/EA/attributes,
  delete child, `DELETE`, `WRITE_DAC`, `WRITE_OWNER`, and generic write/all.
- On Windows `create_private_dir` creates each missing directory with `CreateDirectoryW` and the
  security descriptor already in place, so a directory is never visible with the parent's
  inherited list. A directory that already exists is left as it is, as on Unix.
- `file_identity`'s `GetFileInformationByHandle` moved to `acl.rs` too, so `fs.rs` has no
  `unsafe`. `process_tree.rs`, `stdio.rs` and `keychain.rs` keep theirs (outside this scope).
- Windows tests widen access with `icacls <path> /grant *S-1-1-0:(W)` (Everyone by SID, since
  the name is translated), so the tests need no `unsafe`.
- `cargo check`/`clippy --target x86_64-pc-windows-msvc` passes for kiln-core as is. For
  kiln-plugin (whose wasmtime-internal-fiber build script compiles C) it passes with
  `CC_x86_64_pc_windows_msvc=true` and a stub `AR` that writes the archive it is asked for.
