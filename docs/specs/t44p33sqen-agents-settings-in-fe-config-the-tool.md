---
task_uid: "01m7azsb8xb10xb0t44p33sqen"
task: "T615"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test in crates/fe-config/tests/agent_settings.rs: defaults"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-config/tests/agent_settings.rs: a project's levels and deny/ask lists move to `project_permissions` and only tighten"
    required: true
    manual: false
  - id: "C3"
    text: "Test in crates/fe-config/tests/agent_settings.rs: `commands_allow` and `[agents.claude_code]` dropped with warnings"
    required: true
    manual: false
  - id: "C4"
    text: "`template.rs` still passes"
    required: true
    manual: false
---

# [agents] settings in fe-config: the tool-loop limits, permission levels and command lists, [agents.claude_code], and narrow_agents so a project file can only tighten (AI agents AG2, T1779) (Mac T1783)

# [agents] settings in fe-config: the tool-loop limits, permission levels and command lists, [agents.claude_code], and narrow_agents so a project file can only tighten (AI agents AG2, T1779)

Source: `docs/decisions/50t8wt9we2-ai-agents.md` § 13, card **AG2** — T1779's decision record (AI agents: Claude Agent over ACP, a tool loop for the API chat, permissions and review), approved by the lead with round-4 findings F6–F11 carried into card criteria (§ 14). Read the record's sections the card cites before starting.

Linux-OK: the card can be built and tested on Linux (§ 13; no card in the plan is Mac-only).

## The card, verbatim from § 13

**AG2. `[agents]` settings.** Size **s**. Deps: none. Linux-OK.
**[settings.rs]**

- Files:
  - `crates/fe-config/src/agent_settings.rs` (new: the structs and defaults
    of § 7, and `narrow_agents`);
  - `crates/fe-config/src/settings.rs` (the `agents` field; the project
    filter in `options()`);
  - `crates/fe-config/src/lib.rs`;
  - `crates/fe-config/src/l10n/template.rs`;
  - `crates/fe-config/l10n/en-US/settings.ftl`;
  - `crates/fe-config/l10n/en-US/settings-template.ftl`.
- New test file: `crates/fe-config/tests/agent_settings.rs`.
- Tests:
  - defaults;
  - a project's levels and deny/ask lists move to `project_permissions` and
    only tighten;
  - `commands_allow` and `[agents.claude_code]` dropped with warnings;
  - `template.rs` still passes.

## § 7 Settings and "A project file can only tighten", verbatim

### Settings

Settings live in `settings.toml`. All are user-level unless marked:

```toml
[agents]
tool_loop = true              # bool. The API chat offers tools (trusted folders only)
max_rounds = 25               # u32, 1..=200. Requests per turn
task_budget = 0               # u32 tokens; 0 = off, else >= 20000
turn_time_limit_secs = 600    # u32, 30..=7200
command_timeout_secs = 120    # u32, 1..=3600

[agents.permissions]          # each class: "allow" | "ask" | "deny"
read = "allow"
search = "allow"
propose_edit = "allow"
run_command = "ask"
fetch = "ask"
other = "ask"
commands_allow = []           # Vec<String>, patterns as above
commands_ask = []             # Vec<String>
commands_deny = []            # Vec<String>

[agents.claude_code]
adapter = ""                  # String path; "" = claude-agent-acp on the login PATH
claude = ""                   # String path; "" = claude on the login PATH
sandbox_allow_write = []      # Vec<String> extra paths Claude Code's sandbox may write
mcp_servers = false           # bool. Let Claude Code run the MCP servers its own settings configure (§ 3)
```

### A project file can only tighten

A project's `settings.toml` is narrowed by
`ProjectFilter::new("agents", narrow_agents)`, beside `chat` and `lsp`
(`crates/fe-config/src/settings.rs:1620-1624`).

- **What it keeps.** `narrow_agents` keeps only the project's
  `agents.permissions` class levels, `commands_ask` and `commands_deny`.
- **Where it puts them.** It **moves** them to `agents.project_permissions`.
  - Settings layers merge last-wins, so a project value under
    `agents.permissions` could loosen the user's.
  - Under its own key, the effective policy is per class the stricter of the
    user's and the project's. Lists are the union.
- **What it drops,** each with a warning in the `PROJECT_CHAT` style:
  - `commands_allow`;
  - every other `[agents]` key: the round, time and budget limits, and
    `tool_loop`;
  - all of `[agents.claude_code]`, since a cloned repo must not choose the
    programs Tenmoku runs.
- **No file may set `agents.project_permissions` itself:** a layer filter drops it with a warning, and only `narrow_agents` fills it from a project's file; the user tightens through `agents.permissions`.

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

## Scope beyond the card's file list

- `crates/fe-config/settings-template.toml` and `crates/fe-config/settings.schema.json`: every settings card that adds a table edits both (the `[chat]` card T966 and LSP add-ons A1 did); `settings-template.ftl` is generated from the template, and the schema golden in `src/tests/settings.rs` fails without the new keys.

## Round-4 decision (approver, Mac lead)

F1 (high): a user-set `agents.project_permissions` must not be honoured. No kiln-core change: use the existing `SettingsOptions::layer_filter` (kiln/crates/kiln-core/src/settings.rs, it runs on the user file too, before the project filters). One more pass, no scope add:

1. In the record docs/decisions/50t8wt9we2-ai-agents.md (section 7, around the bullet "If the user's own file sets agents.project_permissions ... harmless"), replace it with the sentence above (decision files are exempt from scope).
2. In `options()`, add `.layer_filter(ProjectFilter::new("agents.project_permissions", drop_project_permissions))`, dropping the key with a warning from every file; `narrow_agents` rebuilds it from the project's own values only. Keep the key out of the settings editor's user rows.
3. Tests: a user-set `project_permissions` is dropped with a warning; a project `allow` cannot loosen a user `deny` for each of the six classes; empty project lists erase nothing.
