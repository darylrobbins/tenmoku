# Remote development (T1986)

Tenmoku will edit a project that lives on another machine. The UI, buffers and
everything that reacts to a keystroke stay on the user's machine. Files,
search, watching, language servers, debug adapters, terminals, tasks and git run
on the host. v1 supports SSH hosts. WSL comes later, behind the same transport
seam. Dev containers are not in v1, though the seam leaves room for them.

This file settles the design. A throwaway spike under `tools/remote-spike/`
measured the protocol (§ 12). The user workflow, with its dialog and pill
states, is in [`docs/designs/remote-development.md`](../designs/remote-development.md).
Nothing under `crates/` or `kiln/` changes in this card. The traits arrive in
phase 1 (§ 13).

## Decisions at a glance

| Topic | Decision |
|---|---|
| Transport | The system `ssh -T`, so the user's ssh config, agent, ProxyJump and known_hosts all apply. Prompts go through `SSH_ASKPASS` to the fe-git askpass helper. A `RemoteTransport` trait leaves room for `wsl.exe -d` later and `docker exec -i` after v1. |
| Protocol | One multiplexed stream with a 12-byte fixed header (length, kind, channel, request id). It carries per-channel byte credits in both directions, cancel, a heartbeat, and a handshake that refuses any protocol **or** build mismatch. Bodies are capped at 1 MiB and chunks at 64 KiB. |
| Encoding | **postcard** over serde. It was the smallest of the four encodings measured, and the fastest on file chunks. It is already in `Cargo.lock` (through wasmtime). A strict build match removes the need for protobuf's schema evolution (§ 2.3). |
| Window | 1.25 MiB of initial credit per channel, each way. Bulk channels get a window sized from the measured link (2 × rate × min RTT, clamped to 256 KiB–4 MiB). Both numbers are measured (§ 2.4). |
| Server | A headless `tenmoku-server` binary (no GPUI) for Linux and macOS. `--stdio` attaches to one daemon per host, user and build. A session per window survives a dropped link for 10 minutes. |
| Deploy | The server is never bundled in the app. The client downloads `tenmoku-server-<version>-<os>-<arch>.tar.gz` from the GitHub release tagged `v<version>` and checks an Ed25519-signed `SHA256SUMS` with the update key it already has. It uploads the archive over the same ssh session to `~/.cache/tenmoku-server/<build>/`. The host needs no internet access. A dev build uses `remote.dev_server`. |
| Host traits | `FsHost`, `WatchHost`, `SearchHost`, `ProcessHost`, `PtyHost`, `GitHost` in a new GPUI-free crate, `fe-host`. The local impls are today's code. Remote versions of fe-lsp's `ServerLauncher` and fe-dap's `AdapterLauncher` are built on `ProcessHost`. |
| Identity | `ProjectLocation = Local(PathBuf) \| Remote { host, path }`, written as `ssh://[user@]host[:port]/abs/path`. It is threaded through recents, reopen, IPC, the layout, backup and undo keys, trust, and the window title. |
| Reconnect | Heartbeat every 10 s; the link counts as lost after 2 missed pongs or EOF. Backoff runs 1–30 s for up to 10 minutes, and the client re-attaches to the session with its token. Saves are queued, and the stamp check on write feeds the existing disk-conflict flow. |
| Ports | Forwarded through the protocol (not `ssh -L`) to `127.0.0.1` on the client, set explicitly or auto-detected from listening sockets in the session's process tree. |
| Trust | Remote folders get their own entries in the existing trust file, keyed `ssh://…`, gating the same things as locally. Connecting to a host trusts no folder. |
| Local only | UI, buffers, undo, vim, syntax, plugins, settings, themes and hot-exit backups. **AI agents and chat stay local and are not offered in remote windows in v1** (§ 10). |
| Latency rule | Typing never awaits the network. Every remote operation is async and has a defined pending state (§ 11). |

## 1. Transport

v1 runs `ssh -T <destination> -- <bootstrap>` as a child process. Its stdin and
stdout carry the protocol, and its stderr goes to the connection log.

- **Why the system ssh rather than a Rust SSH library:** the user's
  `~/.ssh/config` (Host aliases, `ProxyJump`, `IdentityFile`, `Match`
  blocks, certificates), the ssh agent, hardware keys, Kerberos/GSSAPI and
  `known_hosts` all work unchanged. A library would need to reimplement each
  of them. It is also the same choice fe-git made for `git push` over ssh.
- **Options the client always passes:** `-T` (no tty: our stdio is binary),
  `-o ServerAliveInterval=15 -o ServerAliveCountMax=3` (to notice a dead TCP
  path even when our heartbeat is starved), and `-o ConnectTimeout=15`. It
  never passes `BatchMode=yes`, so prompts can reach the user. It sets no
  `ControlMaster` either. The user's config may add one, and that works.
- **Prompts:** ssh has no tty, so it asks through `SSH_ASKPASS`, with
  `SSH_ASKPASS_REQUIRE=force`. That covers passphrases, passwords, OTP,
  host-key confirmation and FIDO touch notices. This is the fe-git pattern
  (`crates/fe-git/src/askpass.rs`): a private mode-700 directory, a unix socket
  and a `#!/bin/sh` shim that execs `tenmoku --askpass <socket> <prompt>`. The
  workbench shows one modal at a time (`fe-workbench/src/git_askpass.rs`). The
  remote client reuses `Askpass::new` as is, and the dialog names the host
  rather than a git remote. An unknown host key is the ssh prompt
  `Are you sure you want to continue connecting (yes/no/[fingerprint])?`. The
  dialog shows it verbatim, with the fingerprint, and answers with what the
  user typed.
- **Trait:**

  ```rust
  pub trait RemoteTransport: Send + Sync {
      /// Runs `argv` on the host with its stdio as the returned pipe.
      fn spawn(&self, argv: &[&OsStr], askpass: &Askpass) -> io::Result<Link>;
      /// "ssh neon", for the pill and the log.
      fn describe(&self) -> String;
  }
  pub struct Link { reader: Box<dyn Read + Send>, writer: Box<dyn Write + Send>, child: Child }
  ```

  The impls are `SshTransport` in v1, `WslTransport` (`wsl.exe -d <distro> --
  <argv>`) with the Windows client (phase 12), and after v1 a
  `DockerExecTransport` (`docker exec -i <container> <argv>`). **Dev
  containers are not in v1.** The trait is the only seam they get now. The
  spike's `Transport` and `ChildStdio` (`tools/remote-spike/src/transport.rs`)
  are this shape, and it ran unchanged over a real `ssh -T` (§ 12).
- **Client platforms:** v1 ships the client on macOS and Linux. fe-git's
  askpass is unix-only (no unix socket helper on Windows yet), so the Windows
  client and with it WSL wait for a Windows askpass channel (phase 12).

## 2. Protocol

### 2.1 Framing

Each frame has a fixed header followed by a body:

```text
u32 LE  len    bytes after this field: 8 header bytes + body (8 ..= 8 + 1 MiB)
u8      kind   Hello Welcome Refused Request Data End Cancel Credit Ping Pong
u8      flags  0 (reserved; non-zero is a protocol error in v1)
u16 LE  chan   channel: 0 = control
u32 LE  id     request id (high bit set = server-originated), or ping nonce
[len-8] body   postcard of the kind's body type
```

- **Why a fixed header rather than an all-postcard frame:** the
  demultiplexer, the credit counter and cancel bookkeeping read the channel,
  request and length without decoding the body. A relay (a future WSL or
  container hop) can forward frames it doesn't understand.
