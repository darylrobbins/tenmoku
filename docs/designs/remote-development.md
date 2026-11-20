# Remote development: the user workflow

Written 2026-11-20 for T1986. The decisions behind this file are in
[`docs/decisions/j8wvgszah0-remote-development.md`](../decisions/j8wvgszah0-remote-development.md)
(section numbers below starting with "D" point there). The visual language is
[`docs/design/design-system.md`](../design/design-system.md). This file walks
through what the user sees, end to end:

- opening a remote folder
- the first connect, which installs the server
- working in a remote window
- losing and regaining the link
- forwarded ports
- trust

It names every state of the remote pill and the connect dialog, so later
design cards can draw them. It writes no code. The cards are D § 13.

v1 covers SSH hosts on macOS and Linux clients. WSL arrives with the Windows
client. Dev containers are not in v1.

## 1. Ways in

- **File › Open Remote…** (command `remote.open`, also in the palette) opens
  the connect dialog (§ 2).
- **File › Open Recent** lists remote folders beside local ones as
  `app — neon`, with a small host glyph (Lucide `server`). Choosing one skips
  the dialog and goes straight to Connecting (§ 3). Remote entries carry
  their `ssh://` location, so the same folder on two hosts shows as two rows.
- **The CLI:** `tenmoku ssh://neon/home/me/src/app` opens that folder. So
  do `ssh://me@box:2222/srv/app` and `ssh://neon/home/me/src/app/README.md`,
  which opens the file in its folder's window, or in a new window rooted at
  the file's directory. A running Tenmoku takes the request over the existing
  IPC handoff, like a local path. `--wait` works too: it returns when the
  remote tab closes.
- **From a remote terminal:** `tenmoku file.rs` inside a terminal of a
  remote window opens the file in that window, through a shim the server
  puts on the terminal's `PATH` (phase 7).
- **Reopen at launch:** windows that were remote at the last quit reopen in
  Connecting and connect on their own. If ssh needs a prompt, the prompt
  waits until that window is in front.
- **Welcome / empty window** (still to design in the design system) gets an
  "Open Remote…" action beside Open Folder.

## 2. The connect dialog

A modal sheet on the current window, 520 px wide. It moves through four
steps. The Back button returns to the previous step, and Esc cancels
whatever is in flight.

### 2.1 Host

- One field: **Host**. Its placeholder reads `ssh alias, user@host or
  user@host:port`.
- The field completes from three sources:
  - recent hosts, most recent first;
  - the concrete `Host` entries in `~/.ssh/config`, read locally, skipping
    patterns with `*` or `?`, and following `Include`;
  - typed history.
- Each suggestion shows its `HostName` in grey when the config gives one
  (`neon   neon.tailnet.ts.net`).
- Buttons: **Connect** (default) and Cancel. Connect is disabled until the
  field is non-empty. A malformed value, such as a space or a scheme other
  than `ssh://`, shows an inline error under the field: "Not an ssh host".
- A pasted `ssh://host/path` fills the host and remembers the path for the
  Folder step.

### 2.2 Connecting

The sheet body shows a spinner and one status line, which moves through
these states:

| State | Line | Notes |
|---|---|---|
| ssh starting | "Connecting to neon…" | |
| Waiting on the user | "Waiting for you to sign in to neon" | Shown while an askpass prompt is up (§ 2.5). |
| Probing | "Checking neon…" | The bootstrap's first line (OS, arch, installed?). |
| Downloading | "Downloading tenmoku-server 0.3.0 for linux-x86_64… 43%" | A 72 px progress bar, as in the design system's operation-in-progress pattern. Skipped when the client's cache has the archive. |
| Verifying | "Verifying…" | Signature and SHA-256 (D § 4). |
| Uploading | "Installing on neon… 60%" | The upload over the same ssh session. |
| Starting | "Starting tenmoku-server…" | Until the handshake's Welcome. |

Downloading, Verifying and Uploading appear only on the first connect to a
host for this Tenmoku version (§ 3). A connect to a host that already has
the server goes from Connecting to the Folder step in about one ssh
handshake: ~350 ms to neon.

### 2.3 Folder

- **A path field,** prefilled with the remembered path, or else the remote
  `$HOME`. Typing completes directory names from the host, with Tab or as you
  type after a 150 ms pause. Each listing is one request, and a row spinner
  shows after 150 ms.
- **A list** of the current directory's folders, with dimmed files below
  them. Double-click or → enters a folder, and ← or Backspace on an empty
  segment goes up. Hidden entries stay hidden until you type a `.`.
- **Open** opens the folder. The current window is used if it is empty
  (no root and no tabs); otherwise a new window opens. **Cancel** closes the
  connection unless another window already uses it.

### 2.4 Errors

