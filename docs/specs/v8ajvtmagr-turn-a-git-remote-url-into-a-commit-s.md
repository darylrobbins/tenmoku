---
task_uid: "01m62s3d4ywb0wg9v8ajvtmagr"
task: "T254"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-git test remote_link::github_forms: https://github.com/o/r.git, https://github.com/o/r, ssh://git@github.com/o/r.git and git@github.com:o/r.git each give commit_url(id) == https://github.com/o/r/commit/\u003cid\u003e"
    required: true
    manual: false
  - id: "C2"
    text: "remote_link::no_match: a gitlab.com, a bare path and an empty remote give None; a host merely containing github.com (notgithub.com, github.com.evil.io) gives None"
    required: true
    manual: false
  - id: "C3"
    text: "remote_link::seam: a RemoteLinkProvider trait (matches a parsed remote, builds commit_url) is held in a slice of providers, so a second provider added in a test is found without changing the lookup; GitHub is the only one registered by default"
    required: true
    manual: false
  - id: "C4"
    text: "remote_link::which_remote: Repo::commit_web_url(id) uses origin, else the sole remote, else None"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-git passes; clippy -D warnings clean; no file outside the scope changes"
    required: true
    manual: false
---

# Turn a git remote URL into a commit's web page through a provider seam in fe-git, with only GitHub implemented, https, ssh and scp-style URLs alike (Mac T1215)

## Problem
The user: "only open on github for github but allow room to support other providers". The hover card's "Open on GitHub" link needs a remote URL -> commit page mapping that other hosts can join later.
## Change
New crates/fe-git/src/remote_link.rs using remote.rs's remote listing. No network. Pure string parsing.