- **Bounds:** the largest body is 1 MiB (`MAX_BODY`). The decoder rejects a
  longer length before it allocates, and it rejects an unknown kind, non-zero
  flags or a short length. Senders chunk files, byte streams and walk batches
  at 64 KiB (`CHUNK`), so one frame holds the wire for at most 64 KiB.
- **Fuzzable decoder:** `decode_all(&[u8]) -> Result<Vec<Header>>` decodes
  the header and then the typed body of every frame in a buffer, and it may
  only return an error. The spike checks this against 200,000 mutated and
  truncated frames (`frame::tests::garbage_never_panics`). The protocol crate
  (phase 2) adds a `cargo fuzz` target on the same function and a proptest
  round trip for every body type.
- **Paths on the wire are bytes** (`Vec<u8>`, the unix `OsStr` bytes), so a
  non-UTF-8 file name round-trips. The spike used `String` for brevity.

### 2.2 Channels, requests, streams

- **Channel 0 is control.** It carries the handshake, pings, `Open` (which
  returns a new channel for a language server, debug adapter, pty, process or
  forwarded TCP connection), session resume and shutdown.
- **Fixed service channels:** 1 fs, 2 watch, 3 search, 4 git, 5 process
  control. Dynamic channels start at 64. Each language server, adapter, pty,
  child process and forwarded TCP connection gets its own channel, so a
  terminal flooding output cannot use up a language server's credit.
- **A request** is `Request{chan, id, op}`. The reply is zero or more `Data`
  frames, then one `End{status}`. A subscription (a watch, search results,
  process output) is a request that stays open. Cancel ends it with
  `End(Cancelled)`.
- **Cancel** is `Cancel{chan, id}`. The server stops producing and ends the
  request with `Cancelled`. The data that can still arrive after a cancel is
  bounded by the channel's credit (§ 2.4). The spike measured 0.94–1.19 MiB
  of bulk data after a cancel with a 1.25 MiB window, and the e2e test
  asserts that the total never exceeds the window. It also measured
  0–24,064 walk entries, which is what the host produces before the cancel
  reaches it.
- **Server-originated requests** have the high id bit set. The server's
  askpass relay uses them when a git or ssh process on the host wants a
  credential (`git push` from a remote project): `tenmoku-server --askpass`
  on the host forwards the prompt, and the client shows the same askpass
  dialog.
- **Opaque payloads:** LSP JSON-RPC, DAP messages, pty bytes and TCP bytes
  travel as raw `Data(Bytes)` on their channel. The server never parses LSP
  or DAP. The client's fe-lsp and fe-dap speak to a channel as if it were the
  child's stdio. Paths in those messages are host paths, and so are the
  client's own paths for a remote window (§ 6), so nothing is rewritten.

### 2.3 Encoding: postcard

Measured with `remote-spike codec` (build-identical bodies, release builds).
The time column is one encode plus one decode, each from an already-built
value of that codec's own types. For prost that means its generated-style
structs, built before the clock starts, the way a protocol that used prost
types throughout would hold them. Converting the spike's serde types into
prost's is a separate row, because only a design that kept serde types and
converted at the edge would pay it. Each cell is the best of three runs.
Both machines were loaded: the Mac is an M3 Max at load 15–27, and neon is
Linux x86_64 at load ~25.

| body | encoding | bytes | Mac | neon |
|---|---|---:|---:|---:|
| walk batch, 512 entries | **postcard** | **16,387** | 24.4 µs | 94.0 µs |
| | bincode 2 (serde) | 16,900 | 18.7 µs | 85.0 µs |
| | protobuf (prost) | 18,432 | 33.5 µs | 133.0 µs |
| | *(prost: building its structs from serde ones)* | — | *7.7 µs* | *38.6 µs* |
| | serde_json | 44,045 | 84.0 µs | 321.6 µs |
| file chunk, 64 KiB | **postcard** | **65,540** | **2.1 µs** | **5.6 µs** |
| | bincode 2 | 65,542 | 3.2 µs | 7.4 µs |
| | protobuf (prost) | 65,540 | 4.9 µs | 7.8 µs |
| | *(prost: building its struct from the serde one)* | — | *0.8 µs* | *2.5 µs* |
| | serde_json | 233,434 | 788.4 µs | 2,335.2 µs |
| request, ReadFile | postcard / bincode / prost | 46 | 0.1 µs | 0.1–0.3 µs |
| | serde_json | 68 | 0.1 µs | 0.4 µs |

Why postcard:

- **It is the smallest everywhere and the fastest on bytes,** which is most
  of the traffic: file contents, pty output and LSP payloads. bincode is
  5.7–9 µs faster per 512-entry walk batch. A 50k-entry tree is ~100
  batches, so that would save under 1 ms per walk, against a 34 ms round
  trip on the real link.
- **It is already a dependency** (postcard 1.1.3, through wasmtime in
  `Cargo.lock`), so it adds no licence, no new crate and no build step. It
  has a written, stable wire format (postcard 1.0).
- **Not protobuf:** it is 12% larger on walk batches. Timed on its own
  prebuilt structs, so with no conversion counted against it, it takes
  1.4× postcard's time on batches on both machines and 1.4–2.3× on chunks.
  It also needs a schema and codegen toolchain (prost-build, protoc) or
  hand-annotated structs. Its strength is cross-version schema evolution,
  and the handshake makes that moot. Client and server are always the same
  build (§ 2.5), so a body type can change in any release without a
  compatibility rule.
- **Not JSON:** 2.7× the bytes and 3.4× the time on batches, and 3.6× the
  bytes and ~375–420× the time on binary chunks (a `Vec<u8>` becomes an
  array of decimal numbers).

### 2.4 Flow control: per-channel credits, and the window

Credits count body bytes. Each side grants each channel an initial window. A
sender may queue a `Data` frame only while it holds enough credit on that
channel. The receiver returns credit as its *consumer* takes data, in grants
of a quarter window, or at once when a request ends. A slow consumer, such as
a terminal tab that isn't drawing or a stalled LSP reader, therefore stops
only its own channel. The e2e test
`a_stalled_channel_does_not_block_another_and_cancel_stops_within_its_window`
leaves a bulk stream unread on one channel, then completes an echo on
another and finds the stalled channel's total bounded by its window. The
sender's scheduler puts urgent frames first (handshake, pong, cancel), then
round-robins the channels one frame at a time. A bulk stream therefore
delays another channel at the sender by at most one 64 KiB chunk.

The window size trades throughput against interactive latency, because bytes
already handed to ssh can't be reordered. The spike measured both:

Throughput of one bulk stream (MiB/s; local child over a pipe, injected
latency, no bandwidth cap; median of three runs):

| one-way latency | 256 KiB | 1 MiB | 1.25 MiB | 4 MiB |
|---:|---:|---:|---:|---:|
| 0 ms | 1,436 | 1,265 | 1,275 | 1,491 |
| 5 ms | 17 | 72 | 90 | 288 |
| 25 ms | 4 | 15 | 19 | 59 |

The same, on a 100 Mbit/s link with 25 ms one way, with a 1 KiB echo on
another channel. The bulk stream is consumed from before the first echo
until after the last one, so every sample meets live contention. The rate
and the longest gap in bulk delivery are measured over the same window as
the echoes, and they show the stream never stalled while they ran:

