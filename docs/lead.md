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
