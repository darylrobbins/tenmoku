# AI agents: Claude Agent over ACP, a tool loop for the API chat, permissions and review

Source card T603 (Mac T1779). Researched 2026-11-13 against fastedit
`31df7806`. Folds T1568 (agent review panel design) and T1569 (agent command
permissions design), whose docs were never written.

This record decides how Tenmoku runs AI agents. It covers three things:

1. the user's own installed Claude Code, driven through the Agent Client
   Protocol (ACP);
2. a tool loop for the built-in API-key chat;
3. the permissions and the review that both of them go through.

It changes no code and files no cards. The lead files them from § 13 after
approving.

**What was read.**

- **Adapter.** `@agentclientprotocol/claude-agent-acp` **0.106.0** (npm
  `latest`, published 2026-11-10; git tag `v0.106.0`, commit `51f514d`,
  Apache-2.0), cloned and read at that tag. Its `package.json` pins
  `@agentclientprotocol/sdk` 2.6.0 and `@anthropic-ai/claude-agent-sdk`
  0.3.322. I also read that SDK's `sdk.d.ts`, from its npm tarball; its
  licence is "SEE LICENSE IN README.md", which means Anthropic's terms, not an
  open-source licence. All adapter citations are `path:line` in the tag's
  tree, and `acp-agent.ts` means `src/v1/acp-agent.ts`.
- **ACP crate.** `agent-client-protocol` **5.2.0** (crates.io, published
  2026-11-10, Apache-2.0, edition 2024, rust-version 1.88) and
  `agent-client-protocol-schema` **1.17.0**, which 5.2.0 pins exactly
  (`=1.17.0`). Both were read from their `.crate` sources. Citations are
  `path:line` in each crate. "schema" means the schema crate.
- **Anthropic's Agent SDK overview**
  (`code.claude.com/docs/en/agent-sdk/overview`), fetched 2026-11-13, for the
  login rule and the branding guidelines.
- **The claude-api skill** (`shared/tool-use-concepts.md`,
  `shared/prompt-caching.md`, `shared/preserved-thinking-migration.md`, and
  the task-budget and tool-choice quick references) for the tool loop's API
  rules.
- **Repo at the base.**
  - `docs/decisions/5at38c8v89-ai-chat.md` (the chat record, "the chat
    record" below) and `docs/designs/agent-lanes.md`.
  - `crates/fe-chat/src/{provider,conversation,request,fake,store}.rs` and
    `crates/fe-chat-anthropic/src/{wire,events,provider}.rs`.
  - `crates/fe-chat-view/src/view.rs`.
  - `crates/fe-workbench/src/{chat_panel,chat_apply,chat_changeset,ai_changes_panel,chat_context,chat_store_worker,trust,lsp}.rs`.
  - `crates/fe-editor/src/changeset.rs` and `crates/fe-buffer/src/proposed.rs`.
  - `crates/fe-lsp/src/launcher.rs`, `crates/fe-config/src/settings.rs`,
    `crates/fe-project/src/search.rs` and
    `kiln/crates/kiln-command/src/{capability,invocation,authorize}.rs`.
  - The picker card, **T604**: its scope and spec.

**Corrections to the card's spec.**

- **The crate API the spec names is gone.** The spec says the
  `agent-client-protocol` crate has "`ClientSideConnection` + the `Client`
  trait; its futures are not Send". That was true up to 0.10.4. 0.11.0
  (2026-04-20) replaced it: "Migrate to new SDK design (#117)", crate
  `CHANGELOG.md:631-636`.
  - In 5.2.0, `Client` and `Agent` are role markers, and a connection is built
    with `Client.v1()` (`src/role/acp.rs:97`).
  - The crate says: "Handlers, transports, and connection futures must be
    `Send` on every target" (`src/lib.rs:177`).
  - Tokio is a dev-dependency only (removed as a dependency in 0.12.0/0.13.0).
  - So the runtime question in § 4 has a simpler answer than the spec
    expected: the connection runs on GPUI's background executor, as the chat's
    stream pump does.
- **The adapter does not write through the client.** The spec asks whether,
  with the client's `fs` capabilities, Edit and Write go through
  `fs/write_text_file`. At 0.106.0 they never do (§ 2). The adapter has not
  used client file or terminal capabilities since 0.18.0. So "edits as
  proposals" cannot be built on `fs/write_text_file` for Claude Code. § 3
  builds it on a worktree lane instead.
- **Naming.** Anthropic's branding guidelines for products built on the Agent
  SDK do not permit "Claude Code" or "Claude Code Agent" as a label. They
  prefer "Claude Agent" in dropdown menus. The adapter calls itself "Claude
  Agent" too (`agentInfo.title`, `acp-agent.ts:2081`). So the chat header's
  choice is **Claude Agent**. UI text names the `claude` program only where
  the user must run it.
- **The chat record and the code differ on project settings.** Chat record
  § 12 says a project file may still set `chat.exclude`. The code drops a
  project's whole `[chat]` table (`drop_chat`,
  `crates/fe-config/src/settings.rs:2127`), and `chat.exclude` does not exist
  anywhere. This record does not rely on `chat.exclude`. The deny list it uses
  is `chat_context::refusal` (`crates/fe-workbench/src/chat_context.rs:214`).
  § 11 lists the gap for the follow-up edit.

## 1. Status

Status: **proposed**. Approver: **the lead** (core), not the user. The user
is away until 2026-11-16 and asked the lead to approve. The user reviews this
record when they return, and may reopen any decision.

No code changed: this card adds only this file. No implementation card is
filed by it.

**Decision:** proposed. The lead approves it, files § 13's cards and marks
T1568 and T1569 abandoned (§ 11).

## 2. What the adapter does

Everything in this section is from `claude-agent-acp` 0.106.0 and
`agent-client-protocol` 5.2.0 / schema 1.17.0.

### Protocol version

- The adapter answers `initialize` with `protocolVersion: 1`
  (`acp-agent.ts:2047`).
- A client that asks for version 2 or later gets the draft v2, which a
  separate agent in `src/v2/` serves (`README.md:131-146`).
  `CLAUDE_AGENT_ACP_PROTOCOL` pins one version (`README.md:137`).
- The Rust crate speaks v1 by default (`ProtocolVersion::V1`, schema
  `src/version.rs`). v2 is behind the crate's `unstable_protocol_v2` feature,
  and "may change in minor releases" (`src/lib.rs:140-143`).
- **Tenmoku speaks v1 only.**

### Auth methods, and what happens when `claude` is not logged in

- **Login methods.** The adapter reports login methods only if the client
  declares `clientCapabilities.auth.terminal` or
  `_meta["terminal-auth"]` (`acp-agent.ts:2022-2029`). Without either, the
  list is empty (`auth-methods.ts:29`).
- **What those methods are.** All are terminal methods: the client is
  expected to run the adapter's own command with arguments, such as
  `--cli auth login --claudeai` for "Claude Subscription" or `--console`
  (`auth-methods.ts:47-81`).
- **What `authenticate` accepts.** It accepts only the gateway methods (a
  custom Anthropic or Bedrock gateway, offered only when the client declares
  `auth._meta.gateway`). Anything else is `invalidParams`
  (`acp-agent.ts:2404-2420`).
- **Not logged in.** When `claude` is not logged in, a session or a turn
  fails with ACP's `authRequired` JSON-RPC error. The adapter takes that code
  from the SDK (`acp-agent.ts:1699-1701`). It keeps the rejection rather than
  ending the turn normally, "ACP defines its JSON-RPC error as the signal that
  starts the client's own auth flow" (`acp-agent.ts:4457-4480`).
- **Auth status.** The adapter also pushes `_auth/status_update` with a
  `kind` of `account`, `api_key`, `gateway`, `external` or `none`
  (`auth-status.ts:55`, `:60`). The payload can carry the account's email and
  organisation.
- **`--hide-claude-auth`.** An integration that must never bill a claude.ai
  subscription can start the adapter with this flag. Then it "hides the
  claude.ai login method from `initialize`, and refuses with `authRequired`
  every session and every turn that a subscription would pay for"
  (`hide-claude-auth.ts:1-6`, `:38-42`).

### Finding `claude`

The adapter runs the Claude CLI from three places, in order
(`claude-cli.ts:19-31`):

1. `CLAUDE_CODE_EXECUTABLE`, if set;
2. the CLI shipped beside a standalone build;
3. the SDK's own platform package.

So without the variable, the adapter does not run the user's installed
`claude`.

### Session methods (v1)

| Method | Where |
|---|---|
| `session/new` | `acp-agent.ts:2122` |
| `session/load` (replays the history as updates; `loadSession: true`) | `:2310`, `:2076` |
| `session/resume` | `:2298` |
| `session/list` | `:2337` |
| `session/close` | `:7941` |
| `session/delete` | `:7968` |
| `session/prompt` | `:2603` |
| `session/cancel` | `:7400` |
| `session/set_mode` | `:7972` |
| `session/set_config_option` | `:7978` |
| `session/fork`, `session/status`, `session/options`, providers, `logout` | unstable |
| `_session/steering` (ext method) | `:579` |

- **No `set_session_model` in v1.** Neither crate has one (schema
  `src/v1/agent.rs:6705-6741`). Model and effort are config options:
  - `model` (`session-model.ts:6`);
  - `effort`, category `thought_level` (`session-config-ids.ts:5`,
    `session-effort.ts:138-143`).
- **Steering mid-turn.** Prompts sent while a turn runs are queued; the ext
  method steers the running turn instead.

### Session updates

The adapter sends these `sessionUpdate`s:

- `agent_message_chunk` and `agent_thought_chunk`;
- `tool_call` and `tool_call_update`;
- `plan` (`plans.ts:50`);
- `available_commands_update`, `current_mode_update` and
  `config_option_update`;
- `session_info_update` and `usage_update`.

The crate's `SessionUpdate` is `#[non_exhaustive]` and has these and more,
including `Notice`, `CompactionUpdate`, and unstable subagent variants
(schema `src/v1/client.rs:105-183`).

A tool call has these fields (schema `tool_call.rs:31`, `:475`, `:516`,
`:548`, `:743`):

- `kind`: Read, Edit, Delete, Move, Search, Execute, Think, Fetch,
  SwitchMode or Other;
- `status`: Pending, InProgress, Completed or Failed;
- `content`, including `Diff { path, old_text, new_text }`;
- `locations` and `raw_input`.

For an Edit or Write, the adapter fills `content` with a diff:
`{ type: "diff", path, oldText, newText }` (`tool-calls/reporters/file-edit.ts:25-60`,
`diff.ts:183`).

### How its file edits and commands reach disk

This is the question the design turns on.

- **Edit, Write, MultiEdit and NotebookEdit write to disk themselves.** They
  are Claude Code's built-in tools, run inside the `claude` process.
  - The 0.18.0 changelog says: "Switch over to built-in Claude tools. We no
    longer replicate specific ACP tools … This means it won't use client
    capabilities for files or terminals" (`CHANGELOG.md:1407`).
  - At 0.106.0 the adapter's `readTextFile` and `writeTextFile` are
    pass-throughs (`acp-agent.ts:8593-8601`, `v1/client-connection.ts:44-50`).
    Nothing in `src/` calls them (searched: the only hits are those
    definitions).
  - So **declaring `fs.writeTextFile` changes nothing: no edit goes through
    `fs/write_text_file`, and no read goes through `fs/read_text_file`.**
- **Bash runs inside Claude Code**, in the session's `cwd`, and writes
  whatever its command writes.
  - The client's `terminal` capability is not used in v1 (the same changelog
    line). v2 only uses it to *display* commands (`README.md:145`).
- **Anything else Claude Code runs can write directly**, unasked:
  - hooks (the user's `~/.claude/settings.json`, the project's
    `.claude/settings.json` and `settings.local.json`; the adapter loads all
    three, `settingSources: ["user", "project", "local"]`,
    `acp-agent.ts:10179`);
  - MCP servers and plugins those settings configure;
  - subagents, which use the same tools.
- Claude Code has no formatter of its own; formatters run as hooks.

### What the client can set at `session/new`

`_meta.claudeCode.options` reaches the SDK's `Options`
(`new-session-meta.ts:12-33`).

- The adapter spreads it in at `acp-agent.ts:10199`. It then overrides
  `cwd`, `permissionMode` and `canUseTool` (`:10214-10216`), and merges
  `disallowedTools` (`:10241`).