| window | bulk MiB/s while echoing | longest bulk gap | echo p50 / p99 under bulk | idle echo p50 |
|---:|---:|---:|---:|---:|
| 256 KiB | 3.4 | 33.4 ms | 55.3 / 59.0 ms | 50.3 ms |
| 1 MiB | 11.9 | 22.3 ms | 63.0 / 82.6 ms | 50.3 ms |
| 1.25 MiB | 11.9 | 19.2 ms | 78.8 / 87.5 ms | 50.3 ms |
| 4 MiB | 11.9 | 23.2 ms | 251.8 / 306.4 ms | 50.4 ms |

Over real ssh to neon (34 ms RTT, on a link whose raw ssh rate swung
2.8–15.2 MiB/s between runs), a 1.25 MiB window gave an echo p50 / p99 of
50.3 / 109.1 ms beside a bulk stream that kept moving 14 MiB/s.

The decision:

- **Initial window: 1.25 MiB per channel, each way.** A response up to 1 MiB
  (with its 4 bytes of chunk overhead per 64 KiB) then fits in one window and
  takes one round trip. With a 1 MiB window the spike's 1 MiB open took two
  (100.6 ms at 25 ms one way in a `--quick` run, against 50.6 ms at 1.25 MiB). Most source
  files are smaller still. On a 100 Mbit/s, 50 ms link it already saturates
  the bandwidth, at a cost of about 30 ms to an interactive frame queued
  behind a bulk stream.
- **Bulk channels get a measured window.** File transfers over 1 MiB,
  uploads, the server install and forwarded ports open their channel as
  bulk. Their window is `2 × delivery rate × min RTT` from the heartbeat and
  the channel's own delivery rate, clamped to 256 KiB–4 MiB and resized at
  each heartbeat. The rows above show why a fixed 4 MiB window is wrong: it
  saturates a fast link no better than 1 MiB, and it adds 200–260 ms to
  every interactive frame on a slow one.
- The server refuses a window under 4 chunks (256 KiB), so that a grant
  batch is always at least one chunk.

### 2.5 Handshake and heartbeat

- **Hello (client → server):** `{protocol: u32, build: String, window: u32,
  resume: Option<Resume>, location: ProjectLocation}`, where `Resume` is
  `{session: SessionToken, streams: Vec<(chan, recv_next: u64)>}`. The
  server replies `Welcome{protocol, build, window, session: SessionToken,
  streams: Vec<StreamResume>}` or `Refused{reason}` and closes. The
  `streams` lists carry the resume offsets of § 7.
- **Strict match, checked by both peers:** `protocol` *and* `build` must be
  equal. The server refuses a Hello that doesn't match. The client checks
  the Welcome in turn and closes the link if the server answered as another
  protocol or build, whatever binary it is (a stale install, a hand-built
  `remote.dev_server`, a relay). The postcard bodies rely on both ends being
  one build (§ 2.3), so no path skips either check. `build` is
  `<version>+<commit>[-dirty]`. `tenmoku --version` prints only
  `tenmoku <version>` today (`cli.rs`), so phase 2 adds the commit to both
  binaries at build time. There is no compatibility range:
  each client build talks only to its own server build, which the install
  step guarantees (§ 4). The spike's handshake refuses a different build with
  the reason
  `client protocol 1 build "0.0.0-other", server protocol 1 build "0.0.0+spike"`.
  The e2e test `a_different_build_is_refused_at_the_handshake` covers the
  server's check. `a_server_answering_a_different_build_is_refused_by_the_client`
  covers the client's, against a fake server that accepts any Hello and
  answers as `9.9.9+other`.
- **Heartbeat:** the client pings on channel 0 every 10 s. The server
  answers at once, through the urgent queue, so a bulk stream can't delay
  the pong at the sender. Two missed pongs (20 s), or EOF from ssh, count as
  a lost link (§ 7). The heartbeat's round trip also feeds the bulk window
  (§ 2.4) and the pill's tooltip ("34 ms").

## 3. Server shape and lifetime

