---
task_uid: "01m5ztnq5hzjqy5czp7h4q8228"
task: "T1207"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Cargo.toml [profile.dev] sets incremental = false with a comment giving the reason (lane targets filling the disk)"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/ci-env.sh comment matches the new behaviour (snapshot CARGO_INCREMENTAL=0 kept as a harmless repeat or removed)"
    required: true
    manual: false
  - id: "C3"
    text: "the commit message records lane target size (du -sh target after a workspace all-targets build plus nextest --no-run) and the fe-workbench one-line-edit test rebuild time, before and after"
    required: true
    manual: false
  - id: "C4"
    text: "the gate passes"
    required: true
    manual: false
---

# Build dev profiles non-incrementally so a lane's target stops filling the disk (incremental = false in [profile.dev])

Lane builds fill the Mac's disk. On 10-27 a lane target reached 55-66 GB while its gate's verification snapshot (.worktrees/.verify/<id>/target) took ~42 GB. One lane plus its gate filled the disk: T1195's post-merge check died with ENOSPC, main was reverted and frozen.

Snapshots have built non-incrementally since T158 (scripts/ci-env.sh sets CARGO_INCREMENTAL=0 and CARGO_PROFILE_DEV_DEBUG=line-tables-only under .worktrees/.verify/). Lanes still build incrementally, and the incremental data is a large share of a lane target: a partly built lane target measured 5.9 GB, 1.9 GB of it target/debug/incremental. The workspace [profile.dev] already uses debug = "line-tables-only" with split-debuginfo = "unpacked", so debug info is not the lever.

Change: make dev builds non-incremental everywhere: `incremental = false` in the workspace [profile.dev] in Cargo.toml. That covers worker cargo runs in lanes, the fast pipeline and snapshots alike (lanes get no environment from rgt beyond CARGO_TARGET_DIR, so a profile setting is the only lane-wide lever). Keep or drop the now-redundant CARGO_INCREMENTAL=0 in ci-env.sh, and update its comment either way. With incremental off, sccache can also cache workspace crates (it skips incremental units); check `sccache --show-stats` shows workspace crates being cached, and note it. Don't change SCCACHE_CACHE_SIZE.

Measure in your own lane, before and after the change, and put the numbers in the commit message:
- `du -sh target` after `scripts/ci-env.sh cargo build --workspace --all-targets` then `scripts/ci-env.sh cargo nextest run --workspace --no-run` from clean;
- the wall time to rebuild fe-workbench's tests (`cargo nextest run -p fe-workbench --no-run`) after a one-line edit in a fe-workbench source file.
If the edit-rebuild time more than doubles, still make the change (disk is the binding limit), but say so in the commit message.
