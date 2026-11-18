---
task_uid: "01m7reb1ze9jv2zy526bwrm4t6"
task: "T1967"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "GH CI windows Test passes fe-acp::lane_dirs_windows lane_dirs_have_no_verbatim_prefix: a #![cfg(windows)] test that runs LaneDirs::resolve on a temp repo with a linked worktree (fixture paths free of \\\\?\\) and asserts with assert! (message starting \"assertion\") that common_dir, worktree_admin_dir and root_admin_dir have no \\\\?\\ prefix; it fails at 8ba6f008c on Windows"
    required: true
    manual: false
  - id: "C2"
    text: "GH CI windows Test passes the 32 fe-acp::claude_profile tests and the 8 fe-acp::launch tests failing at 8ba6f008c (listed in GH CI run 44405849017 windows Test)"
    required: true
    manual: false
  - id: "C3"
    text: "GH CI windows Test passes fe-acp preflight::tests::relative_path_entries_are_skipped"
    required: true
    manual: false
  - id: "C4"
    text: "macOS and Linux behaviour is unchanged: the fe-acp tests pass on the gate with no expectation edits other than the fixture canonicalize"
    required: true
    manual: false
---

# fe-acp on Windows: lane git dirs come back as verbatim \\?\ paths (std canonicalize in claude.rs), and the claude_profile/launch fixtures hand git a \\?\ worktree path

# fe-acp: verbatim \\?\ paths on Windows

GH CI 44405849017 on 8ba6f008c, windows Test: 40 fe-acp integration tests panic at their fixture (claude_profile.rs:50, launch.rs:42):

    git ["worktree", "add", "-q", "-b", "lane", "\\\\?\\C:\\Users\\runneradmin\\AppData\\Local\\Temp\\.tmpNuZomg\\lane"]: fatal: could not create leading directories of '//?/C:/Users/.../lane/.git': Invalid argument

## Cause
- Tests: `Fixture::new` in tests/claude_profile.rs:68 and tests/launch.rs:60 builds `base` with `std::fs::canonicalize`, which on Windows always gives a verbatim `\\?\` path. Git for Windows can't make a worktree there.
- Production: `rev_parse` (claude.rs:371) and `resolve` (claude.rs:537) also use `std::fs::canonicalize`. So `LaneDirs` (common_dir, worktree_admin_dir, root_admin_dir) and the profile's deny/denyWrite paths are `\\?\` paths on Windows. Claude Code then gets paths it won't match against the ones it writes, and once the fixtures are fixed these tests would still fail by comparing plain paths with verbatim ones.
- preflight_tests.rs:138 `pathdiff` does `target.strip_prefix("/")` (StripPrefixError on `C:\...`). On the runner the cwd (D:\a\...) and the temp dir (C:\...) are on different drives, so no relative path between them exists.

## Fix
- Add `dunce` (a workspace dependency, already used by fe-git and fe-workbench) to fe-acp `[dependencies]`. Use `dunce::canonicalize` in claude.rs `rev_parse` and in `resolve`'s per-component canonicalize, keeping its NotFound handling. Don't add a kiln-core dependency.
- Fixtures: use `dunce::canonicalize` in both test files.
- preflight test: make the relative entry portable. Strip the root/prefix components rather than "/", and on Windows make the dir under the cwd's drive (e.g. `tempfile::tempdir_in` a folder under `CARGO_MANIFEST_DIR`/target) so a relative path exists.
- New tests/lane_dirs_windows.rs (`#![cfg(windows)]`) is the regression test. It can't fail at the Mac base, so it isn't a `--regression-test`; evidence comes from GH CI windows after landing.