- **Binary:** `tenmoku-server`, a new crate `crates/tenmoku-server`. It has
  no GPUI dependency, and `scripts/check-no-gpui.sh` enforces that already
  because the crate isn't in `UI_CRATES`. It builds on Linux (x86_64,
  aarch64; static musl so old distros work) and macOS (arm64). It links
  fe-project (listing, watcher, search, finder, fs_ops), fe-buffer's
  `DiskStamp`, fe-git (gix status, blame, HEAD blobs), alacritty_terminal's
  tty (the pty half of fe-terminal) and the spawning half of fe-lsp
  (`LoginPath`, so servers find the host's login-shell `PATH`). It links
  nothing from fe-workbench, fe-gpui or kiln-shell. Windows hosts are not
  supported, other than WSL, which runs the Linux build.
- **Two modes, one binary:**
  - `tenmoku-server daemon` holds the sessions. There is one per host, user
    and build. It listens on a unix socket in `$XDG_RUNTIME_DIR/tenmoku/`
    (else `~/.cache/tenmoku-server/<build>/run/`), in a mode-700 directory,
    with `flock` on a lock file as the liveness test (the pattern
    `crates/fastedit/src/ipc.rs` uses for the app's own socket).
  - `tenmoku-server --stdio` is what ssh runs. It connects to the daemon,
    starting it detached (`setsid`) if no lock is held, and then splices
    stdio to the socket. When ssh dies, only this process dies.
- **Sessions:** one per client window (= per remote project location). A
  session owns that window's watches, language servers, adapters, ptys,
  processes and port forwards. Two windows on the same host share the
  daemon, but not processes, so it matches today's per-window behaviour.
- **Grace period:** a session whose link drops stays alive for **10
  minutes** (`remote.reconnect_grace`, 1–60). Its ptys keep running, and
  each keeps the output the client hasn't acknowledged, up to 1 MiB.
  Language servers and debug adapters keep running, with up to 8 MiB each.
  Resume continues each stream from the exact byte the client last
  acknowledged, or resets it by a fixed rule when the buffer overflowed
  (§ 7, "Resumable streams"). Watches keep recording into a dirty-path set. When the
  grace ends the session is torn down: the daemon kills its process groups
  (TERM, then KILL after 2 s). The daemon exits 60 s after its last session
  ends.
- **Upgrades:** a new client build installs and starts a new server build
  next to the old one (§ 4). An old daemon keeps serving its old clients and
  exits when they're gone. There is no in-place upgrade and no cross-build
  attach.

## 4. Deploy and version policy

The server is **never bundled in the app**, which keeps the app small and
keeps Linux binaries out of a macOS bundle. Instead:

- **Release assets.** `release.yml` (tag `v<version>`, previews included,
  since `scripts/check-release-version.sh` ties the tag to the Cargo version)
  builds and uploads:
  - `tenmoku-server-<version>-linux-x86_64.tar.gz`
  - `tenmoku-server-<version>-linux-aarch64.tar.gz`
  - `tenmoku-server-<version>-macos-arm64.tar.gz`
  - `tenmoku-server-<version>-SHA256SUMS`, listing the three archives
  - `tenmoku-server-<version>-SHA256SUMS.sig`, an Ed25519 signature over the
    sums file

  The signature is made with the existing update key (secret
  `TENMOKU_UPDATE_PRIVATE_KEY`, through `crates/fastedit/examples/sign_update_feed.rs`),
  and the client verifies it with the public key release builds already
  embed (`option_env!("TENMOKU_UPDATE_PUBLIC_KEY")` in `velopack_update.rs`,
  checked with `update_signature.rs`). A per-asset `.sha256` alone,
  which is what the app archives have today, would trust whoever controls
  the download. The signed sums file trusts only the release key.
- **The client downloads, verifies and uploads. The host never fetches.**
  On first connect to a host that lacks the server for this build, the
  client:
  1. downloads the matching archive and the sums pair from
     `https://github.com/tenmoku-editor/tenmoku/releases/download/v<version>/…`
     through fe-net, whose `grammar_fetch.rs` already follows a release
     asset's redirect to GitHub's storage host and refuses any host it
     doesn't name;
  2. checks the signature, then the archive's SHA-256;
  3. caches it in `~/Library/Caches/Tenmoku/remote-server/<build>/` (the
     platform cache dir on Linux), so the next host needs no download;
  4. streams it to the host over the ssh session being opened (bootstrap
     below), and the host re-checks the SHA-256 before installing.

  Why not have the host `curl`/`wget` it? A host may have no egress (a
  corporate build box), a proxy the user never configured for
  non-interactive shells, or neither tool. Host-side fetching would also put
  signature verification on a machine whose tools we don't control. A
  single path that always works beats a fast path plus a fallback. The cost
  is one upload per host per build: the archive is estimated at 10–15 MB
  (not yet built), about 1–5 s at the 2.8–15.2 MiB/s measured to neon, shown as a
  progress bar. A host-side fetch can come later as an optimisation behind
  the same verification.
- **Offline client.** A cached archive installs without the network. With no
  cache and no network the connect fails with "Couldn't download
  tenmoku-server <version>: <error>", plus Retry and a link to the
  `remote.dev_server` setting.
- **Bootstrap.** The ssh argv is `sh -c '<script>'`. The script is POSIX sh
  of under 60 lines, sent inline, and it needs only `uname`, `mkdir`,
  `head -c`, `chmod`, `mv`, `tar` and `sha256sum` or `shasum -a 256`. In one
  ssh session, so with one authentication, it:
  1. prints `TENMOKU-BOOT 1 <uname -s> <uname -m> <present|missing>` for
     `~/.cache/tenmoku-server/<build>/tenmoku-server` (honouring
     `$XDG_CACHE_HOME`);
  2. if the server is missing, reads `<size> <sha256>` and then exactly
     `<size>` bytes from stdin into a temp file in the same directory,
     verifies it, unpacks it, `chmod 755`, and renames it into place;
  3. prints `TENMOKU-BOOT ready` and `exec`s `tenmoku-server --stdio`.

  The client sends nothing after the archive until it reads `ready`, so no
  shell tool can read past the payload. `uname -m` values `x86_64`/`amd64`
  and `aarch64`/`arm64` map to the asset names. Anything else, or a Windows
  host, ends the connect with "tenmoku-server isn't built for <os> <arch>".
- **Install directory.** `~/.cache/tenmoku-server/<build>/`, where `<build>`
  is the handshake's build string with `+` kept, since it is a safe file
  name. After a successful install the bootstrap removes builds other than
  the newest three. It never removes one whose daemon lock is held.
- **Dev builds.** A build with a `-dirty` or unreleased build string has no
  release asset. It needs `remote.dev_server = "/path/to/tenmoku-server"`, a
  local binary built for the host's target (for example `cargo build
  --release -p tenmoku-server --target x86_64-unknown-linux-musl`). It is
  uploaded like a release asset, with its own SHA-256 standing in for the
  signed sums. It installs to `~/.cache/tenmoku-server/dev-<sha256[..12]>/`.
  The strict match of § 2.5 still applies, with no exception for dev
  builds: the binary must report exactly the client's build string, which
  is what a server built from the same checkout reports. Any other build
  ends the connect at the handshake with "remote.dev_server is build
  <theirs>, this client is <ours>. Build it from the same checkout." With no
  such setting the connect stops with "This development build has no
  matching tenmoku-server release. Set remote.dev_server to a server built
  for linux-x86_64." The setting is read only from user settings, never from
  a project file.

## 5. Host traits

The traits live in a new GPUI-free crate, **`fe-host`**, which fe-project,
fe-buffer, fe-editor, fe-terminal, fe-git and fe-workbench may depend on.
Each trait's local impl is today's code, moved rather than rewritten, so
phase 1 changes no behaviour. A window holds a `HostSet` (one `Arc<dyn …>`
per trait), chosen from its `ProjectLocation`: `LocalHost` or
`RemoteHost(conn)`.

| Trait | Shape (abridged) | Today's code that gets it |
|---|---|---|
| `FsHost` | `read(path) -> (bytes, DiskStamp)`; `write_atomic(path, bytes, expect: WriteExpect) -> Result<DiskStamp, Conflict>` (§ 7); `stat`, `list_dir`, `create`, `rename`, `remove`, `trash`, `canonicalize` | fe-project `listing.rs` and `fs_ops.rs`, which have **no FS seam today** (every API takes `&Path` and calls `std::fs`). Also fe-buffer `Buffer::read_file(File)` (`buffer.rs:639`), whose loader passes bytes from the host; fe-workbench `loader.rs`; fe-editor `disk.rs` (`SaveJob`), whose temp-file-and-rename and xattr preservation move into the local impl; and `disk_sync.rs`, `auto_save.rs`, `search_replace.rs`, `encoding_picker.rs`. |
| `WatchHost` | `watch(root) -> Subscription<Vec<FsChange>>` | fe-project `watcher.rs` `ProjectWatcher`, whose `FsChange::{Path, Rescan}` is the wire type unchanged. |
| `SearchHost` | `search(query, options, filter) -> Stream<FileMatches>`; `files(root, filter) -> Stream<Vec<PathBuf>>` | fe-project `search.rs` (`search`, `search_filtered`, `search_excluding`) and `finder.rs`'s file list. |
| `ProcessHost` | `spawn(ProcSpec{program, args, env, cwd, stdio}) -> ProcessIo{stdin, stdout, stderr, exited, kill, pid}`; `which(program)`; `login_path()` | fe-workbench `run_process.rs` (`RunProcess`, `Command::new` in `run_to_end`), `external_format.rs`, `vim_filter.rs`, `test_lens.rs`, `notebook_kernel.rs`, `plugin_dev.rs`, and the git CLI sites `git_remote.rs`, `git_blame.rs`, `git_stage.rs`. |
| `PtyHost` | `open(shell, cwd, env, size) -> PtyIo{read, write, resize, exited, kill}` | fe-terminal `Session::spawn` (`session.rs:379`), which opens its pty locally today (`rustix_openpty` + `tty::from_fd`). `PtyLoop` is already generic over `EventedPty + OnResize` (with a `FakePty` for tests), so the remote impl is an `EventedPty` fed by a channel through a socketpair. Tasks run in the terminal (`task_run.rs` → `terminal.add_task`), so they follow. |
| `GitHost` | `status`, `blame`, `head_blob`, `diff`, plus `run_remote_op(op, askpass)` | fe-git's gix-backed API as fe-workbench calls it, and `remote.rs`'s fetch, push and pull, whose askpass on the host relays to the client (§ 2.2). |

Launchers that already exist get remote impls instead of a new trait:

- **fe-lsp `ServerLauncher`** (`launcher.rs:241`, `launch(&ServerSpec, root,
  &LaunchCancel) -> ServerIo` plus `finds`) is installed per window through
  `set_server_launcher`. `RemoteLauncher` spawns through `ProcessHost`, and
  its `ServerIo` streams are the process's channels. `finds` asks the host's
  `which`. The `ConsentGate` wrapper (`lsp_consent.rs`) still wraps it, so
  consent and trust keep working.
- **fe-dap `AdapterLauncher`** (`launcher.rs:131`). `RemoteAdapterLauncher`
  spawns through `ProcessHost`. For `AdapterTransport::Tcp` and `Port`,
  which today connect to `127.0.0.1` on the *client*, it opens a TCP relay
  channel to the host's loopback port (the port-forward mechanism, § 8) and
  hands fe-dap that stream.

## 6. ProjectLocation

```rust
pub enum ProjectLocation {
    Local(PathBuf),
    Remote { host: HostId, path: RemotePath },
}
pub struct HostId { kind: HostKind /* Ssh (v1), Wsl (later) */, destination: String /* as typed: "neon", "me@box:2222" */ }
pub struct RemotePath(Vec<u8>); // absolute POSIX path bytes, canonical on the host
```

- **Text form:** `ssh://[user@]host[:port]/abs/path`, with percent-encoding
  for bytes that aren't URL-safe. A bare ssh-config alias is the host part
  (`ssh://neon/home/me/src/app`). `tenmoku ssh://neon/home/me/src/app`
  opens it from the CLI, and `ProjectLocation::parse` accepts exactly this.
  `Local` keeps its plain path form, so existing files stay readable.
- **Inside a window, paths stay `PathBuf`.** A remote window's buffers, tree
  rows, LSP URIs and search results hold host paths, and the window's
  `HostSet` says which machine they're on. That works because v1 hosts and
  clients are both POSIX. Opening a *local* file from a remote window opens
  it in a local window. The Windows client (phase 12) needs a `RemotePath`
  type through the editor instead, and that card owns the change.
- **Where it's threaded** (phase 1f, local behaviour unchanged):

  | Site | Today | With `ProjectLocation` |
  |---|---|---|
  | Recents | fe-config `recent.rs` `RecentList{folders: Vec<PathBuf>}` | `Vec<ProjectLocation>`. A remote entry is stored as its URI and shown as `app — neon`. |
  | Reopen | fastedit `reopen.rs` `LaunchWindow = (Option<PathBuf>, …)`, skipping roots that fail `is_dir()` | The root becomes `Option<ProjectLocation>`. A remote root skips the local `is_dir` check and reopens as a window in **Connecting** (design, § "Reopen"). |
  | IPC handoff | `ipc.rs` `Request{targets: Vec<Target{path,…}>, cwd}` | A `Target` may carry a URI. A local `tenmoku ssh://…` hands off like a path. A `tenmoku` shim the server puts on remote terminals' `PATH` sends opens over the protocol to the owning window (phase 7). |
  | Layout / window state | kiln-core `layout.rs` `layout_key(root)` = FNV-1a 64 of the root's bytes | Local roots keep the same key (same bytes), and remote ones hash the URI. |
  | Hot-exit backups, persistent undo | fe-buffer `backup.rs`, `undo_store.rs`, under `<state>/backups/<key>` | The same key scheme. Backups stay **local** (§ 10), so a remote window's unsaved edits survive a lost link and a client restart. |
  | Trust | kiln-core `TrustStore` keyed by canonical `PathBuf` | Keyed by the location's text form (§ 9). |
  | Window identity | title = root name | `app — neon`, and the window's `HostSet`. |

## 7. Reconnect, queued saves, stamps

- **Detection:** EOF on ssh's stdout, or two missed pongs (20 s). ssh's own
  `ServerAlive*` options catch a dead TCP path within 45 s even if the
  heartbeat is starved. The window then enters **Reconnecting**. Buffers stay
  editable throughout, because typing never awaits the network (§ 11).
- **Backoff:** 1, 2, 4, 8, 15 s, then every 30 s, until the session's
  10-minute grace ends. Each attempt is a fresh `ssh` with `Hello.resume =
  Some(token)`. A pending askpass prompt, such as an expired agent key,
  pauses the backoff. When the grace ends, or the daemon has no such session,
  the window shows **Disconnected** with a Reconnect button. Reconnecting
  then starts a fresh session, and the client restarts language servers and
  re-lists the tree.
- **On resume:** requests in flight on the request channels (fs, watch,
  search, git, process control) when the link dropped got `End(LinkLost)`.
  Reads (open, stat, list, search, git status) are retried once
  automatically, and writes are never retried blindly. Byte-stream channels
  resume as below. The session's watch dirty set comes back as
  `FsChange::Path` entries, or `Rescan` if it overflowed. The client then
  re-stats every open buffer and compares stamps, as `DiskSync::check_file`
  does on window focus today.
- **Resumable streams.** A pty, a language server's or debug adapter's
  stdio, and a run's output are byte streams that the far side keeps
  producing while the link is down. Replaying "the buffered output" would
  be wrong: after a drop mid-delivery, neither side knows which bytes the
  other got, and a duplicated or missing byte corrupts LSP's
  `Content-Length` framing. So these channels number their bytes, in both
  directions:
  - **Offsets.** A stream channel's data body is `Chunk{offset: u64, bytes}`,
    where `offset` is the stream position of the first byte. A receiver
    delivers bytes only from whole frames, and only at its next expected
    offset (`recv_next`). It drops anything below `recv_next` (a duplicate)
    and treats a jump past it as a protocol error. A frame cut off by the
    drop is discarded whole, so a cut in the middle of a frame is the same
    as a cut at the boundary before it.
  - **Acknowledgement.** Every `Credit` frame on a stream channel also
    carries the receiver's `recv_next`. The sender keeps every byte it has
    sent until an acknowledgement covers it. While the link is up that is
    at most the window plus a quarter-window grant (about 1.6 MiB). While
    it is down, the producer's new output collects in the same buffer, up
    to its cap: 1 MiB per pty and 8 MiB per language server, adapter or
    run. The client keeps its own unacknowledged input the same way: pty
    keystrokes and messages to a server, at most a window.
  - **Resume.** `Hello.resume` lists the client's `recv_next` for every
    stream channel, and the `Welcome` lists the server's. Each side then
    resends from the other's `recv_next`, so every byte arrives exactly
    once, in order, whether the link died between frames or inside one.
    Credit restarts at the window, with the resent bytes counted against
    it.
  - **Overflow.** If a buffer passed its cap, the bytes the peer needs are
    gone. The sender's `StreamResume` then says `Gap{lost: u64}` instead of
    resending, and that channel's rule applies:
    - *Language server:* the daemon kills it, and the client restarts and
      re-initialises it through fe-lsp's crash path. That path reopens
      every document, so nothing typed offline is lost.
    - *Debug adapter:* the session ends with "Debug session ended: the
      connection to neon was lost". DAP has no resync.
    - *pty:* the client resets its terminal emulator (alacritty `Term`
      reset, scrollback kept) and prints a dim line, "Output was lost
      while disconnected". It then sends a resize to the current size,
      and the server toggles the pty's window size so the program gets
      `SIGWINCH` and redraws. Full-screen programs come back whole, and
      plain output shows the gap honestly. Without a gap the stream is
      exact, so the emulator's state is exact too, and no reset happens.
    - *Run or task output:* the panel shows "N bytes of output were lost
      while disconnected" at the gap and carries on.
  - **Edits made while disconnected.** During Reconnecting the client sends
    nothing on a language server's channel. fe-lsp marks the channel paused,
    new requests fail as "server restarting" (§ 11), and document changes
    are not queued. On a gapless resume, fe-lsp first lets the resent
    traffic through, which completes requests that were in flight. It then
    sends, for every open document that changed since its last sent
    version, one full-text `didChange` with the next version, before any
    new request. The retained server ends up with exactly the client's
    text. A document opened or closed offline gets its `didOpen` or
    `didClose` in the same pass.
  - **TCP relays** (port forwards, DAP over TCP) don't resume. Their
    connections drop with the link, and the listener stays (§ 8).
- **Write preconditions.** A write says what it expects to replace. A bare
  `Option<DiskStamp>` can't do that, since `None` would have to mean both
  "nothing may be there" and "overwrite whatever is there". fe-editor's
  `SaveJob` already keeps these apart (`expected: Option<Option<DiskStamp>>`
  plus `overwrite` and `new_entry`), and the wire type keeps every case it
  has:

  ```rust
  pub enum WriteExpect {
      /// Overwrite was chosen: replace whatever is there.
      Overwrite,
      /// The buffer doesn't know the file's state (`expected: None`). What the
      /// server finds when the write starts must still be there at the
      /// commit, and a path found empty is committed without clobbering.
      AsFound,
      /// Nothing, not even a symlink, may be at the path: a new file
      /// (`expected: Some(None)`) or a Save As destination (`new_entry`).
      Absent,
      /// The file must still be the one with this stamp.
      Matches(DiskStamp),
  }
  ```

  `SaveJob` maps onto it one to one (`overwrite` → `Overwrite`, then
  `new_entry` or `Some(None)` → `Absent`, `Some(Some(s))` → `Matches(s)`,
  `None` → `AsFound`). The host runs the same commit code the local impl
  does, moved into fe-host in phase 1a: the temp file, then a re-check
  just before the rename, and for `Absent` and an `AsFound` that found
  nothing, an atomic no-clobber commit. That is tempfile 3.27's
  `persist_noclobber`: `renameat2(RENAME_NOREPLACE)` on Linux and
  `renameatx_np(RENAME_EXCL)` on macOS, else a hard link, which also refuses
  an existing name. Where none of those works, `SaveJob`'s
  `create_in_place` (`O_CREAT | O_EXCL`) commits instead. A file
  that appears at the path before the commit makes it fail with
  `AlreadyExists`, which becomes `Conflict{current}`. It never replaces the
  newcomer.
- **Queued saves:** a Save (or auto-save) during Reconnecting writes nothing
  remotely. The tab shows *save pending*, the buffer is already in the local
  hot-exit backup, and the save joins a per-file queue that keeps only the
  newest content. The pending record is `{path, content, expect:
  WriteExpect}`. `expect` is fixed when the first save is queued, from the
  buffer's last read or save, and newer content replaces only `content`,
  since none of the queued saves reached the host. The record is stored
  with the hot-exit backup, so a save kept across a closed window or a
  restart keeps its precondition. On resume each pending save runs as
  `write_atomic(path, content, expect)`. A new file queued with `Absent`
  therefore fails with `Conflict` if another writer created that path
  during the disconnection, and nothing is overwritten. Closing a window
  that has pending saves asks "Keep pending changes?". The backups keep
  them, like hot exit, and they are offered on the next connect.
- **Stamp conflicts:** for `Matches`, the server compares the file's current
  `DiskStamp` (`len`, `modified`, `dev`, `ino`, all computed on the host, so
  `dev` and `ino` are the host's) with the expected one, both before
  writing and again just before the rename. For `Absent`, anything at the
  path is a conflict, and the no-clobber commit closes the race. A conflict
  returns `Conflict{current}`, which maps to fe-editor's
  `SaveError::ChangedOnDisk`. That drives the flows that already exist, so
  local and remote share them:
  - a manual save gets fe-gpui's prompt "“name” changed on disk since it
    was opened. Overwrite it with your version?" (Overwrite / Cancel,
    `pane_item.rs`). Overwrite re-sends with `expect = Overwrite`, and
    Cancel keeps the buffer and its pending state;
  - auto save skips the file with the toast "Auto save skipped: name changed
    on disk";
  - a host-side change found by the watcher or the resume re-stat goes
    through `DiskSync` (`disk_sync.rs`): a clean buffer reloads, and an
    unsaved one asks Reload or Keep Mine. The client treats the stamp as an
    opaque value and only compares it for equality.

## 8. Port forwarding

- **Mechanism:** the client listens on `127.0.0.1:<port>` (the same port if
  free, else the next free one). Each accepted connection sends
  `Open{Tcp{host: "127.0.0.1", port}}` on channel 0, gets a bulk channel and
  relays bytes both ways. Closing either side ends the channel. The relay
  uses no `ssh -L`. The forward lives inside our stream, so it survives a
  reconnect (connections drop, the listener stays), and it works unchanged
  over WSL or a container later.
- **Sources:**
  - Explicit: the Ports panel's Forward a Port…, or `remote.ports = [3000,
    "5432:15432"]` in user settings.
  - Detected: every 2 s the server lists listening TCP sockets owned by the
    session's process tree, from `/proc/net/tcp{,6}` plus `/proc/<pid>/fd` on
    Linux, or `lsof -nP -iTCP -sTCP:LISTEN` on macOS hosts. Loopback and
    wildcard binds are offered, and the first appearance raises a
    notification: "Port 5173 is listening on neon — Open in Browser". The
    detected list is offered, not auto-forwarded, except ports a run or task
    configuration declares, which are forwarded when they appear.
- **Bind scope:** the client binds loopback only. v1 has no "share on LAN".
- **Debug adapters** over TCP use the same relay internally (§ 5).

## 9. Trust

- Trust stays a **client-side policy**. The server runs whatever its own user
  can run, since it's that user's account. Trust decides what the *client
  asks for*.
- **Keys:** remote folders use the same trust file (`fe_config::paths::trust_path()`,
  loaded by `kiln_shell::trust::set_trust_file`). `TrustStore`'s
  `BTreeMap<PathBuf, bool>` becomes a map keyed by the location's text form:
  local keys stay the canonical path they are today, and remote keys are
  `ssh://neon/home/me/src/app`, canonicalised on the host. Ancestor lookups
  work per host (a trusted `ssh://neon/home/me/src` covers its children), and
  a host never inherits from a local folder or another host.
- **What it gates** is unchanged. The checks already in place gate language
  server launch (`lsp.rs` `untrusted_root`), `RunProcess`
  (`Refused::Untrusted`), tasks, notebook kernels and plugin roots. They now
  ask with the window's location, and the processes they allow run on the
  host.
- **Connecting trusts no folder.** The first open of a remote folder asks the
  same trust question as a local one, with the host in the title. A
  remote project's own extensions and plugins (project-root plugin
  directories) are **not loaded in v1**, trusted or not. Plugins are local
  wasm, and loading a remote root's would mean fetching and running code
  from the host. The user's own plugins run as usual.
