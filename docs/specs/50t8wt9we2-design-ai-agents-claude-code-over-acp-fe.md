---
task_uid: "01m7aavf0xn4vrhz50t8wt9we2"
task: "T603"
rev: 3
type: "docs"
complexity: "m"
criteria:
  - id: "C1"
    text: "docs/decisions/\u003clane\u003e-ai-agents.md exists with sections Status, What the adapter does, Edits as proposals, The ACP client crate, Chat panel and agents, The API tool loop, Permissions, Review, Trust and privacy, Relation to agent-lanes.md, Supersedes, Testing, Implementation plan, Out of scope and open questions; each ends in a decision or a named open question"
    required: true
    manual: false
  - id: "C2"
    text: "What the adapter does cites claude-agent-acp and the agent-client-protocol crate at named releases, and states with source lines whether Edit/Write go through fs/write_text_file and which tools can still write to disk directly; Edits as proposals says how each such path is kept from silent writes or says plainly where the rule cannot hold"
    required: true
    manual: false
  - id: "C3"
    text: "The ACP client crate states that Tenmoku never calls authenticate with credentials, never reads Claude Code's credentials, and never uses a subscription from the API chat, quoting Anthropic's 'Unless previously approved' rule; it defines the not-logged-in and adapter-missing states"
    required: true
    manual: false
  - id: "C4"
    text: "The API tool loop gives each tool's input schema and output cap, the ToolSpec/tool_use/tool_result wiring, per-turn loop limits, cancel mid-tool, and how history stays append-only; Permissions gives settings.toml keys with types and defaults, a project file that can only tighten, one model for both run_command and ACP request_permission, and an audit trail"
    required: true
    manual: false
  - id: "C5"
    text: "Supersedes says whether T1568 and T1569 are abandoned; Implementation plan lists cards of size s or m only with exact scope paths, dependencies, Linux-OK or Mac-only and new test files, marking cards that touch chat_panel.rs, fe-config settings.rs or Cargo dependencies"
    required: true
    manual: false
  - id: "C6"
    text: "The status line says proposed and names the lead as approver; no card is filed and git diff --stat against the base lists only the new doc"
    required: true
    manual: false
---

# Design AI agents: Claude Code over ACP (fe-acp client, the user's own claude login, edits as proposals), an agent tool loop for the API chat, allow/ask/deny permissions and review (folds T1568, T1569) (Mac T1779)

The user (2026-11-13) asked to flesh out the AI functionality and to support a Claude Code subscription, and chose three things:

1. **Claude Code through an ACP agent panel.** Tenmoku spawns the user's own installed Claude Code through the Agent Client Protocol: the adapter `@agentclientprotocol/claude-agent-acp` (Apache-2.0; formerly `@zed-industries/claude-code-acp`; it runs the `claude` CLI, and `CLAUDE_CODE_EXECUTABLE` points it at the user's own) and the Rust client crate `agent-client-protocol` (crates.io, Apache-2.0, `ClientSideConnection` + the `Client` trait; its futures are not Send). **Tenmoku never handles claude.ai credentials**: the user logs in through Claude Code's own `/login`. Anthropic's Agent SDK docs: "Unless previously approved, Anthropic does not allow third party developers to offer claude.ai login or rate limits for their products". The agent's edits and writes reach the user only as **proposals** in the existing review flow, never as silent writes. The client is generic ACP so Gemini CLI or Codex can plug in later, but only Claude Code is in scope.
2. **An agent tool loop for the built-in API-key chat**: the model reads and searches files and proposes edits itself, and runs commands behind allow/ask/deny permissions. This folds in T1568 (agent review panel design) and T1569 (agent command permissions design); their docs (docs/designs/agent-review.md, agent-command-permissions.md) were never written.
3. An in-chat model and effort picker: filed as its own card (the picker card, below); this design only says how the picker drives an ACP agent.

Out of scope: attachments beyond today's, usage and cost, ghost-text completions, Claude Code's terminal IDE protocol.

This is a design card: one doc, no code, no cards filed.

## Read first

