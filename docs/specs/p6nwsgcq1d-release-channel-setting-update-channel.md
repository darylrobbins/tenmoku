---
task_uid: "01m7whzb8phd475wp6nwsgcq1d"
task: "T389"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "`[update] channel = \"stable\" | \"preview\"` parses into `UpdateSettings.channel` (enum `ReleaseChannel`, default Stable); an unknown value is a settings diagnostic and leaves Stable; the field's doc comment (which the Settings editor shows) says in two sentences that preview builds arrive more often and may be less polished, and that switching back to stable waits for the next stable release that is newer than what is installed, never downgrading; settings-template.toml documents it commented out beside automatic_checks. Tests in crates/fe-config/tests/update_channel_setting.rs: `channel_defaults_to_stable`, `channel_preview_parses`, `channel_unknown_value_is_a_diagnostic_and_stays_stable`, `a_project_file_cannot_set_the_channel` (user-only, like icon_theme)."
    required: true
    manual: false
  - id: "C2"
    text: "A new `ReleaseVersion { major, minor, patch, pre: Option\u003cu64\u003e }` in crates/fe-config/src/whats_new.rs with `parse_release_version(\u0026str)` accepting `X.Y.Z` and `X.Y.Z-preview.N` (N plain digits, no leading zero) and a total order where `X.Y.Z-preview.N` \u003c `X.Y.Z` and previews order by N; the existing `parse_version` and its callers are unchanged. Tests in crates/fe-config/tests/release_version.rs: `preview_sorts_before_its_stable`, `previews_order_by_number`, `next_minor_preview_is_newer_than_the_stable`, `malformed_prerelease_is_refused` (`0.3.0-preview`, `0.3.0-preview.01`, `0.3.0-beta.1`, `0.3.0-preview.1.2`)."
    required: true
    manual: false
  - id: "C3"
    text: "`cargo test -p fe-config` passes and `cargo clippy -p fe-config --all-targets` is clean."
    required: true
    manual: false
---

# Release channel setting: update.channel (stable|preview) and prerelease-aware versions (channels 1) (Mac T1993)

# Release channel setting and prerelease versions

The user: "I can release previews frequently, but the stable releases are less frequent. People could choose the preview channel from their editor and get those more frequent updates."

## Design (shared by the channel cards CH1..CH11)
- Versions: stable `vX.Y.Z`; previews `vX.Y.Z-preview.N` (GitHub prerelease), cut from main with the workspace version set to that string. A preview is older than its own stable, newer than the previous stable. Never downgrade: after stable 0.3.0, the next preview is 0.4.0-preview.1.
- One setting, `update.channel`, read by every updater. Stable users never see a preview.
- The preview channel's feed is a superset: it carries the newest build overall, preview or stable (a stable release is published to both channels). So a preview user on 0.3.0-preview.3 is offered 0.3.0 when it ships, and is back on stable with no downgrade; to return to stable at once a user installs the stable download over it (documented in CH11).
- macOS: a second Sparkle appcast for preview (the app's feed URL chosen by the delegate from the setting), not `sparkle:channel` items, because the stable appcast lives at GitHub's releases/latest/download, which never lists prereleases and must not be re-uploaded on every preview.
- Windows/Linux: Velopack channels `<os>-<arch>-preview` beside `<os>-<arch>-stable`.

## This card
The setting and the version type. Nothing reads the setting yet. Files read first: crates/fe-config/src/settings.rs (UpdateSettings, around line 684), crates/fe-config/src/whats_new.rs (parse_version), crates/fe-config/tests/update_settings.rs (the model for the tests), crates/fe-config/settings-template.toml (line ~479).


Read the files named before starting. Do not widen --scope; if a file outside it is needed, stop and respec. New tests go in new files.