- Host identity is ssh's `known_hosts`, confirmed through askpass (§ 1).
  Tenmoku keeps no second host-key store.

## 10. What stays local

- **Local:** the whole UI, buffers (ropes), undo and redo with the persistent
  undo store, vim, tree-sitter parsing and highlighting, folding, minimap,
  plugins (wasm), keymaps, themes, user settings, hot-exit backups, the
  notification inbox, and file icons. A project's settings file is read from
  the host through `FsHost` and watched through `WatchHost`.
- **Remote:** file contents and metadata, the tree, watching, search, the
  file finder's list, language servers, debug adapters, terminals, tasks,
  runs and tests, formatters, git, and forwarded ports.
- **AI agents and chat (user decision): local only, and not offered on
  remote projects in v1.** Their tools (`chat_tool_loop.rs`, `agent_tools.rs`,
  fe-acp agents) read, write and run through the local `std::fs` and
  `Command`, so in a remote window they'd act on the wrong machine. A remote
  window therefore:
  - builds its `Dock` with `chat: None` (already an `Option<Entity<ChatPanel>>`)
    and no `AgentsPanel`. The `PANELS` rail list (`dock.rs:172`) gains an
    availability predicate, which leaves the Agents (`always_on_rail`) and
    Chat items off a remote window's rail. The AI changes panel goes the
    same way;
  - marks `chat.open`, `chat.new`, `chat.ask_about_selection`, the
    `AgentsCommands` set and `agent_command` unavailable when the window is
    remote. The palette hides them. A key binding for one shows a notice,
    "AI chat and agents aren't available in remote windows yet", and nothing
    opens;
  - leaves local windows untouched. A user can still chat in a local window
    beside the remote one.

  Making agents and chat host-aware (tools through `HostSet`) is a post-v1
  card.
