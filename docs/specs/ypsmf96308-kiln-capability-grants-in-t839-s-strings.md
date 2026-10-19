---
task_uid: "01m57yv417ky2n5pypsmf96308"
task: "T191"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln-command test capabilities_round_trip_t839_strings (tests/capability.rs): buffer.read, buffer.write, commands, decorations, status, fs.read:workspace, fs.write:workspace, process.spawn:rg, process.spawn:*, net:api.anthropic.com and net:* parse and print back unchanged; net:Api.Anthropic.COM prints lower-cased; net:https://x, net:x:443, net:x/y, fs.read:home and an unknown name are errors naming the string"
    required: true
    manual: false
  - id: "C2"
    text: "kiln-command test grants_allow_what_they_name (tests/capability.rs): grants [buffer.read, net:api.anthropic.com] allow those two and not net:example.com or buffer.write; net:* allows any host; net_hosts() reports the named hosts and the wildcard; Grants round-trips through serde as a list of strings"
    required: true
    manual: false
  - id: "C3"
    text: "kiln-core test memory_store_gets_sets_and_deletes (tests/secrets.rs): set then get returns the value, another account gets None, delete then get is None, deleting a missing entry is Ok, and a Secret's Debug output doesn't contain the value"
    required: true
    manual: false
  - id: "C4"
    text: "KeychainStore uses keyring-core with apple-native-keyring-store (macOS), windows-native-keyring-store (Windows) and zbus-secret-service-keyring-store (other unix) as target-specific dependencies; Cargo.lock has no dbus-secret-service-keyring-store or libdbus-sys; without a Secret Service on Linux it returns Unavailable and doesn't panic; keychain_round_trip_live exists and is ignored"
    required: true
    manual: false
  - id: "C5"
    text: "cargo deny check passes with deny.toml unchanged; scripts/check_kiln_boundary.py and scripts/check-no-gpui.sh pass; every new kiln file starts with the SPDX line"
    required: true
    manual: false
  - id: "C6"
    text: "kiln-core's secrets.rs module doc states the convention (service = the bare app id, account = a per-key name such as chat.anthropic), and no service or account string is hard-coded in kiln-core"
    required: true
    manual: false
---

# kiln: capability grants in T839's strings, and a SecretStore over the platform keychain (keyring-core) with a memory store (Mac T963)

# kiln: capability grants in T839's strings, and a secret store over the platform keychain

## Problem

The AI chat decision record (`docs/decisions/*-ai-chat.md`, the task this depends on; signed
off by the user) makes the built-in chat a first-party plugin compiled in: it holds the
capabilities a plugin's manifest would (`buffer.read`, `net:api.anthropic.com`, in the strings
of the plugin record T839, `docs/decisions/vmtnmkay62-extensions-and-plugins.md` §7), granted
by the app, and they're checked where a plugin's would be (record §9). It also keeps the
Anthropic API key in the macOS Keychain, or the platform's equivalent, never in a settings
file (record §7); T839 needs the same store for per-plugin secrets.

Neither exists. `kiln_platform::Capabilities` (`kiln/crates/kiln-platform/src/capabilities.rs`)
is unrelated: what a platform draws natively. Nothing in the repo touches a keychain.

## Change

**`kiln/crates/kiln-command/src/capability.rs`** (new; re-exported from `lib.rs`):

- `Capability`: `BufferRead`, `BufferWrite`, `Commands`, `Decorations`, `Status`,
  `FsRead(FsScope)`, `FsWrite(FsScope)`, `ProcessSpawn(Target)`, `Net(Target)`, where
  `FsScope` is `Workspace` and `Target` is `Named(String)` or `Any` (the `*` wildcard).
  `FromStr` and `Display` round-trip T839's strings: `buffer.read`, `buffer.write`, `commands`,
  `decorations`, `status`, `fs.read:workspace`, `fs.write:workspace`,
  `process.spawn:<program>`, `process.spawn:*`, `net:<host>`, `net:*`. A host is lower-cased,
  has no scheme, port or path; anything else is an error naming the string.
- `Grants`: a set of capabilities with `allows(&Capability) -> bool` (`net:*` allows every
  `net:<host>`, `process.spawn:*` every program) and `net_hosts()`, the named hosts plus
  whether `*` is granted. Serde as a list of strings.
- Docs: what each capability means, quoting T839, and that the plugin host's checks will use
  this type.

**`kiln/crates/kiln-core/src/secrets.rs`** (new; re-exported from `lib.rs`):

