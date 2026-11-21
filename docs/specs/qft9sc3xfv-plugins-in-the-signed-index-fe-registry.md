---
task_uid: "01m71sznamjam346qft9sc3xfv"
task: "T1709"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-registry has plugin entries (id, version, API minor, sha256, capabilities summary) in the same signed index as grammars; an install is verified against its entry (hash and signature) before it is unpacked."
    required: true
    manual: false
  - id: "C2"
    text: "New tests in crates/fe-registry/tests/plugin_index.rs: `plugin_index_entry_round_trips`, `plugin_install_hash_mismatch_refused`, `plugin_install_tampered_index_refused`, `plugin_index_rollback_refused`."
    required: true
    manual: false
  - id: "C3"
    text: "A local (dev or folder) install is shown as 'unsigned' in plugin_manage and still allowed (Q4 follows the design's proposal; revisit after the API is stable)."
    required: true
    manual: false
  - id: "C4"
    text: "The signed archive size and entry count are capped before download is accepted."
    required: true
    manual: false
---

# Plugins in the signed index: fe-registry plugin entries, verified installs (A7, 4.6)

# Plugins in the signed index: fe-registry plugin entries, verified installs (A7, 4.6)

Draft id A7 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 4.6. Depends on T1481, T1483, T1484 for the registry crate and signing.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

**Known limit (lead, rev 4):** the app has no network plugin download path yet, and fe-workbench does not depend on fe-registry. This card delivers the library side: `fe_registry::plugins::verified_install` (signature, rollback and hash in one call) and fe-workbench's `install_listed`, exercised by tests. Wiring them into an app download flow (cached verified index with its persisted serial, capped download, registry-listed capabilities in the prompt) is T2076. A review finding that asks for that wiring is out of scope here.