- **Dev containers:** out of v1, other than the `RemoteTransport` seam (§ 1).

## 11. Latency rule

**Typing never awaits the network.** Edits, cursor motion, selections, vim,
undo, syntax and folding all act on the local buffer. No keystroke handler
holds a remote reply. Every remote operation is async and has a defined
pending state:

| Operation | Pending UI | On a lost link |
|---|---|---|
| Connect / install | Pill "Connecting…", then "Installing server… 43%" (§ design) | Error dialog with Retry |
| Open file | Tab opens at once, read-only and empty, with a loading indicator. Its content appears in one round trip for ≤ 1 MiB (50.7 ms at 25 ms one way). | Tab shows "Not loaded — reconnecting" and loads on resume |
| Save, auto-save | Tab dot becomes the saving ring. Save returns at once. | *Save pending* (§ 7) |
| Disk-change check (focus) | None: a batched async stat. Stale stamps are kept until it answers. | Runs on resume |
| Tree expand | Row spinner after 150 ms | Row keeps its old children, greyed |
| Tree create, rename, move, delete, trash, tree undo | Row shows a pending style until the host confirms; nothing is optimistic | Fails with a notice; nothing queued |
| Watch events | None (background) | Dirty set replayed on resume |
| Quick Open list | Fills progressively from the streamed walk (first batch 1 RTT + ~2 ms, 50k entries in 146 ms over real ssh), with an "Indexing…" footer | Keeps the last list |
| Find in Files | Streamed results with the existing progress state. A new query cancels the old one. | Search ends "interrupted" |
| LSP (completion, hover, definition, diagnostics…) | As today: already async with their own pending and stale states | Server channel paused; requests fail as "server restarting". On resume, offline edits go as one full-text `didChange` per document (§ 7) |
| Format on save | Existing formatter timeout. Past it, the save goes ahead unformatted, as today. | Save queued unformatted |
| Git status, blame, diff gutter | Decorations appear when ready, and stale ones stay until replaced | Kept, greyed |
| Git fetch, push, pull | Existing progress and askpass flow | Operation fails; nothing retried |
| Terminal | Keystrokes are sent async and echo after one round trip. v1 has no local echo prediction. | Tab shows "Reconnecting…" and takes no input; on resume the stream continues from the last acknowledged byte, or resets with a "lost output" line and a redraw (§ 7) |
| Tasks, runs, tests, debug | As today (async process start, run panel states) | Paused with the session; ended if the grace expires |
| Port forward | Ports panel row "Starting…" | Listener kept; connections drop |
| Remote folder picker (Open Remote) | List spinner per directory | Dialog error |

