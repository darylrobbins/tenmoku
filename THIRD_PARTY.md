# Third-party code

## Code copied or adapted into this repository

| Our path | Source | Commit | License |
|---|---|---|---|
| _none yet_ | | | |

## Shipped MPL-2.0 dependencies (unmodified)

MPL-2.0 requires telling recipients where to get the source of these crates.

| Crate | Version | Source | Pulled in by |
|---|---|---|---|
| dwrote | 0.11.5 | https://crates.io/crates/dwrote | zed-font-kit (Windows only) |
| option-ext | 0.2.0 | https://crates.io/crates/option-ext | dirs-sys via zed-font-kit |

`cbindgen` (MPL-2.0) is a build-time tool only and is not distributed.

## Dependencies

Run `cargo deny list` for every crate and its license. Release builds must ship
the license texts. Generating `THIRD_PARTY_LICENSES` is a release-checklist item.
