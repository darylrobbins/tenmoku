---
task_uid: "01m6rgq4jefvj96mak7jr10zxr"
task: "T1389"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "update_check_tests::tenmoku_release_page_is_opened passes: a v0.2.0 reply whose html_url is https://github.com/darylrobbins/tenmoku/releases/tag/v0.2.0, run as 0.1.0, is reported and Download opens that URL"
    required: true
    manual: false
  - id: "C2"
    text: "LATEST_RELEASE is https://api.github.com/repos/darylrobbins/tenmoku/releases/latest and RELEASE_PAGES is https://github.com/darylrobbins/tenmoku/releases/; rg 'darylrobbins/fastedit' over crates/ and extensions/ finds nothing"
    required: true
    manual: false
  - id: "C3"
    text: "extensions/grammars/pack.toml base_url is https://github.com/darylrobbins/tenmoku/releases/download/grammars-1 with every size and sha256 unchanged"
    required: true
    manual: false
---

# Check for Updates fails after the repo rename: point update_check and the grammar pack at darylrobbins/tenmoku

## Problem

The GitHub repository was renamed from darylrobbins/fastedit to darylrobbins/tenmoku (a push on 2026-11-06 answered "This repository moved"). The old API URL now answers `301` (checked: `https://api.github.com/repos/darylrobbins/fastedit/releases/latest` -> 301 to `/repositories/1403404057/releases/latest`; the tenmoku URL answers 200). `fe-net` follows no redirects, so Check for Updates (`crates/fe-workbench/src/update_check.rs`, `LATEST_RELEASE`) fails on every check. `RELEASE_PAGES` also names the old repo, so a release page GitHub now returns (`https://github.com/darylrobbins/tenmoku/releases/tag/...`) would be refused even once the fetch succeeds.

## Fix

- `crates/fe-workbench/src/update_check.rs`: `LATEST_RELEASE` -> `https://api.github.com/repos/darylrobbins/tenmoku/releases/latest`, `RELEASE_PAGES` -> `https://github.com/darylrobbins/tenmoku/releases/`. Don't add redirect following.
- `crates/fe-workbench/src/update_check_tests.rs`, `crates/fe-workbench/src/update_hook_tests.rs`: the fixtures' `html_url`s and `PAGE` name the tenmoku repo. Add the regression test `update_check_tests::tenmoku_release_page_is_opened` to the existing `update_check_tests.rs`: a fake reply for v0.2.0 whose `html_url` is `https://github.com/darylrobbins/tenmoku/releases/tag/v0.2.0`, run as 0.1.0, is reported and its Download action opens that URL (at base the page is refused).
- `extensions/grammars/pack.toml`: `base_url` -> `https://github.com/darylrobbins/tenmoku/releases/download/grammars-1` (grammar fetches follow redirects, but don't rely on the old name). Leave every `size`/`sha256` as is.
- Any non-docs occurrence of `darylrobbins/fastedit` in these files; leave `docs/**` history alone.