## 12. The spike

`tools/remote-spike/` is a standalone crate with its own `[workspace]`. It is
not a member of fastedit's workspace, nothing depends on it, and it isn't
shipped. It proves:

- framed multiplexing of **two channels over a child process's stdio**: the
  client spawns `remote-spike server` and talks to it over its stdin and
  stdout, or, with `--ssh`, runs it through `ssh -T`;
- **credits**, per channel, returned by the consumer;
- **cancel**, bounded by the window;
- an **injected-latency transport** (`Delayed`): a loopback relay that adds
  one-way latency each way and, optionally, a bandwidth cap;
- the **handshake**, where both peers refuse a build mismatch;
- a **fixture** that refuses any directory it didn't make and never deletes
  anything it didn't create.

Run it with:

```sh
cd tools/remote-spike
cargo test --release            # 4 unit + 8 end-to-end tests
cargo run --release -- codec    # § 2.3
cargo run --release -- bench    # the tables below; --quick for a short run
# over real ssh (the fixture must exist at the same path on the host):
#   remote-spike fixture --dir /tmp/remote-spike-fixture    (on the host)
#   remote-spike bench --latency 0 --ssh HOST --remote-bin /path/to/remote-spike \
#       --dir /tmp/remote-spike-fixture
```

The fixture is 50 × 20 directories of 50 empty files each: 50,000 files plus
1,050 directories, which makes 51,050 walk entries. Every entry is
`lstat`-ed, and the walk streams in batches of 512. There is also a 1 MiB
file.

**Measured** on 2026-11-20, from commit 68562b1e9; each cell is the median
of three runs. The client is an M3 Max (14 cores) on macOS 27 at load
13–27, with other lanes building. neon is Linux x86_64 (12 cores) at load
~12, reached with OpenSSH_10.3p1 over a direct Tailscale path (ICMP RTT
27–41 ms). The window is 1.25 MiB unless stated. "Echo beside bulk" keeps a
bulk stream on another channel consumed from before the first echo until
after the last, and the next column shows that stream's rate and its
longest delivery gap over the same window.

| link | connect | ping p50 / p99 | echo 1 KiB p50 | echo beside bulk p50 / p99 | bulk meanwhile: rate, longest gap | 1 MiB open p50 | walk: first batch / total (51,050 entries, 1.50 MiB on the wire) |
|---|---:|---:|---:|---:|---:|---:|---:|
| child stdio, 0 ms | 2.3 ms | 0.02 / 0.04 ms | 0.04 ms | 0.19 / 0.23 ms | 1,981 MiB/s, 0.07 ms | 0.51 ms | 1.6 / 169 ms |
| injected 5 ms one way | 10.3 ms | 10.0 / 13.7 ms | 10.1 ms | 10.1 / 10.2 ms | 93 MiB/s, 9.9 ms | 10.6 ms | 12.9 / 168 ms |
| injected 25 ms one way | 50.6 ms | 50.1 / 189.1 ms | 52.5 ms | 50.2 / 67.2 ms | 18 MiB/s, 61 ms | 50.7 ms | 53.0 / 296 ms |
| **real ssh to neon** | 338 ms | 34.1 / 44.2 ms | 35.0 ms | 50.3 / 109.1 ms | 14 MiB/s, 53 ms | 95.2 ms | 36.2 / 146 ms |

| link | cancel bulk: to End, data after | cancel walk after 1st batch: entries after |
|---|---:|---:|
| child stdio, 0 ms | 0.39 ms, 1.06 MiB | 0 |
| injected 5 ms | 10.1 ms, 1.13 MiB | 3,584 |
| injected 25 ms | 50.2 ms, 1.19 MiB | 11,776 |
| real ssh to neon | 67.4 ms, 0.94 MiB | 24,064 |