- Callbacks such as `hooks` can't cross JSON-RPC (`acp-agent.ts:1630-1633`).
- JSON-only keys do reach the SDK. Five matter here:
  - `settings`: an object loaded as Claude Code's "flag settings" layer,
    "the highest priority among user-controlled settings" (SDK
    `sdk.d.ts:2298-2315`; the adapter applies it at `acp-agent.ts:10139`,
    `:10204`). Lists such as `permissions.ask` merge with the other
    sources' lists rather than replace them;
  - `sandbox`: Claude Code's sandbox for Bash, with `enabled`,
    `failIfUnavailable`, `autoAllowBashIfSandboxed`,
    `allowUnsandboxedCommands` and `filesystem.allowWrite`/`denyWrite`
    (`sdk.d.ts:2297`, `:3600-3625`);
  - `tools`: the base set of built-in tools, as an array of names; tools not
    named are not available (`sdk.d.ts:1714-1723`; the adapter passes it
    through, `acp-agent.ts:10092-10094`);
  - `strictMcpConfig: true`: only MCP servers passed in `mcpServers`, none
    from the user's or project's settings, `.mcp.json` or plugins
    (`sdk.d.ts:2410-2416`);
  - `allowDangerouslySkipPermissions: false`, which removes the bypass mode
    (`new-session-meta.ts:27-28`, `acp-agent.ts:10046-10049`).
- **`session/load` and `session/resume` take the same `_meta`.** The adapter
  builds a fresh query from their parameters, and the doc comment says
  `allowDangerouslySkipPermissions: false` must be repeated on load
  (`new-session-meta.ts:27-28`). A changed option rebuilds the query
  (`OPTION_REBUILDS_SESSION`, `acp-agent.ts:1563-1568`;
  `computeSessionFingerprint`, `:1669`).

### Permission modes

The modes are (`session-mode.ts:437-473`; settings aliases at
`permissions/modes.ts:11-24`):

| Mode | Name shown | What it does |
|---|---|---|
| `default` | "Manual" | Always ask before making changes |
| `acceptEdits` | | Edits in the working directories without asking |
| `plan` | | Plans before making changes |
| `auto` | | Claude decides; hidden if settings disable it |
| `bypassPermissions` | | Accepts all; hidden for root or when disabled |

`dontAsk` is a settings-only alias.

- **When Claude Code asks.** It asks through the SDK's `canUseTool` callback,
  which the adapter turns into `session/request_permission`
  (`acp-agent.ts:9037`, sent at `:8824`).
  - It asks only when its own rules and mode leave a call at "ask".
  - The adapter never auto-allows by mode. "A request that still reaches this
    callback is deliberately bypass-immune (for example … an explicit ask
    rule)" (`acp-agent.ts:9119-9124`).
- **The options offered.** `allow_once` ("allow-once"), `allow_always`
  ("allow-with-updates") and `reject_once` (`permissions/options/shared.ts:5-47`).
  - `allow_always` writes a rule into Claude Code's `localSettings`
    (`permissions/effects.ts:71-74`).
  - It is withheld when an ask rule forced the prompt
    (`acp-agent.ts:9126-9131`).
- **On the client side,** the outcome is `Selected { option_id }` or
  `Cancelled`. A client must answer pending requests `Cancelled` after it
  sends `session/cancel` (schema `src/v1/client.rs:2439-2446`).

**Decision:** Tenmoku builds on 0.106.0's behaviour as read here:

- v1 only;
- no auth capabilities declared;
- edits and commands reach disk only inside Claude Code;
- control through `_meta.claudeCode.options` (`settings`, `sandbox`,
  `tools`, `strictMcpConfig`, `allowDangerouslySkipPermissions`), sent on
  load and resume as on new, and through `session/request_permission`.

fe-acp refuses an adapter older than 0.106.0 (§ 4). A newer one is used, and
the smoke run (§ 12) is repeated whenever the tested version is raised.

## 3. Edits as proposals

The rule: **nothing an agent writes reaches the user's buffers or files
except as a proposal the user accepts.** § 2 shows Claude Code writes files
itself. So the rule holds only if Claude Code never runs in the user's tree.

### Claude Agent runs in a worktree lane

Each Claude Agent chat is an agent lane backed by a git worktree
(`Backing::Worktree`, agent-lanes.md § Isolation › Worktree lanes).

- **The session's `cwd` is the worktree,** never the root.
- **Edit, Write, MultiEdit, NotebookEdit and Bash write freely there.** The
  user's tree is untouched.
- **At the end of each turn** (the `session/prompt` response) the lane
  freezes a revision. It projects the revision's diff onto the user's buffers
  as `ProposedChange`s under `Origin::Ai { agent: "acp.claude-agent", session:
  <ACP session id> }`, with agent-lanes' `lane_project`. They go into a turn
  `Changeset` that the AI Changes panel shows (§ 8). Accepting is the user's
  edit, as today.