- docs/decisions/5at38c8v89-ai-chat.md (the chat record: § 2 provider trait, § 4 history, § 5 caching, which already reserves the tools slot, § 8 proposals, § 9 AI actor and grants, § 10 privacy and untrusted folders, § 17 out of scope).
- docs/designs/agent-lanes.md (the lane model, `LaneKind::External { host }`, `Backing::Overlay` vs `Backing::Worktree`, § Lane model › What a protocol must give, § Card breakdown; none of its cards are filed yet).
- docs/decisions/q5j03ax2af-lsp-add-on-servers.md and crates/fe-lsp/src/launcher.rs (login-shell PATH resolution, spawning, missing-program toast naming the npm package) and crates/fe-workbench/src/trust.rs, crates/fe-workbench/src/lsp.rs (how folder trust gates a spawn).
- The code: crates/fe-chat/src/{provider.rs (ChatProvider, ChatEvent, the unused ToolSpec, ToolChoice, ToolCall), conversation.rs, request.rs, edits.rs}, crates/fe-chat-anthropic/src/{wire.rs, events.rs, provider.rs}, crates/fe-chat-view/src/view.rs, crates/fe-workbench/src/{chat_panel.rs, chat_apply.rs, chat_changeset.rs, ai_changes_panel.rs, chat_context.rs, chat_store_worker.rs}, crates/fe-editor/src/changeset.rs, crates/fe-buffer/src/proposed.rs, kiln_command::Grants.
- For the API tool loop, use the claude-api skill (current tool-use rules: forced `tool_choice` any/tool is a 400 on Claude Opus 5.5, so `auto` plus `strict: true`; `eager_input_streaming` and validating partial input; all parallel `tool_result`s in one user message; `is_error` results; preserved-thinking append-only history; task budgets beta). For ACP use Context7 or the agentclientprotocol.com spec and the adapter's source at a named release.

## Deliverable

docs/decisions/<lane>-ai-agents.md, sections in this order, each ending in a decision or a named open question:

