---
task_uid: "01m5jjkqm0yqagt8f4kqgn5ws5"
task: "T282"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "secrets::a_plugin_sets_gets_and_deletes_its_own_secret: round trip through the host import against MemoryStore"
    required: true
    manual: false
  - id: "C2"
    text: "secrets::entries_are_ext_id_dot_name_in_the_app_service: the store holds service = the app id and account = ext.\u003c"
    required: true
    manual: false
  - id: "C3"
    text: "secrets::one_plugin_cannot_read_anothers: plugin b gets none for a name plugin a set, including a.b + c versus a"
    required: true
    manual: false
  - id: "C4"
    text: "secrets::names_with_dots_or_bad_characters_are_refused: a.b, uppercase and a 65-character name are errors"
    required: true
    manual: false
  - id: "C5"
    text: "secrets::delete_all_removes_every_secret_of_one_plugin: after delete_all the plugin store and index rows are empt"
    required: true
    manual: false
  - id: "C6"
    text: "secrets::the_index_survives_a_restart: reopening PluginSecrets on the same index file finds the names"
    required: true
    manual: false
  - id: "C7"
    text: "secrets::an_unavailable_keychain_is_an_error: a store returning Unavailable or Denied reaches the guest as an err"
    required: true
    manual: false
  - id: "C8"
    text: "secrets::values_never_appear_in_debug_logs_or_the_wasi_environment: Debug of every type and the captured log lack"
    required: true
    manual: false
  - id: "C9"
    text: "cargo nextest run -p kiln-plugin and cargo clippy -p kiln-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Give each plugin its own secrets under ext.<id>.<name> with an index so uninstall can delete them all, and keep values out of logs and the WASI environment (Mac T1060)

## Problem

Each plugin gets its own secret store held by the host, readable only by that plugin, and never sees the app's
environment variables (decision 5). `kiln-core::secrets` (`SecretStore`, `MemoryStore`, the `ext.<extension id>.<name
convention) is the backend; the per-plugin interface and the deletion on uninstall are missing, and
the store cannot list entries, so something must remember each plugin's names.

## Change

`kiln/crates/kiln-plugin/src/secrets.rs` and `wit/secrets.wit`:

- `PluginSecrets::new(store: Arc<dyn SecretStore>, service, plugin_id, index_path)`: host imports `get(name)`,
  `set(name, value)`, `delete(name)` operate only on `ext.<plugin id>.<name>` in the app service. No capability is
  needed (decision 5 lists none). Names are `[a-z0-9_-]{1,64}`: a dot is refused so `a` + `b.c` cannot collide with
  `a.b` + `c`. A keychain `Unavailable` or `Denied` is returned to the plugin as an error, never a panic.
- An index file `<state dir>/plugin-secrets-index.toml` (path passed in, written under a lock and atomically) records
  each plugin's secret names, written before the store is touched, so `delete_all(plugin_id)` can remove every entry
  and the index row; uninstall (P17) calls it.
- `Debug`, logs and errors never contain a value (use `Secret`'s redacting `Debug`). The WASI environment stays empty
  (P13): no test or code path may put a secret in it.

## Notes

Tests use `MemoryStore` and a tempdir; never the real keychain.

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new

Lend note (helium lead): the lend file cut long lines (title, scope, some criteria and spec sentences); this card's title, scope and the ends of cut criteria were completed from the spec. The Mac copy is authoritative: if it differs, respec to it.