- **The worktree starts from the user's saved tree,** not from `HEAD`. It
  includes tracked changes and untracked files that are not ignored, frozen
  as an imported revision (agent-lanes' Update lane).
  - Unsaved buffer text is not in it. The composer says how many files are
    unsaved, and offers Save All.
  - Text the user attaches is sent in the prompt as an embedded resource. The
    adapter declares `embeddedContext` (`acp-agent.ts:2061`).
- **Before each prompt,** Tenmoku runs Update lane when its preconditions
  hold (newest revision decided, nothing written since). Otherwise the prompt
  goes to the lane's tree as it is.
- **What the agent reads back** is its own worktree. That holds its writes,
  decided or not. Rejected work stays there, as agent-lanes decides, and
  rejecting sends the rejection to the agent as text (§ 8). It never sees
  pending proposals in the user's buffers: it has no access to them.

### The security profile

Claude Code runs under one **security profile**, which fe-acp builds from
the current agent settings (§ 7) by one function, `claude::profile`.

**Where it is sent.** Every path that starts a query sends the whole
profile as `_meta.claudeCode.options`:

- `session/new`;
- `session/load` on reopening a chat;
- `session/resume`;
- the `session/load` after a crash or respawn (§ 4).

The adapter builds each query afresh from those parameters (§ 2), so
nothing is inherited from an earlier query, and nothing is left out.

**When it changes.** If the agent settings change while a session is open,
the profile changes. Tenmoku sends `session/load` with the new profile
before the session's next prompt, and the adapter rebuilds the query on the
changed options (§ 2). A prompt is never sent under an outdated profile.

The profile holds the following.

- **Mode and bypass.**
  - Claude Code runs in mode `default`. The mode picker offers only
    `default`, `acceptEdits` and `plan`.
  - `allowDangerouslySkipPermissions: false`, so `bypassPermissions` is
    never offered.
- **An explicit tool list.** `tools` names only the built-in tools fe-acp
  classifies (`CLAUDE_TOOLS`, a constant checked against the SDK's
  `sdk-tools.d.ts` at each tested adapter version):

  | Tools | Class |
  |---|---|
  | `Read` | read |
  | `Glob`, `Grep` | search |
  | `Edit`, `Write`, `NotebookEdit` | propose_edit |
  | `Bash` | run_command |
  | `WebFetch`, `WebSearch` | fetch |
  | `TodoWrite`, `Agent`, `EnterPlanMode`, `ExitPlanMode`, `AskUserQuestion` | none: they write nothing and run nothing themselves; a subagent's calls use the tools above, under the same rules |

  - **Every other built-in tool is unavailable.** That includes scheduling,
    remote triggers, workflows, monitors and anything a later Claude Code
    adds.
  - **A class at `deny` removes its tools** from the list.
- **Forced asks for every class not at `allow`.** `settings.permissions.ask`
  lists:
  - `Edit`, `Write`, `NotebookEdit` and `Bash`, always;
  - plus each tool of a class whose level is `ask`.

  So every such call reaches `session/request_permission` whatever the
  user's own Claude Code settings allow. Their `allow` rules can't beat an
  ask rule, and an ask is bypass-immune (§ 2).
- **How Tenmoku answers asks.**
  - **File tools:** by the path in the call (`locations`,
    `raw_input.file_path`):
    - **inside the worktree:** the `propose_edit` class (§ 7, default allow),
      since it becomes a proposal;
    - **anywhere else:** `reject_once`, always, without a prompt.
  - **A class at `allow`** isn't forced to ask. If Claude Code asks anyway
    (its own rules, or a read outside its working directory), Tenmoku
    answers by policy.
- **MCP servers are off unless the user turns them on.** They run only when
  `agents.claude_code.mcp_servers = true` **and** the class `other` is not
  `deny`. Otherwise the profile passes `strictMcpConfig: true` with no
  `mcpServers`, so no MCP server from Claude Code's settings, `.mcp.json` or
  plugins runs. When they do run, their tools are class `other`:

  | `other` level | Profile |
  |---|---|
  | `deny` | MCP servers stay off, whatever `mcp_servers` says |
  | `ask` (the default) | the ask rule `mcp__*` (Claude Code's tool-name wildcard), so every MCP call reaches `request_permission`, even one the user's own settings allow |
  | `allow` | no rule; if Claude Code asks anyway, Tenmoku allows |
- **The sandbox:**

  ```text
  sandbox: { enabled: true, failIfUnavailable: true,
             autoAllowBashIfSandboxed: false, allowUnsandboxedCommands: false,
             filesystem: { allowWrite: [ …agents.claude_code.sandbox_allow_write ],
                           denyWrite:  [ <git common dir>, <git admin dirs> ] } }
  ```

  - A command that runs can write only inside the worktree's working
    directory, plus any user-listed paths. So `touch ../outside` fails.
  - **The repository's git directories are denied explicitly, not just left
    out.** Claude Code grants a linked worktree's session write access to
    the shared repository `.git` by itself. So leaving it out of `allowWrite`
    protects nothing, and a command could change the user's refs and
    objects.
  - **How they are resolved.** At spawn, and again at each respawn and
    load, fe-acp resolves with git itself:
    - `git -C <worktree> rev-parse --path-format=absolute --git-common-dir
      --git-dir`: the common dir and the lane's own admin dir
      (`<common>/worktrees/<n>`);
    - `git -C <root> rev-parse --path-format=absolute --git-dir`: the root's
      own admin dir, which differs from the common dir when the opened root
      is itself a linked worktree (both `.git` paths are then pointer
      files).

    All three, deduplicated, go into `filesystem.denyWrite`. A deny there
    wins over any grant, Claude Code's automatic one included.
  - **Failing closed.** If git can't resolve them (git missing, a
    `rev-parse` failure, a path that doesn't exist), the session doesn't
    start (§ 4's "Sandbox loosened", naming the reason).
  - **The effect.** Git commands that write (`commit`, `add`, `stash`,
    `checkout`) fail in the lane, and read-only ones (`status`, `diff`,
    `log`) work. The lane's revisions are frozen by Tenmoku (agent-lanes
    card 18), not by the agent's commits.
  - `failIfUnavailable` makes a session refuse to start where the sandbox
    can't run (on Linux it needs bubblewrap). § 4 defines that state.
- **The chat's deny list.** `chat_context::refusal`'s list (`.env*`, `id_*`,
  `*.pem`, `*.key`, `.git` contents for reading) becomes `deny` rules:
  `Read(…)` and `Edit(…)`, anchored with Claude Code's absolute `//` form at
  the root and at the worktree.

### Inherited settings that affect confinement are checked, and fail closed

Claude Code merges its settings sources, and their lists merge rather than
replace (§ 2). So the user's own files could widen what the profile
confines.

- **The sandbox's write access is the union of:**
  - its `filesystem.allowWrite`;
  - the session's working and additional directories, which takes in
    `permissions.additionalDirectories`;
  - every path an `Edit` allow rule grants.
- **The sandbox's network allowlist is the union of** its
  `network.allowedDomains` and every domain a permission rule grants, such
  as `WebFetch(domain:…)` allow rules.
- **Socket access can escape filesystem isolation altogether.** A Docker
  socket is the usual case.

So fe-acp checks every key that bears on these, and refuses on anything it
can't prove only tightens.

#### The sources

Read before spawning, and again before every prompt. These are every
settings file Claude Code reads for a session whose `cwd` is the worktree:

| Source | File |
|---|---|
| Source | File | Loaded by the session now? |
|---|---|---|
| Managed file (Linux) | `/etc/claude-code/managed-settings.json` | yes |
| Managed drop-ins (Linux) | `/etc/claude-code/managed-settings.d/*.json`, every file | yes |
| Managed file (macOS) | `/Library/Application Support/ClaudeCode/managed-settings.json` | yes |
| Managed drop-ins (macOS) | `/Library/Application Support/ClaudeCode/managed-settings.d/*.json`, every file | yes |
| Managed preferences (macOS) | Claude Code's preference domain under `/Library/Managed Preferences/` (machine-wide and per-user), installed by a configuration profile | yes: **not inspected, fails closed** (below) |
| User | `$CLAUDE_CONFIG_DIR/settings.json`, else `~/.claude/settings.json` | yes |
| Project, in the worktree | `.claude/settings.json` | yes |
| Local, in the worktree | `.claude/settings.local.json` | yes |
| Local, in the main checkout | `<root>/.claude/settings.local.json` (Claude Code reads the main checkout's local file for a linked worktree's session) | yes |
| Project, in the main checkout | `<root>/.claude/settings.json` | no: checked ahead, because it becomes the worktree's at the next Update lane |

- **Managed preferences are not inspected.** They are a property list from
  a configuration profile, and fe-acp has no reader for that format. So if
  Claude Code's managed-preferences domain exists in either location,
  Claude Agent doesn't start. The state says so: "Claude Code has managed
  preferences (a configuration profile) that Tenmoku can't inspect".
- **Other platforms fail closed too.** Where Claude Code reads policy from a
  place fe-acp has no reader for (the Windows registry, for one), Claude
  Agent is unavailable and says why.
- **Keeping the list current.** The list, the managed-preferences domain
  name, and the drop-in directories are constants in `claude_settings.rs`,
  taken from Claude Code's managed-settings documentation ("where each
  mechanism stores the policy"). They are checked against it at each tested
  adapter version, and the smoke run (§ 12) covers each entry.
- **From each file fe-acp keeps only `sandbox`, `permissions.allow` and
  `permissions.additionalDirectories`.** Everything else is dropped after
  parsing, and never logged or kept, which matters because `env` and
  `apiKeyHelper` may hold secrets.

#### What may appear

**In `sandbox`,** only values that can only tighten:

- `enabled: true`;
- `failIfUnavailable: true`;
- `allowUnsandboxedCommands: false`;
- `autoAllowBashIfSandboxed: false`;
- `filesystem.denyWrite` and `filesystem.denyRead`;
- `network.deniedDomains`;
- `network.strictAllowlist: true` and `network.allowManagedDomainsOnly: true`;
- `network.allowedDomains`, as long as no entry is a loopback name or
  address (`localhost`, `*.localhost`, `127.*`, `::1`).

Everything else in `sandbox` is refused. That includes:

- `excludedCommands`;
- `filesystem.allowWrite` and `filesystem.disabled`;
- `enabled: false`;
- `network.allowUnixSockets`, `network.allowAllUnixSockets` and
  `network.allowLocalBinding`;
- the proxy ports and `tlsTerminate`;
- `credentials`;
- any key fe-acp doesn't know.

**`permissions.additionalDirectories`** must be absent or empty.

**`permissions.allow`** may hold no rule for `Edit`, `Write`, `MultiEdit` or
`NotebookEdit`, bare or with a path. The one exception is a rule in a
project or local file whose path is relative (no leading `/`, `//` or `~`,
and no `..`). That grants only paths under the project, which the sandbox
already allows.

**Allow rules that grant network domains** widen the sandbox's network
allowlist. Today that is `WebFetch(domain:…)`. The list of such rule kinds
is a constant in `claude_settings.rs`, checked against Claude Code's
sandboxing documentation at each tested version. Each such rule gets the
same check as an `allowedDomains` entry:

- **refused:** a loopback name or address (`localhost`, `*.localhost`,
  `127.*`, `::1`), and any wildcard that could match one (`*`);
- **refused:** a bare `WebFetch` allow, which grants every domain;
- **accepted:** any other named domain.

Allow rules for other tools grant neither paths nor domains to the
sandbox. The profile's ask rules (§ 3) still beat them.
- Tenmoku itself never adds an allow rule: it never selects `allow_always`
  (§ 7).

**A file that doesn't parse** is refused.

#### Refusing

- **At spawn or before a prompt:** the session doesn't start, or the prompt
  isn't sent.
- **The state** names the file and the key (§ 4's "Sandbox loosened").
- **The cost.** A user whose Claude Code allows `Edit` everywhere must
  narrow that rule to use Claude Agent. The state says which rule to change.

#### Settings changed during a turn

Claude Code reloads its settings files while it runs. So fe-acp also
watches every source in the table while a session is open (the workbench's
file watcher, debounced as `disk_sync.rs` is). That includes:

- the `managed-settings.d` directories, for files added, changed or
  removed;
- the managed-preferences locations, where a domain appearing is a change
  that fails.

A change that fails the check:

1. cancels the running turn (`session/cancel`, pending permission requests
   answered `Cancelled`);
2. refuses further prompts until the check passes again.

Between the write and the cancel there is a window of at most the watcher's
debounce. A command already allowed could act under the loosened settings in
it. This is the same kind of gap as the chat record's symlink-swap limit
(§ 8): it needs something already able to write the user's own settings
files. It is stated, not closed.

#### What remains

Settings an organisation pushes to Claude Code from a server, rather than a
file, can't be read by Tenmoku. Their values apply unseen. The smoke run
(§ 12) checks every file case.

### Where the rule can't hold

- **Hooks**, from Claude Code's settings or plugins, run as the user,
  outside the sandbox and outside Tenmoku's control. They can write
  anywhere, including the user's tree. Tenmoku can't intercept them (`hooks`
  can't cross JSON-RPC; `acp-agent.ts:1630-1633`).
  - Tenmoku doesn't turn them off: they are the user's Claude Code, which the
    user asked to use.
  - The first-open notice (§ 9) says so.
  - Untrusted folders spawn nothing (§ 9), so a cloned repo's own
    `.claude/settings.json` hooks never run from Tenmoku.
  - Open question 3 asks whether to pass `disableAllHooks`.
- **MCP servers, once the user turns them on** (`mcp_servers = true`, with
  `other` not `deny`), run outside the sandbox too. Each call is still
  decided by Tenmoku when `other` is `ask` (the `mcp__*` rule), but what a
  server does once a call is allowed is the server's own. They are off by
  default.
- **Commands Claude Code runs can read what the user can read**, the deny
  list's files included. The deny rules bind the `Read` and `Edit` tools,
  and the sandbox bounds writes, not reads. A program substituted for a
  trusted name (§ 7) can still write only inside the worktree.
- **Paths the user lists in `agents.claude_code.sandbox_allow_write`** are
  written directly, outside review. The user chooses them, a project file
  can't set them (§ 7), and the setting's description says so. Paths inside
  the root or its `.git` are refused there with a warning.
- **Claude Code's own state** (`~/.claude/projects/…` transcripts, todos,
  file history) is written outside the project, by design. It is not the
  user's code, and the rule doesn't cover it.
- **Folders that aren't git repositories** have no worktree. There, Claude
  Agent is unavailable and says why (§ 4). Open question 4 asks about a
  copy-based fallback.

### The API chat's tool loop

The API chat holds no write path.

- `propose_edit` (§ 6) makes proposals with `chat_apply`'s path, as Apply
  does today.
- `run_command` runs in the root and **can write the user's files**. This is
  the "permission the user grants knowing it writes":
  - the class defaults to `ask`;
  - the prompt says the command runs in the project folder and can change
    files.
- **For allowed commands the rule does not hold**, and this record says so
  plainly. A sandboxed or worktree-backed `run_command` is open question 5.

### `fs/write_text_file` for later agents

Tenmoku declares `fs.readTextFile: false`, `fs.writeTextFile: false` and
`terminal: false`. A conforming agent then never calls them, and fe-acp
answers `-32601` if one does (the crate's fallback, `src/role/acp.rs:44-60`).

An ACP agent that does use them (not in scope here) gets an overlay lane:

- `write_text_file` becomes a `lane.propose` and is answered once the
  proposal is registered;
- `read_text_file` returns the lane's text: the buffer with that lane's
  pending hunks applied (`ProposedChange::dry_run`).

That is a later card, not in § 13.

**Decision:**

- **Claude Agent:** a worktree lane under one security profile, sent whole
  on every new, load, resume and respawn. It has:
  - an explicit tool list;
  - forced asks for every class not at allow;
  - a sandbox confined to the worktree;
  - inherited settings that bear on confinement (sandbox keys, Edit allow
    rules, additional directories, sockets) checked in every source. They
    are refused if they widen it, at spawn, before each prompt, and on any
    change during a turn;
  - no MCP servers by default, `mcp__*` asks when they're on, and no bypass
    mode.

  File writes outside the worktree are always refused.
- **Hooks, opted-in MCP servers, and command reads** are the stated gaps.
- **The API chat** proposes edits, and its allowed commands write directly,
  which the user is told.

## 4. The ACP client crate

**`crates/fe-acp`**: headless, Linux-OK, and core (not in
`scripts/check-no-gpui.sh`'s `UI_CRATES`).

It holds:

- the ACP v1 client over `agent-client-protocol` 5.2.0, pinned `=5.2.0` with
  default features off. It takes neither `process` nor `stdio`: fe-acp
  spawns the process itself.
- the session model and the `AcpEvent` stream;
- the mapping from `request_permission` to a permission request (§ 7);
- the Claude Agent profile: the `_meta.claudeCode.options` of § 3, and the
  version floor;
- a fake agent for tests (§ 12).

It knows nothing of the workbench.

### Runtime

The crate's futures are `Send` (§ "Corrections"). fe-acp turns a child's
stdin and stdout into `ByteStreams` (`src/jsonrpc.rs:7252`, futures-io).

- **The workbench drives the connection future on GPUI's background
  executor** (`cx.background_spawn`), as the chat's stream pump does
  (`crates/fe-chat-view/src/view.rs:866`).
- Events reach the foreground over an async channel.
- This is not fe-lsp's runtime, which has a thread per server with
  `async_io::block_on`. No tokio runtime is involved.
- **The crate's ordering rule.** Handlers run on the dispatch loop, and
  awaiting a request inside one deadlocks (`src/concepts/ordering.rs:50`).
  So the `session/request_permission` handler hands the request and its
  responder to the workbench, and returns. The workbench answers later
  through the responder.

### Locating the programs

The workbench resolves both programs and hands fe-acp plain paths. It uses
`fe_lsp::LoginPath`, the login-shell `PATH` resolution
(`crates/fe-lsp/src/launcher.rs:453`, re-exported at
`crates/fe-lsp/src/lib.rs:153`).

| Program | Setting | Default |
|---|---|---|
| The adapter | `agents.claude_code.adapter` (a path) | `claude-agent-acp` on the login `PATH` |
| `claude` | `agents.claude_code.claude` (a path) | `claude` on the login `PATH`, passed as `CLAUDE_CODE_EXECUTABLE` so the adapter runs the user's own CLI, not the SDK's copy (§ 2) |

- **No `npx`.** It downloads and runs code from the network at spawn time.
- **Spawning,** as `launch_local` does (`launcher.rs:345-398`): the root as
  the process's working directory (sessions set their own `cwd`), its own
  process group, `kill_on_drop`, and stderr kept in a ring buffer for the
  panel's error details.
- **Environment.** The environment is inherited with the login `PATH`, plus
  `CLAUDE_CODE_EXECUTABLE`. Nothing else is added.

### What fe-acp never does

> "Unless previously approved, Anthropic does not allow third party
> developers to offer claude.ai login or rate limits for their products,
> including agents built on the Claude Agent SDK." (Agent SDK overview,
> fetched 2026-11-13)

- **It never calls `authenticate` with credentials, or at all.** It declares
  no `auth` capability, so the adapter offers it no login method (§ 2).
- **It never reads Claude Code's credentials.** That covers
  `~/.claude/.credentials.json`, `~/.claude.json`, and the macOS keychain's
  Claude Code items. It also never logs or stores `_auth/status_update`'s
  email or organisation. It keeps only its `kind`, in memory.
  - From Claude Code's files it reads only three things in its settings
    files: `sandbox`, `permissions.allow` and
    `permissions.additionalDirectories` (§ 3). The rest of those files,
    `env` and `apiKeyHelper` included, is dropped after parsing and never
    kept or logged.
- **The user logs in through Claude Code itself,** in their own terminal.
  Tenmoku offers no login button and runs no login command.
- **The API chat never uses a subscription.** It authenticates only with the
  Anthropic API key from the keychain or `ANTHROPIC_API_KEY` (`x-api-key`;
  chat record § 7). It never takes an OAuth token from Claude Code or anywhere
  else. fe-acp never puts the chat's key into the agent's environment.
- **Which account pays.** The account Claude Code is logged into pays for
  Claude Agent: the user's subscription or API key, as set up in Claude Code.
  The user asked for exactly this, for their own use.
  - Whether a Tenmoku shipped to other people must pass `--hide-claude-auth`
    is **open question 1**. It blocks any such distribution.
  - fe-acp's launcher takes the flag as a parameter (`hide_claude_auth:
    bool`), so the answer is a one-line change.

### Session lifecycle

| Stage | What happens |
|---|---|
| **Spawn** | One adapter process per window per root, started at the first Claude Agent send or resume in that window. Only in a trusted folder that is a git repository (§ 9). |
| **Initialize** | Once per process, v1. Capabilities: `fs: { readTextFile: false, writeTextFile: false }`, `terminal: false`, no `auth`. `agentInfo.version` below 0.106.0 → the "adapter too old" state. |
| **New** | One ACP session per Claude Agent chat. `cwd` is the chat's lane worktree, with the whole security profile of § 3. The response's config options seed the header picker (§ 5). |
| **Resume** | `session/load` with the saved session id, the lane's worktree as `cwd`, and the whole current security profile, built fresh. The adapter replays the history as updates. |
| **Prompt** | First, the inherited-settings check of § 3. Then, if the profile changed since the session's last load, a `session/load` with the new one. Then the user's text plus attached resources. The turn ends with the prompt response's stop reason. |
| **Cancel** | Stop sends `session/cancel` and answers every pending permission request `Cancelled`. The turn ends when the prompt response arrives. |
| **Settings watch** | While a session is open, a change to any Claude Code settings file of § 3 that fails the check cancels the running turn (as Cancel) and refuses further prompts until it passes. |
| **Crash** | The process exits or the connection errors: every session is marked disconnected and its lane Abandoned (agent-lanes § Lifecycle). The next send respawns and `session/load`s with the whole current profile, which reopens the lane. There is no automatic respawn loop. |
| **Shutdown** | Window close or root change: `session/close` for each session, stdin closed, the process group killed after 2 s. |

### States the panel shows

Each state is shown in place of the composer's send. None is a toast.

| State | When | Text and actions |
|---|---|---|
| **Adapter not installed** | The adapter isn't found | "Claude Agent needs the ACP adapter. Install it with `npm install -g @agentclientprotocol/claude-agent-acp`, or set `agents.claude_code.adapter`." Copy command, Retry. |
| **Claude Code not installed** | `claude` isn't found | "Claude Agent runs your installed Claude Code, and `claude` wasn't found on your login shell's PATH. Install it, or set `agents.claude_code.claude`." |
| **Not logged in** | `authRequired` on `session/new` or a prompt, or `_auth/status_update` with `kind: "none"` | "Claude Code isn't logged in. Run `claude /login` in a terminal, then Retry." Copy command, Retry. The refused prompt stays in the composer. |
| **Adapter too old** | `agentInfo.version` below 0.106.0 | "Update the Claude Agent adapter to 0.106.0 or later." |
| **Sandbox unavailable** | Session start fails on `failIfUnavailable` | "Claude Code's sandbox isn't available on this system (on Linux it needs bubblewrap), so Claude Agent can't run commands safely here." The adapter's message is in the details. |
| **Sandbox loosened** | The inherited-settings check (§ 3) found a widening key, a settings file that doesn't parse, a source it can't inspect (macOS managed preferences), or git couldn't resolve the directories to deny | "Your Claude Code settings loosen its sandbox (`<key>` in `<file>`), so Claude Agent won't run. Remove it, or run Claude Code outside Tenmoku." For an uninspectable source or a git failure, that reason instead. Retry re-checks. |
| **Not a git repository** | The root has no git repository | "Claude Agent works in a git worktree, and this folder isn't a git repository." |
| **Disconnected** | The process exited | "Claude Agent stopped." With stderr's last lines in the details, and Retry. |

**Decision:**

- `crates/fe-acp`, headless, on GPUI's background executor;
- programs from the login `PATH` or two path settings, never `npx`;
- no `authenticate`, no credential reads, no subscription use by the API
  chat;
- the states above.

## 5. Chat panel and agents

### The Agent choice

- **Where.** A control in the chat header, beside History and New Chat:
  **Claude (API key)** or **Claude Agent**. New chats default to the last one
  used in the window. Claude Agent appears only in trusted git folders.
- **Fixed per chat.** A chat's agent is set at its first send and never
  changes; New Chat switches.
- **Separate histories.** The two keep separate API histories. Nothing moves
  between them, so neither prefix is edited.

### How ACP updates render in fe-chat-view

They reuse the tool loop's rows (§ 6, card AG9):

| Update | Rendered as |
|---|---|
| `agent_message_chunk` | Reply text, as an API reply's text |
| `agent_thought_chunk` | A collapsed "Thinking" block |
| `tool_call` / `tool_call_update` | A tool row: kind icon, title, status (pending, running, done, failed), and its locations as file links into the lane's worktree, shown root-relative. A `Diff` shows as a compact read-only diff under the row; it is information, and the review is the lane's (§ 8). Execute rows show their output when the update carries it. |
| `plan` | A checklist block, updated in place |
| `current_mode_update`, `config_option_update` | The header picker's state |
| `session_info_update` | The chat's title |
| `available_commands_update`, `usage_update`, `notice`, compaction and subagent updates, unknown variants | Ignored, with a debug log. Slash commands and usage are out of scope (§ 14). |

- **Permission requests** render as a row in the transcript (§ 7).

### Persistence and resume

- **Where.** A Claude Agent chat is saved in the same place as API chats:
  `<state_dir>/chats/<layout_key(root)>/<id>.json`, 0600, written after each
  finished turn by `chat_store_worker`.
- **What.** A new `kind: "acp"` record holds: the agent id, the ACP session
  id, the lane number, the worktree path, the title, and the rendered
  transcript.
  - Existing files have no `kind` and load as API chats.
  - History lists both kinds.
- **Reopening** shows the saved transcript at once, then `session/load`s it.
  The adapter's replay replaces the transcript when it finishes.
- **When the session can't be loaded** (Claude Code's transcript is gone, or
  the lane was cleared), the saved transcript stays, read-only, with "This
  Claude Agent session can't be resumed."
- **Untrusted folders** never have Claude Agent chats (§ 9), so there is
  nothing to keep memory-only.

### The model and effort picker (T604)

- **For Claude Agent, the picker lists what the adapter offers,** not
  `chat_models::KNOWN`:
  - the `model` config option's values;
  - the `effort` option's (category `thought_level`);
  - and a mode row limited to `default`, `acceptEdits` and `plan` (§ 3).
- A pick sends `session/set_config_option { configId, value }` (or
  `session/set_mode` for the mode).
- `config_option_update` and `current_mode_update` keep the header in step.
- A pick applies from the next prompt. Claude Code keeps its own history, so
  Tenmoku has no append-only concern here.

**Decision:**

- an Agent choice in the header, fixed per chat;
- ACP updates rendered with the tool loop's rows;
- `kind: "acp"` chat records resumed with `session/load`;
- the picker driven by the adapter's config options.

## 6. The API tool loop

The built-in chat (Claude through the API key) gets five client tools in
trusted folders. With them it reads, searches and proposes edits itself, and
runs commands behind permissions.

### The tool set

The tools are declared at a conversation's first send, sorted by name. All
are `strict: true`, so every object has `additionalProperties: false` and
lists every property in `required`, using `null` for "not given". All have
`eager_input_streaming: true`, since the request streams.

Every path is root-relative, resolved through `chat_apply::locate` (inside
the root, symlinks followed and checked), and refused by
`chat_context::refusal`'s deny list.

| Tool | Input schema | Output cap | Touches |
|---|---|---|---|
| `list_files` | `{ "dir": string \| null, "glob": string \| null }` | 1000 paths, then `[truncated: N more]` | quick open's file list under `dir`, ignored files excluded, deny list applied |
| `propose_edit` | `{ "path": string, "edits": [ { "old_text": string, "new_text": string } ] }`; `edits` non-empty; one edit with `old_text: ""` means the whole file (a new file if it doesn't exist) | Result text only: `Proposed N changes to <path>; the user will review them.` or the error | a proposal on the file's buffer, never the buffer or disk |
| `read_file` | `{ "path": string, "start_line": integer \| null, "end_line": integer \| null }` | 2000 lines or 64 KiB, then `[truncated: N more lines; read from line M]` | the open buffer's text (unsaved edits included), else the file; refuses binary and over 1 MiB, as @file does |
| `run_command` | `{ "program": string, "args": [string], "cwd": string \| null }`; argv, **no shell** | 32 KiB of interleaved stdout/stderr: the first 8 KiB and the last 24 KiB, with `[N bytes omitted]`; then `exit N` or `killed after T s` | a process in the root (or `cwd` inside it); can write the user's files (§ 3) |
| `search` | `{ "pattern": string, "regex": boolean, "case_sensitive": boolean, "glob": string \| null }` | 200 matching lines, each with its 512-byte preview, 64 KiB in all, then `[truncated]` | `fe_project::search_filtered` over the root (`crates/fe-project/src/search.rs:503`), deny list as its `PathFilter`, cancellable |

Details:

- **One pending proposal per file, per chat, across turns.** The chat holds
  at most one live `ProposedChange` per file, whichever turn made it, in a
  map kept by the loop (`ChatProposals`, keyed by the file's canonical
  path).
- **The chat's view of a file** is the buffer's text with that one
  proposal's **pending** hunks applied (`ProposedChange::dry_run`).
  - Accepted hunks are already in the buffer.
  - Rejected and stale hunks are left out.
  - A pending new file's view is its proposed text.
- **`read_file`** returns the chat's view. It is headed
  `[includes N pending changes you proposed]` when N > 0, and
  `[M of your earlier changes no longer apply]` when hunks went stale, so
  the model neither repeats a change nor builds on a lost one.
- **`propose_edit`** follows the chat record's § 8 rules:
  - each `old_text` must match exactly once in the chat's view of the file;
  - the edits are validated as a set first (no overlaps, no two insertions
    at one offset), since `from_edits` panics on both.
- **Supersession.** A valid edit set makes the new view: the old view with
  the edits applied. Then:
  1. the file's earlier proposal, from this turn or an earlier one, has its
     still-pending hunks rejected as **superseded**. The rows say "replaced
     by a later change", and the earlier turn's changeset counts them as
     decided;
  2. one new proposal is made by `ProposedChange::from_text` from the buffer
     to the new view. It carries the earlier pending work and the new edits
     together, so every pending hunk of the file belongs to one proposal in
     one turn;
  3. that proposal goes into the current turn's `Changeset`.

  So turn 1's a→b, then turn 2's b→c before review, gives one pending a→c
  in turn 2, never two overlapping proposals.
  - A pending new file is superseded the same way: the new text replaces
    the earlier turn's `add_new_file` entry.
  - An edit set that doesn't validate changes nothing, and the earlier
    proposal stays as it was.
- **Limits come from the schemas' descriptions and Tenmoku's own checks,**
  not from the schemas: strict schemas don't support `minimum`, `maxLength`
  and the like.
- **Inputs are validated** by deserialising them into typed Rust structs
  (`#[serde(deny_unknown_fields)]`), strictly, at `content_block_stop`. A
  failure is answered `is_error: true` with `{"INVALID_JSON": <the raw
  input>}`, built with serde_json, and runs nothing.
- **Untrusted folders** declare no tools (§ 9).

### Wiring

- **fe-chat.**
  - `ToolSpec { name, description, input_schema, strict }`
    (`crates/fe-chat/src/provider.rs:319`) gains `eager_input_streaming:
    bool`.
  - `ChatRequest.tools` (`:32`), today hard-coded empty at
    `request.rs:103`, is the conversation's frozen tool set.
  - `ToolChoice` stays `Auto` only: forced `any`/`tool` is a 400 on Claude
    Opus 5.5. So `tool_choice` is omitted, as `wire.rs:22` already does.
  - `ToolCall { id, name, input }` (`provider.rs:341`) is what a finished
    `tool_use` block (`ChatEvent::BlockFinished`, `BlockKind::ToolUse`)
    becomes.
  - A new `ToolResult { tool_use_id, content: String, is_error: bool }`
    renders as `{"type":"tool_result","tool_use_id":…,"content":…,"is_error":…}`.
  - The tool set and schemas are in a new `crates/fe-chat/src/tools.rs`,
    with constant text and no per-workspace content.
- **fe-chat-anthropic.**
  - `request_body` (`wire.rs:25`) already serialises tools (`:67-77`). It
    adds `eager_input_streaming`.
  - `input_json_delta` is already decoded (`events.rs:200-208`), and a
    `tool_use` block's input is already parsed strictly at stop, kept raw if
    invalid (`events.rs:247-286`).
  - `tool_result` content blocks are new (nothing handles them today).
  - With a task budget, the body carries
    `output_config.task_budget: {"type": "tokens", "total": N}` and the
    header `anthropic-beta` adds `task-budgets-2026-03-13`. It is
    comma-joined with the fallback beta and T604's effort beta.
- **One round.** A round is one request. The reply is streamed and kept
  verbatim.
  - If `stop_reason` is `tool_use`, every finished `tool_use` block is
    validated, decided (§ 7) and run. Independent calls run concurrently.
  - **All** results go back as `tool_result` blocks in **one** user message,
    in the order of the calls. A denied or failed call still gets its result,
    with `is_error: true`.
  - A reply with `stop_reason` `max_tokens` or `refusal` runs none of its
    tools (chat record § 2). What happens to the history is per round (next
    section).

### History stays append-only

`Turn` (`crates/fe-chat/src/conversation.rs:148`) holds one user message and
one reply today. It becomes the user message plus `rounds: Vec<Round>`, each
`Round { reply: Vec<ReplyBlock>, results: Vec<ToolResult> }`.

- **What `history()` emits per turn:**
  1. `user`: the turn's user content;
  2. for each round, `assistant` (the reply's blocks, verbatim, thinking and
     tool_use included) and, if the round called tools, `user` (its
     `tool_result` blocks).
- **Nothing earlier is re-rendered.** The tool set, system prompt and every
  earlier block stay byte-identical. The cache prefix holds round to round,
  and preserved thinking's history check passes (chat record § 4: accounts
  created on or after 2026-08-31 are enforced).
- **Append-only means at block level:** no block already sent is ever
  changed, moved or removed. New blocks only go after the last one.
- **Refusal and failure are per round.** Rounds before the one that ended
  badly were sent, so they always stay, results included. Only the bad
  round's own reply is affected:

  | How the round's reply ended | Its reply in history |
  |---|---|
  | `refusal` (before or mid-stream) | Dropped: the partial is discarded, as the docs require |
  | Failed (a provider error) | Dropped. **Retry** sends the identical request again |
  | `max_tokens` | Kept, marked cut off. Its tool calls are answered `is_error` ("cut off at the token limit") |

  A turn whose **first** round is refused or fails leaves the history as
  today: the user edits and resends. A later round's refusal ends the turn
  with the earlier rounds in place, and the turn's state says where it
  stopped.
- **When the next user text joins the last user message.** If a turn ends
  with a user message last, its text goes in as a new `text` block appended
  after that message's `tool_result` blocks. That is:
  - a tool_result message not yet sent (stopped mid-tool, or at the round
    limit);
  - or one that was sent but whose reply was dropped (a later round
    refused).

  Every block that was sent stays byte-identical, and the request still
  alternates user and assistant. The cached prefix extends to the earlier
  blocks, and no thinking block follows the changed message, so
  preserved thinking's check is unaffected.
- **Saved chats from before this card** load with their one reply as one
  round (`#[serde(default)]`).
- **This card builds on T604's `Turn` changes** (`asked_model`,
  `asked_effort`).
- **The tool set is fixed at a conversation's first send.** A chat started
  without tools (untrusted, or `agents.tool_loop = false`) stays without
  them.
  - Folder trust revoked mid-chat: the tools are withdrawn with a
    `tool_removal` block in an appended `role: "system"` message (beta
    `mid-conversation-tool-changes-2026-07-01`), and any later call is
    answered `is_error`. What happens to a turn running at that moment is
    below (§ "Trust withdrawn mid-turn").
  - Classes set to `deny` keep their tool declared, and are answered "Denied
    by your settings", so the cached tool list never changes.

### Loop limits

- **Rounds per turn:** `agents.max_rounds` (default 25). When a reply would
  start round 26, its tool calls are answered `is_error` with "The round
  limit was reached; the user can continue". The turn ends with **Continue**,
  which sends that message.
- **Task budget:** `agents.task_budget` tokens, default 0 (off). When it is
  set, it must be at least 20000.
- **Wall time:** `agents.turn_time_limit_secs` (default 600). The loop
  doesn't start a new round after it, and ends as at the round limit.
- **Each command:** `agents.command_timeout_secs` (default 120). Then
  SIGTERM to its process group, SIGKILL 2 s later, and the result says it
  was killed.

### Stop and cancel mid-tool

Stop drops the stream, which stops generation (chat record § 2).

- **A reply still streaming** keeps only its finished text blocks, as a
  stopped turn does today. Its tool calls never ran.
- **Tools still running:**
  - commands' process groups are killed (as above);
  - searches are cancelled through their `AtomicBool`;
  - pending permission rows close as refused.
- **Every call of the last round gets a result:** its real one if it
  finished, else `is_error: true` "Stopped by the user". The history stays
  valid and append-only, and the next user text joins that message (above).

### Trust withdrawn mid-turn

Every turn records the root and its **trust generation** when it starts.
The generation is a counter the loop bumps whenever the root's trust
changes or the window's root changes.

**Before anything lands,** every tool result, proposal and permission
answer is checked against the turn's generation. That covers a result
being added to the round, a proposal being registered, and a reply being
sent. Work from an older generation never lands.

**When trust is withdrawn, or the root changes, during a turn:**

1. The stream is dropped, which stops generation.
2. Running commands' process groups are killed, searches are cancelled, and
   pending permission rows close as refused.
3. A **finished `propose_edit`** keeps its result. Its proposal is already
   registered and stays, since a proposal is inert.
4. **Every other call of the round,** finished or not, is answered
   `is_error: true`, "Folder trust was withdrawn".
   - Finished reads, searches and listings have their content discarded,
     never sent.
   - Commands have their output discarded.
5. Nothing is sent. The turn ends stopped.
6. **The next send** puts the user's text after those results, in the same
   message (above). Then it appends the `tool_removal` system message last:
   a mid-conversation system message must follow a user message and be last
   or followed by an assistant turn. After that the chat has no tools.

A command that was already running can have written files before it was
killed. Allowing it was the user's choice (§ 3). The audit trail (§ 7)
records the kill.

### What the user sees

Each call is a tool row in the transcript (fe-chat-view, card AG9):

| Call | Row |
|---|---|
| `read_file` | "Read `src/lib.rs` rows 1–200" |
| `search` | "Searched `foo` · 12 files" |
| `list_files` | "Listed `src/`" |
| `propose_edit` | "Proposed 3 changes to `src/x.rs`" with Review |
| `run_command` | "Ran `cargo test` · exit 0 · 3.2 s", with its output folded |

- **Status** shows pending, running, done, failed or denied.
- **A call waiting for permission** is the prompt row of § 7.
- **The turn's proposals** collect in the AI Changes panel (§ 8).

**Decision:**

- **Who gets the loop:** on by default (`agents.tool_loop = true`), in
  trusted folders only. `run_command` asks by default.
- **Turn shape:**
  - rounds inside a turn, with all of a round's results in one message;
  - append-only at block level, and refusal and failure handled per round;
  - frozen tools, with `tool_removal` only on a trust change;
  - trust generations, so nothing lands after trust is withdrawn.
- **Proposals:** one pending proposal per file per chat, superseded across
  turns.
- **Limits:** 25 rounds and 600 s per turn; the task budget is off by
  default.

## 7. Permissions

This section folds T1569's sections:

- what a permission covers;
- levels;
- scope and trust;
- setting shape;
- the audit trail.

### What a permission covers

A **request** is a class, plus a target.

| Class | API chat tool | Claude Agent tools (§ 3) | Target |
|---|---|---|---|
| `read` | `read_file`, `list_files` | `Read` | path |
| `search` | `search` | `Glob`, `Grep` | pattern |
| `propose_edit` | `propose_edit` | `Edit`, `Write`, `NotebookEdit` (inside the worktree) | path |
| `run_command` | `run_command` | `Bash` | program and args (Claude Agent: the command string) |
| `fetch` | — | `WebFetch`, `WebSearch` | URL or query |
| `other` | — | MCP tools, when turned on (§ 3) | the tool's title |

A Claude Agent request is classed by its tool name (`raw_input` and the
title name it). An unknown name falls back to the ACP `ToolKind`, and else
to `other`.

**How each level is enforced for Claude Agent.** Claude Code asks only when
its own rules say so (§ 2). So Tenmoku's levels are made binding through
the security profile (§ 3), not by waiting for an ask:

| Level | Enforced by |
|---|---|
| `deny` | The class's tools left out of `tools`: unavailable, never called |
| `ask` | The class's tools put in the flag-settings `ask` rules: every call reaches `request_permission`, and Tenmoku prompts |
| `allow` | Nothing forced. If Claude Code asks anyway, Tenmoku allows |

Per-pattern rules (`commands_*`) and remembered answers apply when Bash
asks, which it always does (§ 3).

`other` covers MCP tools, whose names are unknown in advance. It is
enforced with Claude Code's `mcp__*` wildcard (§ 3):

- **`deny`:** MCP servers stay off, whatever
  `agents.claude_code.mcp_servers` says.
- **`ask`:** `mcp__*` is an ask rule. Every MCP call prompts, even one the
  user's own Claude Code settings allow.
- **`allow`:** no rule.

MCP servers run at all only with `mcp_servers = true`.

### Levels

A level is `allow`, `ask` or `deny`. **The most restrictive applicable rule
wins:** deny, then ask, then allow.

- `commands_deny` and `commands_ask` patterns make a matching command deny
  or ask.
- `commands_allow` patterns allow a command whose class is `ask`. They never
  override a deny.
- Hard rules, which no setting changes:
  - ACP file writes outside the worktree are denied (§ 3);
  - deny-list paths are denied;
  - in an untrusted folder, nothing exists to ask about (§ 9).

### Command patterns

A pattern is words matched against argv:

- `cargo` matches any `cargo` command;
- `cargo test` matches argv starting `cargo test`;
- a trailing `*` word matches anything after it.

**Programs are bound to what they resolve to,** not to their file name, so
a repository can't put its own `git` in front of an approval.

- **API `run_command`.**
  - **A bare program name** (no `/`) is resolved on the login `PATH`
    (`fe_lsp::LoginPath`). Two kinds of `PATH` entry are skipped:
    - relative entries, empty ones included;
    - entries inside the root or any of the window's folders.

    The match is a regular, executable file, symlinks followed to their
    real path. The executor runs **exactly that resolved path**, never the
    name again.
  - **Allow patterns and remembered answers match only bare names that
    resolve this way.** A remembered answer records the resolved real path,
    and applies only while the name still resolves to it. A different
    resolution asks again.
  - **A program given with a path** (`./build.sh`, `target/debug/app`,
    `/usr/bin/git`) matches no allow pattern and no remembered answer. It is
    `ask` every time, or denied by a deny pattern on its file name or the
    class.
  - The prompt and the audit show the resolved path.
- **Claude Agent.** Commands are shell strings, which Claude Code's shell
  resolves; Tenmoku can't see that resolution.
  - A command with shell syntax (`;`, `&&`, `||`, `|`, `` ` ``, `$(`,
    redirections, variable expansion) matches no allow pattern.
  - So does one whose first word contains a `/`.
  - Either is `ask` unless a deny pattern matches its first word, or the
    class is `deny`.
  - A bare first word matches allow patterns and remembered answers by name.
    A substituted program then still runs inside the sandbox, so it can
    write only inside the worktree (§ 3). That bound, not name resolution,
    is what protects the user's tree here.
- **API `run_command` has no shell** (§ 6), so its patterns match argv
  exactly.

### One model for both agents

`fe_chat::permissions::Policy::decide(&Request) -> Level` is used by the API
loop's executor and by fe-acp's request handler alike.

- **ACP options.** The ACP outcome is chosen from the options the adapter
  offered:

  | Tenmoku's decision | Option selected |
  |---|---|
  | allow (by policy, or by the user's Allow once or Allow for this chat) | the `allow_once` option |
  | deny | `reject_once` |
  | cancelled | `Cancelled` |

  - **Tenmoku never selects `allow_always`.** It writes rules into Claude
    Code's own settings files (§ 2). "Allow for this chat" is Tenmoku's
    memory instead.
  - **A request with no `allow_once` option** (for example
    `ExitPlanMode`'s mode choices) shows its options as offered, and the
    user's pick is sent as is. It is class `other`.
- **The prompt** is a row in the chat transcript, under the tool row it is
  about, never a toast. It holds:
  - the class and target, and for a command its working directory;
  - for API `run_command`, "This runs in your project folder and can change
    files";
  - **Allow once**, **Allow for this chat**, **Deny**.
  - Keyboard: Enter allows once, Escape denies.
  - An unanswered prompt keeps the turn waiting. Stop closes it as refused.
- **Remember-for-session.** "Allow for this chat" holds, in memory for the
  chat, either:
  - the class, for non-command classes; or
  - the command's program plus first argument, as an allow pattern. For
    the API chat the program is bound to its resolved real path, as above.

  It is gone when the chat or window closes. It never overrides a deny.

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
- **If the user's own file sets `agents.project_permissions`,** it can only
  tighten, so it is harmless.

### Grants and plugin authorisation

`kiln_command::Grants` are the ceiling the app gives an agent. Permissions
are the user's decision per call inside it.

- **API chat.** The built-in chat's grants (`CHAT_CAPABILITIES`,
  `crates/fe-workbench/src/chat_panel.rs:108`) gain `process.spawn:*` for
  `run_command`. Every run checks `Grants::allows(process.spawn:<program>)`
  (`kiln/crates/kiln-command/src/capability.rs:344`) and then the policy.
  Each executed tool call is an `Invocation` with `Actor::Ai { agent:
  "fastedit.chat", session: <chat id> }`, `Source::Script` (chat record
  § 9).
- **Claude Agent.** It is granted `process.spawn:claude-agent-acp` and
  `process.spawn:claude`, named by the resolved programs' file names, checked
  at spawn. It holds no `buffer.write` (agent-lanes § What a protocol must
  give).
- **What grants don't cover.** Grants bind what Tenmoku does on an agent's
  behalf, not what Claude Code does inside its own process. That is bounded
  by § 3's worktree, sandbox and ask rules.
- **Plugin authorisation** (`authorize.rs`) is unchanged and separate.

### Audit trail

Every decided request is one JSON line in
`<state_dir>/agents/<layout_key(root)>/audit.jsonl` (file 0600, directory
0700). Each line holds:

- the time;
- the agent (`fastedit.chat` or `acp.claude-agent`) and the chat or session
  id;
- the class and target (a path, pattern, or argv/command; never file
  contents);
- the decision and who decided: `policy`, `rule:<pattern>`, `user`,
  `session` or `hard`;
- for commands, the resolved program path (API chat) and the exit code, or
  "killed" with the reason (timeout, stop, trust withdrawn).

The log is written by a worker in `chat_store_worker`'s pattern, and rotated
at 1 MiB with one `.1` kept. `agent.show_audit_log` opens it read-only.
Untrusted folders write nothing, and have nothing to log anyway (§ 9).

**Decision:**

- six classes, three levels, strictest wins;
- one `Policy` for both agents. For Claude Agent its levels bind through the
  profile's tool list and ask rules, and `allow_always` is never selected;
- commands bound to their resolved program (API chat), and sandbox-bounded
  (Claude Agent);
- prompt rows in the chat;
- the settings above, which a project can only tighten through
  `agents.project_permissions`;
- grants as the ceiling;
- a JSONL audit trail per root.

## 8. Review

This section folds the parts of T1568 that this design needs.

### How proposals reach review

- **API chat.** Each `propose_edit` in a turn adds to that turn's
  `Changeset` (`crates/fe-editor/src/changeset.rs:221`; `add_file` `:240`;
  `add_new_file` `:282`). Files with no editor go through
  `ai_changes_buffers.rs`.
  - When the turn ends with proposals, the workbench calls
    `Workbench::set_changeset` (`ai_changes_panel.rs:679`), and the AI Changes
    panel shows the turn as Review Changes does today.
  - Proposals on open files also show in their editors
    (`EditorView::set_proposals`).
  - Accept, Reject, Accept All, Reject All and Revert Turn work unchanged.
- **Claude Agent.** At each turn end, the lane's frozen revision is
  projected into a `Changeset` (agent-lanes card 18) and set the same way.
  The panel shows the newest turn, from either agent, with the agent's name
  in its header.

### Sending feedback back

The panel gains **Request changes**: a text box under the header, and a
per-hunk Comment that adds `path:rows` and the comment to that text.

- **Sending it** becomes the agent's next instruction:
  - **API chat:** the chat's next user message, with the rejected hunks and
    comments quoted.
  - **Claude Agent:** a `session/prompt` with the same text. Stale hunks are
    sent with the user's current text.
- Both go through `LaneStore::instruct` once agent-lanes card 6 lands. Until
  then they go straight to the chat panel.

### What waits for T1568's fuller panel

- review across several lanes at once;
- viewed-file marks;
- progress while an agent runs;
- comments delivered mid-run (steering);
- the side-by-side lane diff (agent-lanes card 15).

**Decision:**

- both agents' turns land in today's AI Changes panel through
  `set_changeset`;
- Request changes and hunk comments become the next prompt;
- the rest of T1568 is later work.

## 9. Trust and privacy

- **Untrusted folders spawn no agent.** Claude Code in a folder would load
  the repo's `CLAUDE.md`, `.claude/settings.json`, its hooks and its MCP
  servers. In an untrusted folder Tenmoku spawns nothing, and Claude Agent
  isn't offered.
  - Trust is the workbench's folder trust: `kiln_shell::trust::decided`, as
    `folder_trusted` reads it (`chat_panel.rs:1969`), and as fe-lsp gates its
    servers (`crates/fe-workbench/src/lsp.rs:1184-1201`).
  - A folder that loses trust has its running prompts cancelled. Pending
    permission requests are answered `Cancelled`, then the adapter shuts
    down (as § 4) and its lanes are abandoned.
- **The API loop has no tools in untrusted folders.** Chats there declare no
  tools, and send only what the user attaches (chat record § 10). They stay
  memory-only. A turn running when trust is withdrawn is ended as § 6's
  "Trust withdrawn mid-turn" says: nothing gathered under the old trust is
  sent.
- **The first-open notice for Claude Agent** is shown once, and dismissal is
  remembered in the state dir. It says:
  - What you send, and what Claude Code reads in this folder's worktree, goes
    to Anthropic under the account your Claude Code is logged into (your
    Claude subscription or API key, as you set it up in Claude Code).
  - Claude Code's own settings, `CLAUDE.md` and hooks apply. Hooks run
    outside Tenmoku's control and can change files anywhere.
  - MCP servers are off unless you turn them on
    (`agents.claude_code.mcp_servers`). Then they run outside Tenmoku's
    control too.
  - Settings that loosen Claude Code's sandbox stop Claude Agent from
    starting.
  - Claude Agent's work happens in a separate git worktree and reaches your
    files only through review.
  - Commands run in Claude Code's sandbox, and each one is asked about as
    your agent settings say.
- **What leaves the machine through Tenmoku** for Claude Agent: the
  prompts and attachments it sends to the adapter. Claude Code sends them
  on. Tenmoku keeps no copy beyond the chat record of § 5.
- **Grant strings for a spawned agent:** `process.spawn:claude-agent-acp`
  and `process.spawn:claude` (§ 7). The agent gets no `buffer.write`, no
  `fs.write:*` and no `net:*` from Tenmoku. Claude Code's network use is its
  own.

**Decision:**

- untrusted folders: no agent process and no tools;
- the notice above;
- named `process.spawn` grants only.

## 10. Relation to agent-lanes.md

- **The ACP agent is `LaneKind::External { host: "acp" }`** with
  `Backing::Worktree`.
  - The workbench makes it when a Claude Agent chat starts. It doesn't come
    through `lane.open`.
  - Agent `acp.claude-agent`, name "Claude Agent" (from `agentInfo.title`).
  - Its `sessions` hold the ACP session id.
- **How ACP gives what agent-lanes § What a protocol must give requires:**

  | Requirement | ACP |
  |---|---|
  | 1. id and name | the app's id and `agentInfo` |
  | 2. a session | the ACP session id |
  | 3. edits as proposals | the worktree's frozen revisions, since there is no `lane.propose` path (§ 3) |
  | 4. turn boundaries | the `session/prompt` response |
  | 5. a way back | `session/prompt` (§ 8) |
  | 6. liveness | the process exiting or the connection failing |

- **Needed first:**

  | Lanes card | What it gives |
  |---|---|
  | 1 | the headless lane model |
  | 3 | detached hunks |
  | 4 | git worktrees and revisions |
  | 5 | the projection |
  | 6 | chat as lane 1, and `LaneStore` |
  | 7 | ownership and the record |
  | 8 | one buffer per path |
  | 18 | worktree revisions and the ledger. It adds "an ACP prompt response" to the turn ends that freeze a revision |

- **Done without:**
  - cards 0 and 9–17 (colour tokens and surfaces: wanted, not blocking);
  - card 19 (terminal lanes);
  - card 20 (the WIT seam).
- **The API tool loop needs no lanes card.** It is lane 1's work once card 6
  lands, and reaches review through `set_changeset` before then.

**Decision:**

- Claude Agent is an External worktree lane, and needs lanes cards 1, 3–8
  and 18 before AG11;
- the API loop needs none.

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

## 12. Testing

No network and no real Claude Code in CI.

### The fake ACP agent

`crates/fe-acp/src/bin/fe-acp-fake-agent.rs` is built with the crate's
`Agent.v1()` (`src/role/acp.rs:312`; `examples/simple_agent.rs` is the
pattern). It plays a JSON script given as its argument:

- updates to send;
- permission requests, and the option it expects back;
- `authRequired` on new or prompt;
- a stop reason;
- exiting mid-turn.

It records what it received.

- In-process tests run the same agent over `Channel::duplex()`
  (`src/jsonrpc.rs:7380`).
- Process tests (spawn, crash, shutdown, stderr) run the binary through
  `CARGO_BIN_EXE_fe-acp-fake-agent`.
- fe-acp's tests assert:
  - the `initialize` capabilities;
  - the security profile of § 3, as JSON, **byte-identical** on
    `session/new`, `session/load`, `session/resume` and the load after a
    crash;
  - a settings change that changes the profile sends `session/load` with the
    new one before the next prompt;
  - each class level mapping: a `deny` class's tools absent from `tools`, an
    `ask` class's tools in the ask rules;
  - the inherited-settings check, over a fake home and a fake main checkout
    and worktree. Each accepted key passes, and each widening case refuses
    with its file and key named. The widening cases are:
    - in `sandbox`: `excludedCommands`, `filesystem.allowWrite`,
      `filesystem.disabled`, `enabled: false`,
      `allowUnsandboxedCommands: true`, `network.allowUnixSockets` (a Docker
      socket), `network.allowAllUnixSockets`, `network.allowLocalBinding`, a
      loopback `allowedDomains` entry, and an unknown key;
    - `permissions.additionalDirectories`;
    - `permissions.allow` with bare `Edit`, `Edit(//etc/**)`, `Edit(~/x)`,
      `Write(../x)`, and a relative `Edit(src/**)` in the **user's** file.
      The same relative rule in a project file is accepted;
    - `permissions.allow` network grants: `WebFetch(domain:localhost)`,
      `WebFetch(domain:127.0.0.1)`, `WebFetch(domain:*)` and a bare
      `WebFetch` are refused, and `WebFetch(domain:docs.rs)` is accepted;
    - each case placed in each source of § 3's table: the main checkout's
      `settings.local.json`, and a file in a fake `managed-settings.d`
      directory, included;
    - a managed-preferences domain present in the fake macOS location
      refuses, naming the source as uninspectable;
    - a file that doesn't parse.

    Keys outside the three it reads, such as `env`, are never kept;
  - a settings file changed to a widening value mid-turn cancels the turn,
    answers pending requests `Cancelled`, and refuses the next prompt. So
    does a widening file added to `managed-settings.d`;
  - the git directories in `filesystem.denyWrite`, over temporary
    repositories:
    - for a main-checkout root, the common dir and the lane's admin dir;
    - for a root that is itself a linked worktree, those plus the root's
      own admin dir;
    - a `rev-parse` failure refuses the session;
  - with `mcp_servers = true`: `other = "ask"` puts `mcp__*` in the ask
    rules and `strictMcpConfig` is off; `other = "deny"` sets
    `strictMcpConfig: true` despite the opt-in;
  - `Cancelled` answers after cancel;
  - the state for each failure in § 4's table.

### The scripted provider for tool loops

`fe_chat::ScriptedProvider` (`crates/fe-chat/src/fake.rs:25`) already plays
one reply per request and records the requests. Tests script multi-round
turns with `tool_use` blocks and assert:

- each request's `tools` bytes equal the first's;
- request N+1 extends request N's messages byte for byte;
- all results are in one user message;
- `is_error` results appear for denied, invalid, stopped and limited calls;
- stop mid-tool leaves a valid history;
- **refusal in a later round:** tools complete in round 1, round 2 is
  refused, and the next request keeps round 1's reply and results byte for
  byte, with the new text as a block after them;
- a failed later round's Retry resends the identical request.

The workbench loop tests (AG10) add:

- **Trust withdrawn** during a permission wait, during a read and during a
  running command. Each ends the turn, kills or cancels the work, and sends
  nothing. The next request holds the `is_error` results, the user's text
  and the `tool_removal` message last.
- **Executable substitution** (AG8): with `commands_allow = ["git status"]`,
  a `git` placed in the root, or on a relative `PATH` entry, is not the
  resolved program. `./git status` asks. A remembered answer asks again once
  the name resolves elsewhere.
- **Cross-turn proposals** (AG7): turn 1's a→b, then turn 2's b→c, leaves
  one pending a→c proposal in turn 2, and turn 1's hunk reads as superseded.
  The tests also cover a partly decided earlier proposal, a stale hunk, and
  a pending new file edited again.

### Workbench tests

Workbench tests use GPUI's test platform, and Linux temp folders (`.txt` and
`.go` files, not `.rs`, so no language server starts).

### The manual smoke run

The smoke run needs a Claude Code login and the real adapter. It is run by
hand at 0.106.0, and again for each raise of the tested version. It checks:

1. With no auth capabilities, `initialize`'s `authMethods` is empty.
2. Logged out: the panel shows "Not logged in". Logging in in a terminal and
   Retry works.
3. An Edit lands in the worktree only, and the user's tree is untouched. It
   still arrives as `request_permission` with these allow rules in place,
   which checks that the flag-settings ask rules win:
   - `"permissions": {"allow": ["Bash(*)"]}` in `~/.claude/settings.json`;
   - `"permissions": {"allow": ["Edit(src/**)"]}` in the main checkout's
     `.claude/settings.local.json`. This is a relative project rule, which
     § 3 accepts; a bare `Edit` would be refused.
4. An Edit with an absolute path outside the worktree is refused.
5. `touch ../outside` from Bash fails under the sandbox. Run it twice: once
   with the opened root a **main checkout**, and once with it a **linked
   worktree** of another checkout. Each time:
   - `git commit`, `git branch x` and `git gc` fail in the lane;
   - the common `.git` (refs, objects) and the root's own admin dir are
     unchanged;
   - `git status` and `git diff` work.
6. A deny-listed `.env` read is refused.
7. Turn end produces a review in AI Changes.
8. Restarting Tenmoku resumes the chat with `session/load`. After the
   restart, step 3's forced ask, step 4's refusal and step 5's sandbox still
   hold, and `bypassPermissions` is still not offered.
9. The picker's model and effort change through `session/set_config_option`.
10. Cancel mid-command ends the turn.
11. With `read = "ask"` and `allow: ["Read"]` in `~/.claude/settings.json`,
    a Read still prompts in Tenmoku. With `fetch = "deny"`, `WebFetch` isn't
    offered to the model at all.
12. Claude Agent refuses to start, and names the key, for each of the
    following in `~/.claude/settings.json`:
    - `"sandbox": {"excludedCommands": ["touch"]}`;
    - `"sandbox": {"filesystem": {"disabled": true}}`;
    - `"sandbox": {"network": {"allowUnixSockets": ["/var/run/docker.sock"]}}`;
    - `"permissions": {"additionalDirectories": ["/tmp"]}`;
    - `"permissions": {"allow": ["Edit(//tmp/**)"]}`;
    - `"permissions": {"allow": ["WebFetch(domain:localhost)"]}`.

    It also refuses for:
    - `Edit(//tmp/**)` in the main checkout's `.claude/settings.local.json`;
    - `"sandbox": {"excludedCommands": ["touch"]}` in a file in
      `managed-settings.d`;
    - on macOS, an installed Claude Code configuration profile (managed
      preferences), as uninspectable.

    Editing a widening value into one of these during a turn cancels the
    turn.
13. A user-configured MCP server doesn't start while `mcp_servers = false`,
    or while `other = "deny"`. With `mcp_servers = true`, `other = "ask"`
    and that server's tool in the user's own `allow` list, a call still
    prompts in Tenmoku.
14. **The source table matches this Claude Code version.** It is done in
    two parts, matching the table's last column.
    - **Sources the session loads now** (every row marked "yes", except
      managed preferences). For each, in turn:
      1. plant `"sandbox": {"excludedCommands": ["touch"]}` in it;
      2. run `claude` by hand in a lane worktree and confirm `touch ../x`
         now escapes the sandbox, which shows Claude Code reads that file;
      3. confirm Claude Agent refuses to start, naming that file.

      A source Claude Code reads that the table lacks is a bug in
      `claude_settings.rs`, fixed before the tested version is raised.
    - **Files checked only ahead of Update lane** (the main checkout's
      `.claude/settings.json`). Plant the same value there, and confirm
      Claude Agent refuses to start, naming that file. The current session
      isn't expected to honour it, since Claude Code reads the worktree's
      copy. Then run Update lane by hand and confirm the worktree's copy now
      carries it.

**Decision:** the fake agent binary, `ScriptedProvider` for loops, and a
14-step manual smoke run per adapter version.

## 13. Implementation plan

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

### Wave 1

None of these depends on another card in this plan.

**AG1. Permission model.** Size **s**. Deps: none. Linux-OK.

- Files:
  - `crates/fe-chat/src/permissions.rs` (new: `Class`, `Level`, `Request`,
    `Policy::decide`, command patterns, shell-syntax detection, session
    memory);
  - `crates/fe-chat/src/lib.rs`.
- New test file: `crates/fe-chat/tests/permissions.rs`.
- Tests:
  - strictest wins;
  - allow patterns never beat deny;
  - shell strings, and first words with a `/`, match no allow pattern;
  - a request carries its resolved program path. Allow patterns and session
    memory match only bare names, and memory only while the resolved path
    is unchanged;
  - session memory scoped to a chat.

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

**AG3. Tool-loop turn model and tool set.** Size **m**. Deps: T604.
Linux-OK.

- Files:
  - `crates/fe-chat/src/conversation.rs` (`Round`, `ToolResult`, history per
    § 6, appending to a pending result message, old files loading);
  - `crates/fe-chat/src/request.rs` (frozen tools, task budget);
  - `crates/fe-chat/src/provider.rs` (`ToolSpec::eager_input_streaming`);
  - `crates/fe-chat/src/tools.rs` (new: the five schemas and descriptions,
    typed inputs, validation);
  - `crates/fe-chat/src/fake.rs` (multi-round scripts);
  - `crates/fe-chat/src/lib.rs`.
- New test file: `crates/fe-chat/tests/tool_loop.rs`.
- Tests:
  - byte-identical prefixes across rounds;
  - one result message per round;
  - stop and round limit leave valid history;
  - `max_tokens` and `refusal` run no tools;
  - a later round's refusal keeps every earlier round, and a failed round's
    Retry is the identical request;
  - the next user text goes in as a block after the last results;
  - invalid input becomes an `INVALID_JSON` error result;
  - the `tool_removal` message, last, after the user's text.
- **Carried acceptance criterion F9** (§ 14):
  - finalisation keeps tool calls and results matched one to one;
  - an uncommitted streaming reply keeps finished text only, no `tool_use`
    and no results;
  - a reply with invalid tool input is retried once as the identical
    request, and never enters history;
  - prior committed rounds are unchanged.

  Exact-request tests: Stop, trust withdrawal, and `max_tokens`, each during
  tool-input streaming.

**AG4. Tool wire in fe-chat-anthropic.** Size **s**. Deps: AG3, T604.
Linux-OK.

- Files:
  - `crates/fe-chat-anthropic/src/wire.rs` (`eager_input_streaming`,
    `tool_result` blocks, `output_config.task_budget`);
  - `crates/fe-chat-anthropic/src/provider.rs` (the
    `task-budgets-2026-03-13` and `mid-conversation-tool-changes-2026-07-01`
    betas in `anthropic-beta`).
- New test file: `crates/fe-chat-anthropic/tests/tool_wire.rs`.
- Tests: request JSON fixtures, and the beta header joined with the existing
  ones.

**AG5. fe-acp: crate, dependency and client core.** Size **m**. Deps: none.
Linux-OK. **[Cargo deps]**

- Files:
  - `Cargo.toml` (workspace dependencies: `agent-client-protocol = "=5.2.0"`
    with default features off, and `fe-acp = { path = "crates/fe-acp" }`);
  - `Cargo.lock`;
  - `deny.toml` (only if `cargo deny check licenses` fails; § 2's research
    found every new crate MIT or Apache-2.0);
  - `crates/fe-acp/Cargo.toml`;
  - `crates/fe-acp/src/lib.rs`;
  - `crates/fe-acp/src/client.rs` (new: connection, initialize, new, load,
    prompt, cancel, set config option, set mode, and the handler that hands
    out permission requests);
  - `crates/fe-acp/src/events.rs` (new: `AcpEvent` from `SessionUpdate`).
- New test file: `crates/fe-acp/tests/client.rs`, with an in-process fake
  over `Channel::duplex()`.
- The 5.2.0 crate turns on serde_json's `preserve_order` feature
  (its `Cargo.toml:382-387`). Features unify, so this changes JSON key order
  workspace-wide. The card runs `crates/fe-chat/tests/request.rs`,
  `crates/fe-chat-anthropic/tests/provider.rs` and fe-config's settings
  schema tests unedited, and fixes any order-dependence it finds within its
  scope (chat record § 4 already requires order-independent output).

**AG6. fe-acp: launching, states and the fake agent binary.** Size **m**.
Deps: AG5. Linux-OK.

- Files:
  - `crates/fe-acp/src/launch.rs` (new: spawn from given paths with
    `CLAUDE_CODE_EXECUTABLE`, process group, stderr ring, shutdown, the
    `hide_claude_auth` parameter);
  - `crates/fe-acp/src/claude.rs` (new: `claude::profile`, the § 3 security
    profile from the agent settings, with `CLAUDE_TOOLS` and the class
    mapping; sent on new, load, resume and respawn; re-load when it changes;
    the git common and admin dirs from `git rev-parse` into
    `filesystem.denyWrite`; the version floor; `authRequired` and the other
    states of § 4);
  - `crates/fe-acp/src/claude_settings.rs` (new: the inherited-settings
    check):
    - the source table, with the `managed-settings.d` drop-ins;
    - managed preferences failing closed;
    - keep only `sandbox`, `permissions.allow` and
      `permissions.additionalDirectories`;
    - accept only tightening values, network-granting allow rules included;
    - the watcher callback that cancels a turn on a widening change;
  - `crates/fe-acp/src/bin/fe-acp-fake-agent.rs` (new);
  - `crates/fe-acp/src/lib.rs`;
  - `crates/fe-acp/Cargo.toml`.
- New test files: `crates/fe-acp/tests/launch.rs` and
  `crates/fe-acp/tests/claude_profile.rs`.
- Tests:
  - the profile JSON, byte-identical on new, load, resume and respawn;
  - a changed setting causes a re-load before the next prompt;
  - class levels mapped to `tools` and ask rules;
  - each inherited-settings case of § 12;
  - each state;
  - a crash mid-turn;
  - shutdown kills the group.
- **Carried acceptance criteria** (§ 14, "Open questions carried to
  implementation"):
  - **F6 (high):** with `allowManagedPermissionRulesOnly: true` in a
    managed file or drop-in, Claude Agent refuses to start and names that
    setting. So the forced asks can never be silently ignored. Test:
    `claude_profile.rs` places it in each managed source, and asserts the
    refusal and its reason.
  - **F8:** for a root that is itself a linked worktree, the main checkout
    is resolved from `--git-common-dir`, and its
    `.claude/settings.local.json` is checked and watched. Test: a widening
    rule there refuses; the same rule in the linked root's own local file
    doesn't count as the main checkout's.
  - **F10:** every spawn and respawn sets
    `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`. Test: `launch.rs` reads it
    back from the fake agent's environment.
  - **F11:** smoke step 14 in this record is rewritten to plant `"touch *"`
    after a baseline failure, in the card's own commit to this doc.

### Wave 2

**AG7. Workbench tool executors.** Size **m**. Deps: AG1, AG3, T604.
Linux-OK. **[chat_panel.rs]**

- Files:
  - `crates/fe-workbench/src/agent_tools.rs` (new: `read_file`,
    `list_files`, `search` and `propose_edit`, and `ChatProposals` with
    supersession (§ 6). Built over `chat_apply::locate` and `propose`,
    `chat_context::refusal`, `fe_project::search_filtered`, and a turn's
    `Changeset`);
  - `crates/fe-workbench/src/chat_panel.rs`, visibility only:
    `mod chat_apply` and `mod chat_context` become `pub(crate) mod`
    (`chat_panel.rs:82-85`), so a sibling module can reach them;
  - `crates/fe-workbench/src/chat_apply.rs`, visibility only: `locate`
    (`:424`) and `propose` (`:537`) become `pub(crate)`;
  - `crates/fe-workbench/src/lib.rs`.
- New test file: `crates/fe-workbench/src/agent_tools_tests.rs`.
- Tests:
  - caps and truncation lines;
  - deny list;
  - outside-root refusal;
  - pending-proposal view in `read_file`;
  - overlapping edits refused;
  - supersession across turns (§ 12): a→b then b→c, a partly decided
    earlier proposal, a stale hunk, a pending new file edited again.

**AG8. `run_command` executor and audit log.** Size **m**. Deps: AG1, AG2.
Linux-OK (Unix process groups; Windows parts behind `cfg`).

- Files:
  - `crates/fe-workbench/src/agent_command.rs` (new: resolving a bare name
    on the login `PATH`, skipping relative and in-root entries, to a real
    path, and running exactly that path; grant check; timeout; output cap;
    kill on cancel);
  - `crates/fe-workbench/src/agent_audit.rs` (new: the JSONL worker,
    rotation, `agent.show_audit_log`);
  - `crates/fe-workbench/src/lib.rs`.
- New test files: `crates/fe-workbench/src/agent_command_tests.rs` and
  `crates/fe-workbench/src/agent_audit_tests.rs`.
- Tests: the executable-substitution cases of § 12; timeout and cancel kill
  the group; output caps.

**AG9. Tool rows and permission rows in fe-chat-view.** Size **m**. Deps:
AG3, T604. Linux-OK.

- Files:
  - `crates/fe-chat-view/src/tool_rows.rs` (new: tool rows with status,
    folded output, diff previews, plan checklists, the permission row with
    Allow once, Allow for this chat and Deny);
  - `crates/fe-chat-view/src/view.rs`;
  - `crates/fe-chat-view/src/lib.rs`;
  - `crates/fe-chat-view/l10n/en-US/fe-chat-view.ftl`.
- New test file: `crates/fe-chat-view/tests/tool_rows.rs`.

### Wave 3

**AG10. The API tool loop in the chat panel.** Size **m**. Deps: AG4, AG7,
AG8, AG9, T604. Linux-OK. **[chat_panel.rs]**

- Files:
  - `crates/fe-workbench/src/chat_tool_loop.rs` (new: rounds, limits,
    decisions through `Policy`, prompts, stop mid-tool, trust generations
    and withdrawal, `set_changeset` at turn end, Continue);
  - `crates/fe-workbench/src/chat_panel.rs` (grants, the loop's hook into
    `view_event`, tools off in untrusted folders);
  - `crates/fe-workbench/src/lib.rs`;
  - `crates/fe-workbench/l10n/en-US/chat.ftl`.
- New test file: `crates/fe-workbench/src/chat_tool_loop_tests.rs`.
- Tests: the trust-withdrawal cases of § 12.

**AG11. Claude Agent session host.** Size **m**. Deps: AG1, AG2, AG6, AG8,
and agent-lanes cards 1, 3, 4, 5, 6, 7, 8 and 18. Linux-OK.
**[Cargo deps]** (workspace-internal only: fe-workbench gains `fe-acp`; no
new external crate, so `deny.toml` is unaffected)

- Files:
  - `crates/fe-workbench/Cargo.toml` (`fe-acp.workspace = true`; AG5 put
    `fe-acp` in the root's workspace dependencies);
  - `Cargo.lock`;
  - `crates/fe-workbench/src/acp_host.rs` (new):
    - one adapter per window and root;
    - resolving the programs with `fe_lsp::LoginPath`;
    - the lane per chat, with Update lane before a prompt and freezing at
      turn end;
    - the profile re-load on a settings change;
    - watching § 3's Claude Code settings files while a session is open,
      cancelling the turn on a widening change;
    - permission requests through `Policy` and the hard rules;
    - the audit;
    - trust changes (cancel, `Cancelled`, shutdown);
  - `crates/fe-workbench/src/lib.rs`.
- New test file: `crates/fe-workbench/src/acp_host_tests.rs`, against the
  fake agent binary.
- **Carried acceptance criteria** (§ 14):
  - **F7 (high):** a `request_permission` whose tool is
    `SandboxNetworkAccess` is never decided by policy, never remembered by
    "Allow for this chat", and refused without a prompt for a loopback host.
    Tests: with `other = "allow"` and a remembered `other` answer, such a
    request still prompts; a `localhost` one is refused; neither leaves a
    remembered answer.
  - **F10:** a Bash call asking `run_in_background` runs in the foreground
    and ends before the turn's revision is frozen (fake agent).

**AG12. The Agent choice, ACP transcripts and persistence.** Size **m**.
Deps: AG9, AG10, AG11, T604. Linux-OK. **[chat_panel.rs]**

- Files:
  - `crates/fe-workbench/src/chat_panel.rs` (the header choice, routing
    sends, the § 4 states, the first-open notice, the picker driving config
    options);
  - `crates/fe-chat/src/store.rs` (the `kind: "acp"` record);
  - `crates/fe-workbench/src/acp_transcript.rs` (new: `AcpEvent`s converted
    into AG9's row types, so fe-chat-view never depends on fe-acp);
  - `crates/fe-chat-view/src/view.rs` (a transcript of rows with no API
    history behind it, for Claude Agent chats);
  - `crates/fe-workbench/src/lib.rs`;
  - `crates/fe-workbench/l10n/en-US/chat.ftl`.
- New test files: `crates/fe-workbench/src/chat_agent_tests.rs` and
  `crates/fe-chat/tests/acp_store.rs`.

**AG13. Request changes and hunk comments.** Size **s**. Deps: AG10, AG12.
Linux-OK. **[chat_panel.rs]**

- Files:
  - `crates/fe-workbench/src/ai_changes_panel.rs` (the text box, per-hunk
    Comment, Send);
  - `crates/fe-workbench/src/chat_panel.rs` (one entry point that takes the
    instruction as the chat's next message or prompt);
  - `crates/fe-workbench/l10n/en-US/changes.ftl`.
- New test file: `crates/fe-workbench/src/ai_changes_request_tests.rs`.

**AG14. Follow-up doc edits.** Size **s**. Deps: approval of this record.
Linux-OK.

- Files: `docs/designs/agent-lanes.md` and
  `docs/decisions/5at38c8v89-ai-chat.md` (the edits listed in § 11).
- Test: none (docs).

Order: wave 1 in parallel; then AG7, AG8 and AG9; then AG10. AG11 waits for
its lanes cards, then AG12 and AG13 follow.

- **Mac-only cards:** none. Every card is Linux-OK.
- **Mac check:** the sandbox's macOS path (Seatbelt) is checked in the
  manual smoke run on a Mac.

**Decision:** the 14 cards above, AG1 to AG14, filed by the lead.

## 14. Out of scope and open questions

**Out of scope:**

- **Inputs and outputs:** attachments beyond today's, usage and cost
  reporting, and ghost-text completions.
- **Claude Code's own protocols:** its terminal IDE protocol, and slash
  commands from `available_commands_update`.
- **Other ACP agents.** Gemini CLI and Codex, and so the overlay
  `fs/write_text_file` path of § 3.
- **ACP's draft v2.**
- **Steering a running turn.**
- **Subagent sessions.**
- **Background tasks** that outlive a turn.
- **Anything that runs outside a worktree:** Claude Agent in non-git folders,
  and a sandboxed `run_command` for the API chat.

**Open questions:**

1. **Subscription use if Tenmoku is distributed.** The user asked to use
   their own Claude subscription through their own Claude Code, and this
   record does that.
   - Tenmoku offers no login and never touches credentials. But a Tenmoku
     shipped to other people could be read as offering claude.ai rate
     limits, which the Agent SDK rule forbids "unless previously approved".
   - Should such builds start the adapter with `--hide-claude-auth`, or wait
     for Anthropic's approval? **This blocks any distribution beyond the
     user's own machine.** The user answers it on return.
2. **Labels.** "Claude Agent" in the header, and "Claude Code" only where an
   instruction names the user's installed `claude`. Is that enough for the
   branding guidelines, or should instructions avoid the name too?
3. **Hooks.** Should Tenmoku pass `disableAllHooks: true` in the flag
   settings, closing § 3's gap at the cost of the user's own hooks? Or leave
   them, as decided?
4. **Non-git folders.** Offer Claude Agent there with a copied directory as
   its lane, or keep it unavailable, as decided?
5. **Sandboxing `run_command`.** Should the API chat's `run_command` run in
   a worktree lane or an OS sandbox too, so that allowing a command no longer
   means it can write the user's files? Or keep `ask` plus the warning, as
   decided?
6. **Worktree cost.** A Claude Agent lane builds in its own worktree (its
   own `target/`, no `node_modules`). Share `CARGO_TARGET_DIR`, or cap
   lanes? This is agent-lanes' open question 4, and matters more here.

### Open questions carried to implementation

The fourth review of this record (round 4) found F6–F11. The approver
(Mac lead) took the record as proposed, with these carried to the cards
named below. Each is an acceptance criterion of that card in § 13. The
text of §§ 3, 6 and 12 above is not yet amended for them: where they
differ, the card's criterion wins.

- **F6 (high): managed `allowManagedPermissionRulesOnly`.**
  - **Gap.** Set in managed settings, it makes Claude Code ignore
    permission rules from flag settings, which are this design's forced asks
    and deny-list rules (SDK `sdk.d.ts:7797`). § 3's check doesn't read it.
  - **Fix.** The inherited-settings check also reads
    `allowManagedPermissionRulesOnly` from every managed source (file,
    drop-ins), and Claude Agent refuses to start, with that reason, when it
    is true.
  - **Card:** AG6 (`claude_settings.rs`).
- **F7 (high): `SandboxNetworkAccess` requests.**
  - **Gap.** Claude Code's live sandbox network requests reach
    `request_permission` with kind `Other`. § 7's fallback classes them
    `other`, so policy or a remembered class-wide answer could approve one,
    localhost included. `allow_once` then opens that host for the session.
  - **Fix.** A request whose tool is `SandboxNetworkAccess` is its own
    request, never class `other`:
    - always prompted, never decided by policy;
    - never remembered;
    - refused without a prompt for a loopback host (the § 3 loopback list).
  - **Card:** AG11 (`acp_host.rs`, permission requests), with the
    classification in AG1's `Policy`.
- **F8 (medium): the main checkout when the root is a linked worktree.**
  - **Gap.** When the opened root is itself a linked worktree,
    `<root>/.claude/settings.local.json` is not the main checkout's local
    file, which Claude Code loads.
  - **Fix.** Resolve the main checkout as the parent of `git rev-parse
    --path-format=absolute --git-common-dir`. Check and watch its
    `.claude/settings.local.json`. Keep the opened root's
    `.claude/settings.json` as the Update-lane source.
  - **Card:** AG6 (`claude_settings.rs`), with a linked-root settings test.
- **F9 (medium): tool-loop recovery for unfinished tool blocks.**
  - **Gap.** § 6's rules don't keep tool calls and results matched in two
    cases, so the next request can fail:
    - Stop while streaming keeps text only, yet another rule answers every
      call;
    - trust withdrawal doesn't say which streamed `tool_use` blocks survive.

    Separately, invalid JSON is kept as a string-valued `tool_use.input`,
    which can't be echoed as the object the API requires.
  - **Fix.** Separate finalisation rules for each case. Prior committed
    rounds stay unchanged in every case:
    - **an uncommitted streaming reply:** drop every `tool_use` and keep only
      finished text, with no results;
    - **a committed round:** exactly one result per retained call.

    A reply with invalid tool input is not committed. It is retried once as
    the identical request, then the turn fails without entering history.
  - **Card:** AG3 (`conversation.rs`), with exact-request tests for Stop,
    trust withdrawal, and `max_tokens` during tool-input streaming.
- **F10 (medium): background commands outlive cancel and freezing.**
  - **Gap.** Claude Code's Bash can run commands in the background,
    including after its own timeouts. They survive `session/cancel`
    (adapter `README.md:88-98`), and can keep writing the lane after its
    revision is frozen.
  - **Fix.** The adapter is always spawned with
    `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` (SDK `sdk.d.ts:3363`), on every
    spawn and respawn, so nothing is backgrounded. A detached process a
    command starts on its own is a stated limit. It is bounded by the
    sandbox to the worktree, and anything it writes after a freeze shows in
    the next revision.
  - **Card:** AG6 (`launch.rs`), with a test in AG11 that a background
    request runs in the foreground and ends before the turn's freeze.
  - **Where this is placed.** The approver's note put F10 on the API
    tool-loop card. But the gap is in Claude Code's Bash under the Claude
    Agent launcher: the API loop's `run_command` has no backgrounding. So it
    is placed on AG6 and AG11, and the approver can move it.
- **F11 (medium): smoke step 14 can't show the escape.**
  - **Gap.** `excludedCommands: ["touch"]` matches only `touch` without
    arguments, so `touch ../x` stays sandboxed.
  - **Fix.** Plant `"touch *"`. First show the same model-issued `touch ../x`
    fails under the baseline sandbox, then show it escapes after the source
    gains the setting.
  - **Card:** AG6, whose criteria include rewriting smoke step 14 this way
    before the first manual smoke run.