An error replaces the status line with a red signal icon, the message and
actions. The log behind **Show Log** is the connection log: ssh's stderr and
the bootstrap transcript.

| Cause | Message | Actions |
|---|---|---|
| ssh failed (DNS, refused, timeout) | "Couldn't connect to neon: ssh: connect to host neon port 22: Connection refused" (ssh's last stderr line) | Retry, Show Log, Back |
| Authentication failed or cancelled | "neon didn't accept the sign-in." | Retry, Show Log, Back |
| Host key changed | "The host key for neon has changed. This could mean someone is intercepting the connection. Tenmoku won't connect until the key in ~/.ssh/known_hosts is fixed." | Show Log, Back. No override in the dialog. |
| Unsupported host | "tenmoku-server isn't built for FreeBSD amd64." | Back |
| No matching release (dev build) | "This development build has no matching tenmoku-server release. Set remote.dev_server to a server built for linux-x86_64." | Open Settings, Back |
| Offline, no cache | "Couldn't download tenmoku-server 0.3.0: <error>" | Retry, Open Settings, Back |
| Verification failed | "The downloaded tenmoku-server failed its signature check and was deleted." | Retry, Show Log |
| Host can't install | "Couldn't install on neon: ~/.cache isn't writable (mkdir: Permission denied)" or "…: no space left on device" | Show Log, Back |
| Handshake refused (by either side) | "tenmoku-server on neon is a different build (0.2.9+ab12cd3). Reinstalling…" | None: it reinstalls automatically, once. A second refusal is an error with Show Log. |
| Dev server is another build | "remote.dev_server is build 0.3.0+9f8e7d6, this client is 0.3.0+ab12cd3-dirty. Build it from the same checkout." | Open Settings, Show Log, Back. Nothing is reinstalled. |

### 2.5 The sign-in prompt

ssh's prompts arrive through the askpass helper (D § 1). They use the same
modal as git credential prompts, titled with the host.

- **Passphrase or password:** "Enter passphrase for key
  '/Users/me/.ssh/id_ed25519':", taken verbatim from ssh. It has a secure
  field, OK and Cancel. Tenmoku never stores the answer.
- **Unknown host:** the full ssh text, including the fingerprint line and
  "Are you sure you want to continue connecting (yes/no/[fingerprint])?",
  with the buttons **Connect** (answers `yes`) and Cancel. The fingerprint
  is selectable so it can be compared.
- **OTP / keyboard-interactive / FIDO touch:** shown as ssh words them. A
  touch notice has no field, just Cancel, and closes on its own when ssh
  proceeds.

One prompt is shown at a time across windows, as git prompts are today.

## 3. First connect

The first connect to a host for a given Tenmoku version is the only slow
one. The sequence:

1. Connecting, then Probing. The bootstrap reports
   `Linux x86_64, missing`.
2. Downloading, from the GitHub release `v<version>`, into the client's
   cache. It is skipped if another host already needed this archive.
3. Verifying.
4. Uploading over the open ssh session. The estimate is 10–15 MB, about
   1–5 s at the 2.8–15.2 MiB/s measured to neon.
5. Starting, then the Folder step.

Nothing is asked of the user beyond the ssh sign-in. The dialog's footnote
says what happens: "Tenmoku installs its server in ~/.cache/tenmoku-server
on neon. It runs only while you're connected." A Learn more link opens the
docs page. Later connects to that host show only Connecting.

After a Tenmoku update, the next connect installs the new server next to the
old one, so the first connect after each update is slow too. Old builds are
pruned to the newest three on the host (D § 4).

## 4. The remote window

- **Title:** `app — neon`. The window's root, tabs and tree all belong to
  the host.
- **The remote pill** sits in the title bar, immediately left of project ▾
  (the design system's title-bar order gains it as the first project-scoped
  item). It is a 20 px pill holding a host glyph and the host's name, and
  its colour follows the signal tokens. Every state:

  | State | Pill | Tooltip | Click menu |
  |---|---|---|---|
  | Connecting | spinner + `neon`, neutral | "Connecting to neon…" | Cancel, Show Log |
  | Installing | spinner + `neon`, neutral, with a thin progress underline | "Installing tenmoku-server… 60%" | Cancel, Show Log |
  | Connected | glyph + `neon`, neutral | "Connected to neon (me@neon.tailnet.ts.net) · 34 ms" | Disconnect, Forwarded Ports (n), Show Log, Close Remote Window |
  | Connected with ports | glyph + `neon · 2`, neutral | "…· 2 forwarded ports" | as Connected |
  | Reconnecting | spinner + `neon`, warning (amber) | "Lost connection to neon. Reconnecting (attempt 3, next in 8 s)." | Reconnect Now, Disconnect, Show Log |
  | Disconnected | broken-link glyph + `neon`, error (red) | "Disconnected from neon. Your edits are kept." | Reconnect, Show Log, Close Remote Window |
  | Needs sign-in | key glyph + `neon`, warning | "neon is waiting for you to sign in." | Sign In (brings the prompt forward), Disconnect |

  The pill's accessible name follows the same text: "Remote: neon,
  connected, 34 milliseconds". A state change is announced once.
- **Status bar:** remote operations (opening a large file, the first Quick
  Open walk, installs) use the existing operation-in-progress slot. Nothing
  new is added.
- **The rail** of a remote window has no Agents or Chat items, and the AI
  changes panel is absent (D § 10). Their palette commands are hidden. A key
  binding for one shows the toast "AI chat and agents aren't available in
  remote windows yet". Local windows are unchanged.
- **Bottom panel:** gains a **Ports** tab in remote windows (§ 6).

## 5. Working remotely

This section covers what changes for the user. The rule is that typing never
waits (D § 11).

- **Tree:** expanding a folder shows a row spinner after 150 ms. Create,
  rename, move and delete show the row in a pending style until the host
  confirms, and an error arrives as a toast. Changes made on the host by
  other tools, such as `git checkout` in a terminal, appear through the
  watcher as they do locally.
- **Opening a file:** the tab appears at once, read-only, with a loading
  indicator. The content follows in one round trip for files up to 1 MiB.
  Larger files stream, using the existing large-file path.
- **Saving:** the tab's unsaved dot becomes a ring while the save is in
  flight, and turns clean when the host confirms. If the file changed on the
  host since it was read, the usual prompt appears: "“main.rs” changed on
  disk since it was opened. Overwrite it with your version?" (Overwrite /
  Cancel). Auto save skips such a file with its usual toast. A change on the
  host to a file with no unsaved edits reloads it, and one with unsaved
  edits asks Reload or Keep Mine, as locally.
