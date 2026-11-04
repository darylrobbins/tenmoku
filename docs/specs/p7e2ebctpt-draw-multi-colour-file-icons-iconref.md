---
task_uid: "01m6j5s65byaaapmp7e2ebctpt"
task: "T282"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::coloured_icons::coloured_ref_draws_as_image_on_row_tab_and_crumb: a stub set answering IconRef::Coloured draws an image element, not svg(), on a tree row, a tab and a crumb"
    required: true
    manual: false
  - id: "C2"
    text: "tests::coloured_icons::masked_asset_still_tinted: IconRef::Asset still draws svg() in the surface text colour"
    required: true
    manual: false
  - id: "C3"
    text: "tests::coloured_icons::ignored_row_dims_coloured_icon: an ignored row draws a coloured icon at reduced opacity"
    required: true
    manual: false
  - id: "C4"
    text: "tests::coloured_icons::assets_serve_registered_and_lucide: Assets::load serves a registered coloured path and every Lucide Icon"
    required: true
    manual: false
  - id: "C5"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Draw multi-colour file icons: IconRef::Coloured drawn as an image, kiln-ui tree rows, tabs and crumbs use it, Assets serves coloured files (Mac T1359)

## Problem

Every file icon is a Lucide stroke drawn by `svg()` as an alpha mask in the text colour, so an icon can have no colour of its own. A coloured per-language set (Material Icon Theme style) needs a second draw path, and every surface that draws a file or folder icon draws it itself with `svg().path(icon.path())`.

## Change

In `kiln/crates/kiln-ui/src/file_icons.rs` and `assets.rs`:

- Add `IconRef::Coloured(SharedString)`: an asset path to a multi-colour SVG that is drawn as an image, never tinted. `IconRef::path()` and `name()` answer its path as they do for `Asset`; `bundled()` answers `None`. The existing `Asset` (masked) variant is unchanged.
- Add `IconRef::element(&self, size: Pixels, tint: Hsla) -> AnyElement` (name may differ): `svg().path(..).text_color(tint)` for `Bundled`/`Asset`, and for `Coloured` `img(..)` at `size` (precedent: the Markdown task check in `crates/fe-markdown-view/src/blocks.rs` uses `img(Arc::new(Image::from_bytes(ImageFormat::Svg, ..)))`). The implementer checks whether gpui `img()` with an embedded asset path renders an SVG from `Assets`; if not, load the bytes once through the asset source into a cached `Arc<Image>` per path. The SVG must be rasterised sharply at 1x and 2x at 14 and 16 px.
- `Assets::load` serves a second table of coloured icons (empty in this card; card C fills it) via a small registration point, so a host asset source that delegates to `kiln_ui::Assets` serves them too.
- Move kiln-ui's own surfaces to `IconRef::element`: tree rows (`tree_row.rs`), tabs (`tab_bar.rs`), breadcrumbs (`breadcrumbs.rs`). An unreadable row's `status.error` tint and the dimmed (ignored) state still apply: for a coloured icon the ignored state reduces opacity instead of changing colour.

Out of scope: the fe-workbench surfaces (card B), the actual icons (card C), the setting (card D).

## Tests

New `kiln/crates/kiln-ui/src/tests/coloured_icons.rs` (declared in `tests/mod.rs`): a stub `IconSet` answering `IconRef::Coloured` renders a tree row, a tab and a crumb as an image element (not `svg()`) at the surface's size; the same stub's masked `Asset` still tints with the row colour; an ignored row's coloured icon has reduced opacity; `Assets::load` serves a registered coloured path and still serves every Lucide `Icon`.

## Implementation notes

- gpui's `img()` with an embedded asset path does load an SVG through the app's asset source, but rasterises it at scale 1.0 from the SVG's own size (24 px for a 24-unit viewBox), so it'd be resampled on screen and soft at 2x; `Image::from_bytes(ImageFormat::Svg, ..)` does the same. `IconRef::element` instead hands `img()` a custom loader that rasterises the SVG with the app's `SvgRenderer` at exactly `size × window.scale_factor()` device pixels (`SvgSize::ExactSize`), and caches the `Arc<RenderImage>` per (path, side) in an app global (`ColouredImages`), failures included, so each frame reuses one atlas texture.
- The bytes come from the app's asset source, else from `kiln_ui::Assets`, so kiln-ui's own coloured icons draw under a host source that doesn't delegate.
- Registration point: `Assets::register_coloured(&[(path, svg)])`, process-wide (assets load on background threads). A bundled `Icon` path still serves the bundled file; `list` includes registered paths. A registration that adds a path or changes a path's file bumps a revision; the image cache empties when it sees a new one, so a replaced icon, or one that was missing and cached as a failure, draws from the new file the next time it's drawn.
- Surfaces wrap `icon.element(..)` in a sized div that carries their existing debug selector (`tree-icon:…`, `tab-icon:…`, `crumb-icon:…`). A tree row's `RowColors` gains `icon_opacity`: 0.5 (`DIMMED_ICON_OPACITY`) for a coloured icon on an ignored, readable row, else 1; a masked icon is muted or error-tinted as before.
- Tests draw each surface in a `HeadlessAppContext` whose asset source is `kiln_ui::Assets` and whose renderer records the scene: masked icons are checked as monochrome sprites in the surface's tint (row, ignored row, error row, tab, crumb), coloured ones as one polychrome sprite at the surface's side in device pixels, its atlas tile rasterised at that side with both of the stub SVG's colours, faded on an ignored row. Removing the tint, the row's opacity or the cache invalidation each fails a test. The recorder's `RgbaImage` return type needs `image` as a kiln-ui dev-dependency (Cargo.toml, outside the listed scope). `coloured_icon_rasterised_at_device_size` covers 14 and 16 px at 1x and 2x.
