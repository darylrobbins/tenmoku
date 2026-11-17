---
task_uid: "01m7kghgrgnpzr7n4z6fpq14dc"
task: "T409"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "update_check.rs's LATEST_RELEASE is https://api.github.com/repos/tenmoku-editor/tenmoku/releases/latest and RELEASE_PAGES https://github.com/tenmoku-editor/tenmoku/releases/; tests::update_check_home::the_update_check_asks_the_current_repository (base API only) fails at base and passes"
    required: true
    manual: false
  - id: "C2"
    text: "No darylrobbins/tenmoku or darylrobbins/fastedit URL remains outside docs/specs, docs/decisions and the test fixture string in crates/fastedit/src/grammar_install.rs (T1485 holds that file; a follow-up changes it); pack.toml's base_url and the tests that name it follow; the Velopack feed URL the release workflow bakes in is the new home (check release.yml and velopack_update.rs, and say in the hand-in whether 0.2.2's baked feed URL follows GitHub's redirect)"
    required: true
    manual: false
  - id: "C3"
    text: "README.md says releases are built for all four platforms as docs/release/platforms.md says, says 69 languages (the grammar count), and links https://tenmoku.dev (the site is live)"
    required: true
    manual: false
---

# The repository moved to tenmoku-editor/tenmoku: Check for Updates' latest-release URL and release-page prefix, pack.toml's grammars-2 base_url, the README and docs name the new home (the old API URL now answers 301, which fe-net doesn't follow, so every shipped build's check fails) (Mac T1912)