- **Quick Open:** results fill in while the host walks the tree. The first
  batch arrives in one round trip, and the 50k-file fixture finished in
  146 ms over real ssh. An "Indexing…" footer shows until the walk ends.
- **Find in Files** runs on the host and streams results. The progress
  state is unchanged.
- **Terminals** open a shell on the host, in the project folder. A keystroke
  echoes one round trip later; v1 has no local echo prediction. Tasks, runs,
  tests and debug sessions run on the host, and their panels behave as
  locally.
- **Language servers** run on the host, found on the host's login-shell
  `PATH`. Their status sits in the status bar as today, with "on neon" in
  the tooltip.
- **Git** runs on the host. When `git push` needs a credential, the host's
  request comes back to the same sign-in modal (§ 2.5), titled with the git
  remote and "on neon".

## 6. Ports

- **Detection:** when a process started from this window (in a terminal, a
  task or a run) starts listening on a TCP port on the host, a toast says
  "Port 5173 is listening on neon", with the actions **Open in Browser** and
  **Forward**. Open in Browser forwards the port and then opens
  `http://localhost:5173`. Ports that a run or task configuration declares
  are forwarded without asking. Detection can be set by
  `remote.auto_forward`: `notify` (the default), `forward` or `off`.
- **Ports tab** (bottom panel, remote windows only). One row per forwarded
  or detected port:

  | Column | Content |
  |---|---|
  | Port | `5173`. A detected port that isn't forwarded is dimmed. |
  | Local address | `localhost:5173`, or `localhost:5174` when 5173 was taken locally, with a note "5173 was in use" |
  | Process | `node (vite)` and its pid, when the host knows them |
  | Source | Detected, Task "dev", Settings, or Added |

  Row actions: Open in Browser, Copy Address, Stop Forwarding (or Forward
  for a detected port). The header has **Forward a Port…**, a small field
  that takes `3000` or `3000:13000` (host:local). The empty state reads "No
  ports yet. Ports your programs listen on appear here."
- **Scope:** forwards bind `127.0.0.1` on the client only.
- **Lifetime:** forwards follow the session. During Reconnecting their
  local listeners stay up, and new connections wait or fail with a refused
  connection. They resume on reconnect. Disconnecting removes them, and so
  does closing the window. Settings-declared ports come back on every
  connect.
- The pill shows the forward count (`neon · 2`). Its menu's Forwarded Ports
  item opens the Ports tab.

## 7. Losing and regaining the link

What the user sees, in order:

1. **The link drops:** ssh exits, or two heartbeats go missing (20 s). The
   pill turns amber, Reconnecting. One toast appears: "Lost connection to
   neon. Your edits are kept, and saves will finish when it's back." There
   is no modal, and editing carries on.