Throughput is in § 2.4. Over real ssh to neon, one bulk stream moved 1–5 /
3–4 / 1–10 / 1–6 MiB/s with 256 KiB / 1 / 1.25 / 4 MiB windows across the
three runs, in no order by window. A raw `ssh neon head -c 20000000`
(compression off) measured 2.8–15.2 MiB/s from one run to the next, from
`/dev/zero` and `/dev/urandom` alike. That link's rate, not the protocol,
sets the spread.

What the numbers say:

- **Round trips are the transport plus about 0.1 ms** at the median. Ping,
  echo and the handshake all sit within 0.5 ms of the injected RTT, apart
  from the 25 ms echo's 2.5 ms, and the p99s carry the loaded Mac's
  scheduling stalls.
- **A 1 MiB open costs one round trip** with a 1.25 MiB window: 50.7 ms at a
  50 ms RTT. A `--quick` run with a 1 MiB window took 100.6 ms, two round
  trips. Over neon it costs 95.2 ms (67–116 ms across runs): one 34 ms
  round trip plus ~60 ms of transfer, on a link whose raw rate swung
  2.8–15.2 MiB/s.
- **The 50k walk is bound by the host's `lstat`,** not the transport. The
  local walk with no transport took 120–239 ms between runs on the loaded
  Mac. Over ssh, neon's Linux walk delivered all 51,050 entries in 146 ms
  total, at a 34 ms RTT. The first batch arrives one RTT after the request,
  so Quick Open can fill progressively.
- **Cancel is prompt and bounded:** End arrives one RTT after the cancel on
  the injected links, and one to two (37–72 ms) over ssh. The data that
  still arrives is never more than the window. For a walk it is what the
  host produced in that time.
- **A bulk stream doesn't starve the interactive channel, and it isn't
  starved either:** the bulk stream kept flowing through every echo sample
  (longest gap about one RTT), and the echoes' median stayed within 0.2 ms of
  their idle round trip on the injected links and within 15 ms over ssh.
- **Window size is the latency knob on slow links,** hence the measured bulk
  window (§ 2.4).
- **ssh connect costs ~350 ms on a 34 ms link,** about 10 round trips'
  worth. That covers key exchange, authentication, the remote login shell
  (fish on neon) and the server's start. Reconnect pays it again, and the
  pill's states account for it.

Spike notes:

- `thread::sleep(5 ms)` took 40 ms on the loaded Mac (timer coalescing), so
  the latency relay spins (`yield_now`) to its deadlines instead of
  sleeping. Production code must not rely on short sleeps for timing
  either.
- The spike uses one thread per request and `String` paths. The real
  protocol crate uses the client's async executor and byte paths (§ 2.1).

## 13. Phases and cards

The "Linux" column says whether a card can be built and fully tested on a
Linux host. Most can, since everything below the UI is GPUI-free and the
protocol tests use a child-stdio transport.

| Phase | Card | Linux |
|---|---|---|
| 0 | This design and spike (T1986) | — |
| 1a | `fe-host` crate with `FsHost`, local impl moved from fe-project `listing.rs`/`fs_ops.rs`, the loader and fe-editor `disk.rs`. No behaviour change. | yes |
| 1b | `WatchHost` and `SearchHost` local impls (fe-project watcher, search, finder) | yes |
| 1c | `ProcessHost` local impl at every direct spawn site in fe-workbench and the git CLI sites | yes (workbench tests run headless) |
| 1d | `PtyHost` local impl in fe-terminal (`Session::spawn` through the trait) | yes |
| 1e | `GitHost` local impl over fe-git | yes |
| 1f | `ProjectLocation` type, persisted in recents, reopen, IPC, layout/backup/undo keys, trust keys, window title (local only) | yes |
| 2 | Protocol crate `fe-remote-proto`: frames, postcard bodies, mux, symmetric credits, bulk window sizing, cancel, heartbeat, handshake; `cargo fuzz` target, proptests, latency-transport tests | yes |
| 3 | `tenmoku-server`: daemon and `--stdio` attach, sessions, grace, Fs/Watch/Search services | yes (and macOS) |
| 4 | Client `RemoteHost` for Fs/Watch/Search, `SshTransport` with askpass, bootstrap and install; `release.yml` builds the server assets and signs `SHA256SUMS` | yes, with a fake transport; a real-sshd e2e on Linux CI |
| 5 | Open Remote UI: connect dialog, pill, first-connect install progress, recents, `ssh://` CLI, AI hidden in remote windows | logic yes; visuals need macOS screenshots |
| 6 | Remote processes: `RemoteLauncher` (fe-lsp), `RemoteAdapterLauncher` (fe-dap, with TCP relay), runs, tests, formatters | yes |
| 7 | Remote terminals and tasks (`PtyHost` over the protocol, the remote `tenmoku` shim) | yes |
| 8 | Remote git (`GitHost`, server askpass relay) | yes |
| 9 | Reconnect: session resume, resumable streams (offsets, acks, gaps), queued saves with `WriteExpect`, stamp conflicts; the acceptance cases below | yes |
| 10 | Port forwarding: explicit, detected, Ports panel | yes (detection: Linux `/proc`; macOS `lsof` path on macOS) |
| 11 | Remote trust keys and the remote-plugin rule | yes |
| 12 | Windows client: askpass channel on Windows, `RemotePath` through the editor, `WslTransport` | no (Windows) |
| after v1 | `DockerExecTransport` (dev containers); host-aware AI tools; host-side server fetch; local echo prediction | — |

Phase 9's acceptance cases, each run over the latency transport with the
link cut by the test:

- **Cut at a frame boundary, and cut inside a frame,** in each direction,
  while a pty streams output and a language server exchanges messages. After
  resume, the pty bytes the client got equal the bytes the program wrote,
  with none duplicated or missing. Every LSP message on both sides parses
  exactly once, and the requests in flight at the cut complete.
- **Edits made while disconnected** reach the retained language server as
  one full-text `didChange` per changed document, with increasing versions.
  The server's view of the text then matches the buffer, and diagnostics
  refresh.
- **Overflow:** a pty that writes more than 1 MiB while the link is down
  resumes with the reset, the "Output was lost" line and a `SIGWINCH`. A
  language server that writes more than 8 MiB is restarted and
  re-initialised, with every open document reopened.
- **New-file collision:** a new file's save queued during Reconnecting
  (`Absent`), while another writer creates that path on the host. On resume
  the save fails with `Conflict`, the other writer's file is unchanged, and
  the Overwrite / Cancel prompt appears. The same case covers the race
  window: the file is created between the server's check and its commit.
- **Changed file:** a pending `Matches` save to a file changed on the host
  meanwhile conflicts the same way, and a kept pending save restored after
  a client restart keeps its `WriteExpect`.

Phases 1a–1f and 2 are independent and can run in parallel. Phase 3 needs 2.
Phase 4 needs 1a, 1b, 2 and 3. Phases 6–11 need 4.

## 14. Known limits and open questions

- The archive size (10–15 MB) is an estimate until phase 3 builds a static
  server. If it comes out much larger, the upload time in § 4 grows with it.
- Remote terminals have no local echo prediction, so each keystroke echoes
  one RTT later (34 ms to neon). Prediction is a post-v1 card.
- Windows hosts aren't supported, other than WSL. Intel macOS hosts aren't in
  the v1 asset list, though adding `macos-x86_64` is one matrix row.
- The bulk window's estimate (2 × rate × min RTT) is the design. The spike
  measured only fixed windows, so phase 2 validates the estimator on the
  capped link.
