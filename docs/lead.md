# Leading fastedit

The lead guide lives in the Regatta repo: `~/code/regatta/docs/lead.md`. Read it first; it covers
attaching, the watch, the needs-lead rows, subagents, flakes and rotation.

What differs here:

- fastedit has no `.claude/agents/`. Where the guide names `gate-triage`, `worker-check`,
  `inbox-triage` or `card-drafter`, start a `general-purpose` subagent with the same brief, and tell
  it never to run `tsk` or a mutating `rgt` command.
- The Mac runs one lane (`lanes_max = 1`). Disk is the limit: a lane build is ~45 GB, a finish gate
  ~42 GB plus ~31 GB per regression side for a bugfix, a post-merge check ~30-35 GB. Tell each worker
  to ask before any cargo build and before `rgt finish`, and give one go at a time.
- Other hosts (neon, argon, helium) have their own leads and boards; reach them over ssh with
  `herdr agent prompt`.

## Host batches

Mac cards lent to a host come back as a batch: the host lead cherry-picks the finished work onto
`mac/main` as a `<host>-batch-<n>` branch, gates it on Linux and posts "READY" in that host's own
`~/coordination/landing.md`. Nothing on the Mac sees that note: the watch reads only the Mac's
events, so a READY batch waits until a lead looks. Batches have sat unlanded for a day that way.

- **Find them each heartbeat.** `~/.local/share/fastedit-lead/unlanded-host-work.sh` (run by
  `hosts.sh`) lists every "(Mac T<n>)" commit on a host's main that Mac main has neither as a pick
  nor as its own "(T<n>)" commit. A card listed with no READY batch is finished but unbatched: ask
  that host's lead to cut one.
- **Land them as a stack.** Fetch the branch (`git fetch ssh://<host>/home/daryl/code/fastedit-lanes
  <branch>`), cherry-pick it onto Mac main in a `.worktrees/gate-stack-<n>` worktree (conflicts in
  module and dependency lists keep both sides), and run the full macOS gate there: fmt, clippy,
  build, nextest, no_gpui, syntax features, deny and the screenshot examples. Write the plan, the
  host-to-Mac task mapping and the after-landing steps in `.worktrees/LANDING-stack-<n>.md`.
- **After a green gate:** fast-forward main to the stack, one stack at a time and only while main's
  post-merge check is green; the daemon post-checks and pushes. Then sync the hosts, abandon the
  Mac copies of the landed cards (the lent copies were blocked when lent), fix dependents whose
  dependency is now abandoned (abandoned is not satisfied), tell the host lead, and remove the
  worktree after `cargo clean`.
- **Carry unlanded batches in the handoff** under "Requests awaiting reply", by branch and SHA.