1. **Status**: proposed; the approver is **the lead** (core), not the user: the user is away until 2026-11-16 and asked the lead to approve; the user reviews it on return. No code changed.
2. **What the adapter does**, at a named release of claude-agent-acp and of the agent-client-protocol crate: protocol version (v1, or the draft v2 it also serves), the auth methods it reports and what happens when `claude` is not logged in, the session methods (new, load/resume, cancel, set mode, model and config options), the session updates it sends (message and thought chunks, tool calls and their updates, plans, diffs), and above all **how its file edits and commands reach disk**: whether, with the client's `fs.readTextFile`/`fs.writeTextFile` capabilities, Edit and Write go through `fs/write_text_file`; whether Bash, formatters or any other Claude Code tool can still write files directly; which permission modes exist and what each asks through `session/request_permission`. Cite the source lines.
3. **Edits as proposals**: from section 2, how the "never a silent write" rule holds: `fs/write_text_file` becomes a proposal (`Origin::Ai { agent, session }`, chat_apply's path or the lane model's `lane.propose`) and the agent is answered when; what the agent reads back through `fs/read_text_file` (the buffer, with pending proposals or without); and what happens to anything that can still write directly (commands): deny by default, a worktree lane (`Backing::Worktree`), or a permission the user grants knowing it writes. Decide; if the rule can't hold for some path, say so plainly.
4. **The ACP client crate**: name (fe-acp or another), headless, linux-ok; the runtime for the crate's non-Send futures beside GPUI and fe-lsp's runtime; locating the adapter (`claude-agent-acp` on the login-shell PATH as fe-lsp does, `npx`, or a setting) and the user's `claude`; what it never does (no `authenticate` with credentials, no reading ~/.claude or keychain items, no subscription use from the API chat); the session lifecycle (spawn per window or per root, initialize, new or resume, prompt, cancel, crash and restart, shutdown); the "not logged in" state ("Run `claude /login`", and whether a button opens it in Tenmoku's terminal); the "adapter not installed" state naming the npm package; the fake ACP agent the tests use.
5. **Chat panel and agents**: an Agent choice in the chat header (Claude through the API key, Claude Code); is it fixed per conversation; how ACP updates render in fe-chat-view (text, thoughts, tool call rows with status, plans, diffs); how ACP chats persist and resume (the adapter's session load against chat_store), and memory-only in untrusted folders; how the picker card's model and effort picker drives an ACP session (the adapter's model and effort config options).
6. **The API tool loop**: the tool set (read_file, list_files, search, propose_edit, run_command), each with its input schema, output cap and what it may touch (the chat's @file deny list and `chat.exclude` apply); ToolSpec, ToolChoice and ToolCall wiring in fe-chat and tool_use/tool_result in fe-chat-anthropic; how a Turn holds several API messages and stays append-only for the prompt cache and preserved thinking; loop limits (rounds per turn, a task budget, wall time), stop and cancel mid-tool; what the user sees for each call; whether the loop is on by default; untrusted folders.
7. **Permissions** (folds T1569's sections: what a permission covers, levels, scope and trust, setting shape, audit trail): allow, ask and deny per class (read, search, propose edit, run command by program or pattern); one model for both the API loop's run_command and ACP's `session/request_permission` (its allow-once, allow-always, reject options map onto it); the prompt (a toast with choices, an inbox row, or a row in the chat), remember-for-session; the `settings.toml` keys with types and defaults, where a project file can only tighten (a `ProjectFilter`, as `chat` and `lsp` are narrowed); relation to `kiln_command::Grants` and plugin authorization; where granted and refused calls are recorded.
8. **Review** (folds the parts of T1568 that this needs): how a turn's proposals from either agent reach the Changeset and the AI Changes panel (ai_changes_panel.rs), what "request changes" or a hunk comment sends back (an ACP prompt, a user turn), and what of T1568's fuller panel (viewed files, progress, comments mid-run) is left for later.
9. **Trust and privacy**: untrusted folders spawn no agent (Claude Code would run the repo's CLAUDE.md, .claude settings, hooks and MCP servers) and give the API loop no tools beyond attachments, or say otherwise; the first-open notice for Claude Code (data goes to Anthropic under the user's Claude Code account; Claude Code's own settings, hooks and MCP servers apply); the grant strings for a spawned agent (`process.spawn:<program>`, as agent-lanes.md § What a protocol must give).
10. **Relation to agent-lanes.md**: which of its cards (headless lane model, detached hunks, chat as lane 1) this needs first and which it does without; whether the ACP agent is `LaneKind::External`.
11. **Supersedes**: say whether T1568 and T1569 are abandoned in favour of this doc, and list the stale references (agent-lanes.md lines 12-13 name the two missing docs) for a follow-up edit.
12. **Testing**: the fake ACP agent (an AgentSideConnection test binary or example in the new crate) and the scripted fake chat provider for tool loops; what a real-adapter smoke run checks by hand (needs a Claude Code login: manual).
13. **Implementation plan**: cards of size s or m only, each with scope paths (exact files, new tests in new files), dependencies, Linux-OK or Mac-only, and its new test files; mark every card that touches crates/fe-workbench/src/chat_panel.rs or crates/fe-config/src/settings.rs (both are hot), and the card that adds Cargo dependencies (deny.toml licence check). They depend on the picker card where they touch the same chat files.
14. **Out of scope and open questions**.

## Notes

Touch no file but the new doc. No implementation card is filed by this card: the lead files them from section 13 after approving. Linux-OK (docs only).
## Round-3 respec (approver decision, Mac lead)

Round 3 findings F2, F4 (high) and F5 (medium) are fixed in the decision doc, not carried. Touch only the decision doc.

1. F2: add `managed-settings.d/*.json` and macOS managed preferences to § 3's managed-settings source table and to the watch list, or fail closed (refuse to start the agent, with the reason) when a source cannot be inspected.
2. F2: replace the "other tools' allow rules don't touch the sandbox" bullet: `WebFetch(domain:...)` and any other permission rule that grants network domains widen the network allowlist, so apply the same loopback/domain check as `sandbox.network.allowedDomains`; add a test and a smoke case.
3. F4: at spawn, resolve the common git dir and the worktree admin dirs (`git rev-parse --git-common-dir` and `--git-dir`) and put them in `sandbox.filesystem.denyWrite`, rather than relying on omitting allowWrite; smoke-cover both a main-checkout root and a linked-worktree root.
4. F5: smoke step 3 uses a relative project `Edit` rule (a bare `Edit` is refused by § 3); step 12 nests `filesystem`/`network` under `sandbox`; step 14 is split into currently-loaded sources vs files checked only for the Update lane.

If round 4 finds a NEW gap in the same sandbox/settings section, ask the Mac lead again; they will take it as proposed with named open questions.

## Round-4 decision (approver, Mac lead): take it as proposed

Round 4 confirmed F2/F4/F5 fixed and found F6-F11 (read them with `rgt review packet T603`). Do not fix them in the text further; record and carry them. Doc only, one pass:

1. Add an "Open questions carried to implementation" subsection listing F6, F7, F8, F9, F10, F11. For each: one line for the gap, one for the bounded fix, and the section-13 implementation card (by its entry) that must close it.
2. F6 (managed `allowManagedPermissionRulesOnly` vs our forced asks) and F7 (SandboxNetworkAccess requests arriving as kind Other) are high and security-relevant: in section 13 make closing each an explicit acceptance criterion of the card that implements Claude Code permissions/sandbox, with a test that the forced ask still prompts under allowManagedPermissionRulesOnly (or the agent refuses to start, with a reason), and a test that a SandboxNetworkAccess request is never auto-approved or remembered.
3. F8 and F11 become criteria of the sandbox/settings card; F9 and F10 of the API tool-loop card.
4. Then dispute each of F6-F11 once: `rgt review dispute <finding> "carried to named implementation criteria (doc § open questions)"`, and run `rgt finish` once.
