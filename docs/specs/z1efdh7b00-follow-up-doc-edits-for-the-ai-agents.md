---
task_uid: "01m7azsgg9thecpsz1efdh7b00"
task: "T624"
rev: 1
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "`docs/designs/agent-lanes.md:12-13`. Its Out of scope names `docs/designs/agent-review.md` and `docs/designs/agent-command-permissions.md`, neither of which exists: point to this record"
    required: true
    manual: false
  - id: "C2"
    text: "`agent-lanes.md:630` and `:689` (the review panel) and `:922-923` (`lane.run` and the permissions design): point to § 7 and § 8"
    required: true
    manual: false
  - id: "C3"
    text: "`agent-lanes.md` card 18's turn ends: add the ACP prompt response"
    required: true
    manual: false
  - id: "C4"
    text: "`docs/decisions/5at38c8v89-ai-chat.md` § 2's \"v1 sends no tools\" and § 17's \"the agent tool loop\" now point to § 6"
    required: true
    manual: false
  - id: "C5"
    text: "`docs/decisions/5at38c8v89-ai-chat.md` § 12's project `chat.exclude` matches neither the code nor this record (§ \"Corrections\")"
    required: true
    manual: false
---

# Follow-up doc edits for the AI agents record: agent-lanes.md and the chat record point to it (AI agents AG14, T1779) (Mac T1795)

# Follow-up doc edits for the AI agents record: agent-lanes.md and the chat record point to it (AI agents AG14, T1779)

Source: `docs/decisions/50t8wt9we2-ai-agents.md` § 13, card **AG14** — T1779's decision record (AI agents: Claude Agent over ACP, a tool loop for the API chat, permissions and review), approved by the lead with round-4 findings F6–F11 carried into card criteria (§ 14). Read the record's sections the card cites before starting.

Linux-OK: the card can be built and tested on Linux (§ 13; no card in the plan is Mac-only).

## The card, verbatim from § 13

**AG14. Follow-up doc edits.** Size **s**. Deps: approval of this record.
Linux-OK.

- Files: `docs/designs/agent-lanes.md` and
  `docs/decisions/5at38c8v89-ai-chat.md` (the edits listed in § 11).
- Test: none (docs).

## § 11 Supersedes, verbatim

## 11. Supersedes

- **T1568 and T1569 are abandoned** in favour of this record:
  - § 7 is T1569's design;
  - § 8 is the part of T1568 this design needs;
  - § 8's list is what T1568 leaves for a later review-panel design.
  The lead marks both abandoned on approval.
- **Stale references, for the follow-up edit** (card AG14):
  - `docs/designs/agent-lanes.md:12-13`. Its Out of scope names
    `docs/designs/agent-review.md` and
    `docs/designs/agent-command-permissions.md`, neither of which exists:
    point to this record.
  - `agent-lanes.md:630` and `:689` (the review panel) and `:922-923`
    (`lane.run` and the permissions design): point to § 7 and § 8.
  - `agent-lanes.md` card 18's turn ends: add the ACP prompt response.
  - `docs/decisions/5at38c8v89-ai-chat.md`:
    - § 2's "v1 sends no tools" and § 17's "the agent tool loop" now point to
      § 6;
    - § 12's project `chat.exclude` matches neither the code nor this record
      (§ "Corrections").
  - `docs/specs/4aasx0q1gw-…md:72-73` is a historical spec and is left as
    written.

**Decision:** T1568 and T1569 are abandoned. AG14 makes the edits listed.

## Plan-wide rules, verbatim from § 13

Cards are size **s** or **m**. Scopes are exact files, and tests are in new
files. "Linux-OK" means the card can be built and tested on Linux. Marks:

- **[chat_panel.rs]** touches `crates/fe-workbench/src/chat_panel.rs`;
- **[settings.rs]** touches `crates/fe-config/src/settings.rs`;
- **[Cargo deps]** adds Cargo dependencies, and needs the `deny.toml`
  licence check.

Cards that touch T604's files depend on **T604** (the picker card).

New fe-workbench modules are declared from `crates/fe-workbench/src/lib.rs`,
not from `chat_panel.rs`. Each card's tests are declared from its own new
module.

Order: wave 1 in parallel; then AG7, AG8 and AG9; then AG10. AG11 waits for
its lanes cards, then AG12 and AG13 follow.

- **Mac-only cards:** none. Every card is Linux-OK.
- **Mac check:** the sandbox's macOS path (Seatbelt) is checked in the
  manual smoke run on a Mac.

T604 in the record is Mac T1780 (the model and effort picker), already on main.

## Notes

- Line numbers are the record's, at its base `31df7806`; find the same text on main (T1780 since edited the chat record's § 4 and § 17).
