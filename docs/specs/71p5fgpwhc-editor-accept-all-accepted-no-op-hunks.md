---
task_uid: "01m5q7z4rrtw01rz71p5fgpwhc"
task: "T245"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "Editor Accept All over a proposal whose hunks include no-ops emits AcceptedNoChange for each, counts them as accepted, and the AI Changes counts and Revert Turn then reflect them. Tests accept_all_emits_accepted_no_change, accept_all_then_revert_turn"
    required: true
    manual: false
---

# Editor Accept All: accepted no-op hunks emit AcceptedNoChange so panel counts and checkpoint adoption stay current (follow-up of T235 F16)

## Problem
The editor's Accept All flow in proposal_commands.rs skips accepted no-op hunks when no transaction exists: it emits no acceptance event or notification and returns zero, leaving AI Changes counts and checkpoint adoption stale (review finding F16, medium, T235).

## Change
Emit AcceptedNoChange for successfully decided no-op hunks, count them as accepted.

## Tests
An editor Accept All regression followed by Revert Turn.
