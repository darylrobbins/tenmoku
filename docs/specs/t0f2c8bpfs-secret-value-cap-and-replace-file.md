---
task_uid: "01m74cc78s9rv19ft0f2c8bpfs"
task: "T577"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "`kiln_core::platform::replace_file(from, to)` exists: a plain rename on Unix; on Windows it retries a sharing violation or access-denied for up to 1 s. The grant store (grants.rs) and the compile cache (cache.rs) write through it."
    required: true
    manual: false
  - id: "C2"
    text: "New test `replace_file_succeeds_over_a_target_held_open` in kiln/crates/kiln-core/tests/replace_file.rs: the target is held open (on Windows without delete sharing) and released after 300 ms; the replace succeeds."
    required: true
    manual: false
  - id: "C3"
    text: "New test `replace_file_gives_up_after_one_second`: a target held for the whole test returns the error within 1.5 s."
    required: true
    manual: false
  - id: "C4"
    text: "A 4 KiB secret round-trips through the platform store in kiln/crates/kiln-plugin/tests/secrets.rs (`secret_4kib_value_round_trips`). If Windows Credential Manager refuses it, one documented value cap for every OS is set in secrets.wit and enforced in src/secrets.rs, with test `secret_over_the_cap_is_refused_on_every_os`."
    required: true
    manual: false
  - id: "C5"
    text: "`GrantStore::get` reads under the lock (the unlocked read noted in section 2.2 W-i is fixed) with a test."
    required: true
    manual: false
---

# Secret value cap and replace_file retrying sharing violations (P9, W-i) (Mac T1691)

# Secret value cap and replace_file retrying sharing violations (P9, W-i)

Draft id P9 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 2.2 row W-i. Whether Credential Manager accepts 4 KiB can only be seen on Windows CI; make the cap decision explicit in the card's finish notes.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- Cap decision: Credential Manager keeps a credential blob in at most 2560 bytes, and the Windows keyring store writes the value as UTF-16, so 4 KiB would be refused there (not seen on Windows CI). One cap of 1280 UTF-8 bytes now applies on every OS (`MAX_VALUE_LEN`, documented in secrets.wit, enforced in kiln-plugin/src/secrets.rs). `secret_4kib_value_round_trips` is therefore not written; `secret_over_the_cap_is_refused_on_every_os` covers it, and a 4 KiB value is refused.
- Scope was widened (respec) to add tests/grants.rs (the C5 test) and l10n/en-US/kiln-plugin.ftl (the cap message).
- `GrantStore::get` skips the lock when the state directory doesn't exist, so a read creates nothing.
- `replace_file_gives_up_after_one_second` holds the target on Windows; on Unix, where a rename never waits, the target is a non-empty directory, so the rename fails at once.
