---
task_uid: "01m796pftj3vk67bfmxkwfx5rb"
task: "T593"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "the_preview_setting_applies_live and a_plugin_is_pending_until_granted assert one cache entry on every OS, with no cfg!(unix) in those asserts"
    required: true
    manual: false
  - id: "C2"
    text: "CONTRIBUTING.md no longer says Windows doesn't cache compiled plugins"
    required: true
    manual: false
---

# Plugin tests and docs: Windows caches compiled plugins now (Mac T1758)

Windows CI: `the_preview_setting_applies_live` (crates/fastedit/tests/plugins.rs:234) and
`a_plugin_is_pending_until_granted` (crates/fe-workbench/src/plugin_host_tests.rs:299) assert
`cache_entries() == usize::from(cfg!(unix))` and get 1 on Windows.

Decision (from the code): the assertions are stale, not the cache. 77be02ccd "Compile cache trusted
on Windows: private DACL and owner check (P7, W-g)" (kiln-plugin src/cache.rs: `platform::create_private_dir`,
`owned_and_unshared`) made Windows cache compiled plugins; the `cfg!(unix)` assertions and the
CONTRIBUTING.md bullet "Compiled plugins aren't cached, as nothing checks that the cache folder is the
user's alone" predate it.

Change: both assertions become `assert_eq!(.., 1)` on every platform, delete the "Windows doesn't cache"
comments, and remove that bullet from CONTRIBUTING.md's Windows section. Check the other
`cache_entries() == 0` assertions (plugin_host_tests.rs 282, 291, 629, 679) still make sense (they
are before the grant/load, so they should). If the cache itself is skipped on Windows when its
private-dir setup fails, assert on that instead of cfg: find where the entry count is 0 on Windows.

Test-only plus a doc line. Linux proves the unchanged-on-unix side; only Windows CI proves the
Windows count is 1.
