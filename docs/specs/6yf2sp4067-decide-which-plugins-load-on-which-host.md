---
task_uid: "01m5j50tn82nvrmp6yf2sp4067"
task: "T280"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "C1 compat::an_older_minor_loads: needing 1.0 on a 1.1 host is Ok"
    required: true
    manual: false
  - id: "C2"
    text: "C2 compat::a_newer_minor_is_refused_naming_both: needing 1.2 on a 1.1 host is NeedsNewerHost naming 1.2 and 1.1"
    required: true
    manual: false
  - id: "C3"
    text: "C3 compat::another_major_is_refused: needing 2.0 on a 1.x host is UnsupportedMajor"
    required: true
    manual: false
  - id: "C4"
    text: "C4 compat::a_deprecated_major_loads_until_its_window_ends: a test DEPRECATED entry lets the old major load up to its last host version and refuses after"
    required: true
    manual: false
  - id: "C5"
    text: "C5 compat::an_adapter_serves_a_one_zero_plugin_on_a_one_one_host: log_v1_0.wat imports kiln:extension/log@1.0.0 and runs on a test host at 1.1 through a test-only adapter"
    required: true
    manual: false
  - id: "C6"
    text: "C6 cargo nextest run -p kiln-plugin and cargo clippy -p kiln-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Decide which plugins load on which host API version and link adapters for older minors, refusing newer minors and unsupported majors (Mac T1056)

## Problem

WIT packages carry semver, the manifest's `engine` names the API a plugin needs, and the host keeps each supported older
minor's world through adapters; a major may be dropped only after a deprecation window (§7 Versioning). Nothing
yet decides which plugins load on which host.

## Change

`kiln/crates/kiln-plugin/src/compat.rs`: `HOST_API: ApiVersion` (1.0 now), `Compat::check(needed: ApiVersion) ->
Result<(), Incompatible>` (same major and needed minor not above the host's; a newer minor is `NeedsNewerHost`, another
major is `UnsupportedMajor`, naming both versions), a `DEPRECATED` table (empty now) giving a major's last supported
host version, and `compat::link(&mut Linker)` (the stub P3 calls) which registers adapters for older minors; there are
none yet, the table and a test-only adapter prove the mechanism. wasmtime's linker already matches semver-compatible
interface versions, so an adapter exists only for a breaking difference.

## Notes

Test with a test-only host at 1.1 and a `.wat` component importing `kiln:extension/log@1.0.0`.

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new files; the scope lists exact files.