- `trait SecretStore: Send + Sync { get(service, account) -> Result<Option<Secret>>,
  set(service, account, &str) -> Result<()>, delete(service, account) -> Result<()> }`;
  deleting a missing entry is `Ok`. `Secret` holds the string and its `Debug` prints
  `Secret(<redacted>)`. `SecretError` (`Unavailable` when the platform has no store or it
  refuses, `Denied` when the user declined access, `Other`), via thiserror.
- `KeychainStore::new()`: **keyring-core** (MIT OR Apache-2.0) with
  `apple-native-keyring-store` on macOS, `windows-native-keyring-store` on Windows and
  `zbus-secret-service-keyring-store` on other unix (pure-Rust D-Bus). Never
  `dbus-secret-service-keyring-store` and never a `vendored` feature (it builds libdbus, under
  AFL-2.1 OR GPL-2.0). Target-specific dependencies in `kiln-core/Cargo.toml`, versions in the
  workspace `Cargo.toml`. Prefer making entries from a store value over keyring-core's
  process-wide default store; if the API needs the default, set it once inside
  `KeychainStore::new` and say so in the finish notes. On Linux without a Secret Service
  (headless boxes, CI) `new` or the first call returns `Unavailable`, never panics.
- `MemoryStore`: a `Mutex<HashMap>` store for tests and for hosts with no keychain.
- **Naming convention** (record, user decision 3): every secret an app keeps uses the **bare app id as
  the service** (fastedit's is `dev.fastedit.fastedit`, `fe_config::paths::app_id()`), never a
  per-feature service, and a **per-key account name**: the chat's is `chat.anthropic`, an
  extension's would be `ext.<id>.<name>`. The trait takes both as arguments and stays agnostic;
  `secrets.rs`'s module doc states the convention so later extension-secret cards share one
  service. Nothing in this card hard-codes a service or account.

Run `cargo deny check` and record the new crates and their licences in the finish notes; if
any needs a licence `deny.toml` doesn't allow, stop and ask rather than editing `deny.toml`.

Tests: `kiln/crates/kiln-command/tests/capability.rs` and `kiln/crates/kiln-core/tests/secrets.rs`
(top-level fns). The keychain round trip is one `#[ignore]`d test,
`keychain_round_trip_live`, for a person to run on macOS (it may raise a Keychain prompt).
Every new kiln file starts with the SPDX line; `scripts/check_kiln_boundary.py` must pass.

## Implementation notes

- `Target` clashes with `kiln_command::Target` (an invocation's target), so it stays at
  `kiln_command::capability::Target`; `Capability`, `CapabilityError`, `FsScope`, `Grants` and
  `NetHosts` are re-exported from the crate root. `net:*.example.com` and IDN hosts are refused
  (punycode only); `Grants::allows` compares hosts without case.
- The per-OS store choice and error reading live in `kiln-core/src/platform/keychain.rs`, not
  `secrets.rs`: `scripts/check_kiln_boundary.py` allows OS branches only under `src/platform`.
  That file and a `mod` line in `platform/mod.rs` are outside the card's listed scope.
- Entries are built from the store value (`CredentialStoreApi::build`); keyring-core's default
  store is never set.
- `Denied`: macOS errSecUserCanceled (-128) and errSecAuthFailed (-25293), read by downcasting
  to `security_framework::base::Error`; the Secret Service's dismissed prompt
  (`secret_service::Error::Prompt`). Hence the direct `security-framework` (macOS) and
  `secret-service` (other unix) dependencies, at the versions the stores use. Windows'
  Credential Manager doesn't prompt. Any failure opening the store is `Unavailable`.
- `keychain_without_secret_service_is_unavailable` (Linux/BSD) reruns the test binary with
  `DBUS_SESSION_BUS_ADDRESS` pointed at a missing socket. The ignored live test also returns
  `Unavailable` on this Linux box (no `org.freedesktop.secrets`); it hasn't been run against a
  real keychain. macOS and Windows were checked with `cargo clippy --target`, not run.
- New crates, all MIT OR Apache-2.0: keyring-core 1.0.1, apple-native-keyring-store 1.0.2,
  windows-native-keyring-store 1.1.0, zbus-secret-service-keyring-store 1.0.1,
  secret-service 5.2.0, security-framework 3.7.0, security-framework-sys 2.17.0, and
  RustCrypto's aes, cbc, cipher, hkdf, hmac, sha2 0.11, digest 0.11, block-buffer,
  block-padding, crypto-common, inout, hybrid-array, const-oid, cmov, ctutils, cpubits,
  cpufeatures 0.3. `cargo deny check` passes with `deny.toml` unchanged; no
  `dbus-secret-service-keyring-store`, `libdbus-sys` or openssl in `Cargo.lock`.