2. **While reconnecting:**
   - A Save marks the tab **save pending**: a ring with a small clock, and a
     tooltip saying "Will save when neon is back".
   - Terminal tabs show a dim overlay, "Reconnecting…", and take no
     input. Programs in them keep running on the host. Language-server
     features pause, and their status shows "waiting for neon". Editing
     carries on, and the language server catches up on resume.
   - Opening a file that isn't loaded shows its tab as "Not loaded —
     reconnecting". It loads on resume.
   - Attempts back off over 1, 2, 4, 8 and 15 s, then every 30 s. The
     tooltip shows the attempt and the next retry, and Reconnect Now
     retries at once.
3. **Back within 10 minutes:** the session resumes. A toast "Reconnected
   to neon" dismisses itself after 4 s.
   - **Terminals** carry on from exactly where they left off, and the
     output written meanwhile appears. A terminal that wrote more than
     1 MiB while away can't be replayed. It shows a dim line, "Output was
     lost while disconnected", and the program in it is asked to redraw,
     so a full-screen program such as vim or htop comes back whole.
   - **Language servers** carry on. They get the edits made while away
     before anything else, so diagnostics and completion match the buffer.
     A server that wrote more than 8 MiB while away restarts instead, the
     way it does after a crash, and its status shows "restarted after
     reconnect". A debug session in that state ends, with "Debug session
     ended: the connection to neon was lost".
   - **Pending saves** run in order, each checking the host file is still
     the one it last saw. If the file changed on the host meanwhile, or a
     new file's name was taken by another file, that file's Overwrite /
     Cancel prompt appears, and nothing on the host is replaced until the
     user chooses Overwrite.
   - **Open files** whose host copy changed reload, or ask Reload or Keep
     Mine, as on window focus today.
4. **Longer than 10 minutes, or the host rebooted:** the pill turns red,
   Disconnected. The toast reads "Disconnected from neon. Terminals and
   language servers there were stopped. Your edits are kept." and offers
   Reconnect. Reconnecting starts a fresh session: the tree reloads,
   language servers start again, terminals open fresh, and pending saves
   run with the stamp check.
5. **Closing a window with pending saves:** "2 files have changes that
   weren't saved to neon." The choices are **Keep for Next Time** (default:
   kept in the local backups, offered on the next connect to that folder),
   Discard and Cancel.
6. **Needs sign-in:** if a reconnect needs a prompt, the prompt waits until
   that window is in front. Meanwhile the pill shows Needs sign-in and the
   backoff pauses.

## 8. Trust

- **The first open of a remote folder** asks the same trust question as a
  local one, with the host in the title: "Trust the folder app on neon?"
  The body says what trust allows: language servers, tasks, runs, notebook
  kernels. The buttons are the same, and Don't Trust keeps the window in
  restricted mode, as locally.
- **Trust is per folder per host.** Trusting `/home/me/src` on neon covers
  its subfolders on neon only, never the same path on another host or on
  this Mac. The Trust Folder and Don't Trust Folder commands
  (`workbench.trust_folder`, `workbench.untrust_folder`) work unchanged.
- **Connecting is not trusting.** Signing in to a host decides nothing about
  its folders.
- **Project plugins:** a remote project's own plugins never load in v1,
  even when the folder is trusted. The restricted-mode notice lists that:
  "Plugins from this project aren't loaded in remote windows."
- **Host identity** is ssh's own: the unknown-host prompt (§ 2.5) and the
  refusal on a changed key (§ 2.4). Tenmoku keeps no second list of hosts.

## 9. Settings

All four are in user settings only; a project file can't set them.

| Setting | Default | Meaning |
|---|---|---|
| `remote.reconnect_grace` | `10` (minutes, 1–60) | How long a host keeps a dropped session |
| `remote.ports` | `[]` | Ports forwarded on every connect: `3000` or `"5432:15432"` |
| `remote.auto_forward` | `"notify"` | Detected ports: `notify`, `forward` or `off` |
| `remote.dev_server` | unset | A local `tenmoku-server` to install instead of the release download (development builds) |

## 10. For later design cards

Each item needs a drawing, with every state listed above:

- **Connect dialog:** the Host, Connecting (7 states, § 2.2), Folder and
  Error (9 causes, § 2.4) steps, plus the sign-in modal's three kinds
  (§ 2.5).
- **Remote pill:** 7 states (§ 4), its menu, and its place in the title bar
  at both rail sides.
- **Tab states:** saving ring, save pending, and "Not loaded —
  reconnecting".
- **Terminal overlay:** "Reconnecting…".
- **Ports tab:** rows and their dimmed detected state, the Forward a Port
  field, and the empty state.
- **Toasts:** lost, reconnected, disconnected, port detected, AI
  unavailable.
- **Close prompt:** pending saves.
- **Recents and Welcome:** remote rows with the host glyph.
