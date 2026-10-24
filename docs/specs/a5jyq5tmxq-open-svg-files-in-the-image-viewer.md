---
task_uid: "01m5n9j05ndnrdhma5jyq5tmxq"
task: "T238"
rev: 2
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "An .svg or .svgz opens in the image viewer, drawn at its intrinsic size (viewBox fallback) with the viewer's zoom, checkerboard and status items, and re-rendered sharply after zoom settles; over 8 MiB or 100 megapixels is refused"
    required: true
    manual: false
  - id: "C2"
    text: "An SVG that links an existing file outside the workspace root and its own folder is not drawn (the Markdown preview's rule)"
    required: true
    manual: false
  - id: "C3"
    text: "image.open_as_text replaces an SVG viewer tab in place with an editor on the file and image.open_as_image goes back keeping the zoom; both do nothing on other formats (open_as_image is offered in the palette in any editor, as the editor's key context is outside this card); Pane::open_as_text opens any path as text"
    required: true
    manual: false
  - id: "C4"
    text: "New tests in svg_viewer_tests.rs and svg_view_as_text_tests.rs pass headless; the only Cargo change is usvg.workspace = true in fe-gpui (Apache-2.0 OR MIT, already in Cargo.lock); Cargo.lock changes only by the usvg dependency edge under fe-gpui, with no package added or version changed"
    required: true
    manual: false
---

# Open .svg files in the image viewer rendered with GPUI's SVG renderer, with Open as Text / Open as Image to switch to the source and back (Mac T1118)

## Problem

Follows the image viewer card. .svg is the one image format that is also text: VS Code opens it as text and offers a preview, but a browse-first fastedit should show the picture first and let the user switch to the source. GPUI renders SVG itself: `gpui::SvgRenderer` is `cx.svg_renderer()` (usvg + resvg, both Apache-2.0 OR MIT, already in the lock through gpui-pre 0.3.9, which exports `svg()`, `img()` and `Image::from_bytes(ImageFormat::Svg, ..)`), and crates/fe-markdown-view/src/image.rs already renders a local SVG with `renderer.render_single_frame(&bytes, 1.0)` at `SMOOTH_SVG_SCALE_FACTOR`.

## Change

1. `loader::is_image` also accepts `svg` (and `svgz`); the viewer decodes an SVG with the renderer as the Markdown preview does, sized by its intrinsic size (usvg's `Tree::size`; falls back to the viewBox, then 300 x 150, as browsers do), all of the image viewer's zoom, checkerboard, status bar items (size is the intrinsic size in px) and reload behaviour applying. `crates/fe-gpui/Cargo.toml` gains `usvg.workspace = true` (workspace dep already present, `usvg = { version = "0.46", default-features = false }`; license Apache-2.0 OR MIT; already in Cargo.lock, which does not change).
2. Safety, as the Markdown preview has it: before rendering, every `href` in the SVG is checked and an SVG linking to an existing file outside the workspace root and the SVG's own folder is not drawn ("Blocked: links a file outside the workspace"), because usvg would read that file. Copy `check_svg_links` from crates/fe-markdown-view/src/image.rs (private there; a later chore can share it); compressed (.svgz) input is decompressed with `usvg::decompress_svgz` first. An SVG over 8 MiB or with a render size over 100 megapixels is not drawn.
3. At zoom levels past the pre-rendered resolution the SVG is re-rendered at the displayed size (capped at 8192 px a side) once zooming settles for 150 ms, so it stays sharp; until then the bitmap scales.
4. View as text: commands `image.open_as_text` ("Open as Text") and `image.open_as_image` ("Open as Image"). On an SVG viewer tab the first replaces the tab in place with an editor tab on the same file (same position, same pane; the viewer's zoom is remembered for the way back); on an `.svg` editor tab the second does the reverse. A small "Text" button in the viewer's top-right corner and the palette run them; neither shows for other formats (the commands do nothing on them). Opening an `.svg` from the tree or quick open opens the viewer; `Pane::open_as_text(path)` (new) opens an editor tab for any path regardless of `is_image`, for this command only. The tab of an `.svg` open as text is an ordinary editor tab (edit and save as any file); if the same path is open in both forms in different panes each follows disk changes itself.
5. `crates/fe-workbench/src/command_catalog_tests.rs` lists both commands.

## Tests (new files)

- `crates/fe-gpui/src/svg_viewer_tests.rs` (#[path] mod in image_viewer.rs; headless): `svg_renders_at_its_intrinsic_size`, `viewbox_only_svg_sizes_from_the_viewbox`, `svgz_is_decompressed`, `a_link_outside_the_workspace_is_blocked`, `a_link_inside_the_folder_is_allowed`, `zoom_in_rerenders_after_settling`, `oversize_svg_is_refused`, `transparent_regions_show_the_checkerboard`.
- `crates/fe-workbench/src/svg_view_as_text_tests.rs` (declared in loader.rs under `#[cfg(test)]`): `svg_opens_in_the_viewer`, `open_as_text_replaces_the_tab_in_place_with_an_editor`, `open_as_image_goes_back_keeping_the_zoom`, `commands_do_nothing_on_a_png`, `editing_the_svg_text_and_saving_updates_the_viewer_on_return`.
Linux-OK (headless; resvg renders on the CPU). Depends on the image viewer card. Overlaps pane.rs (T247). Complexity m.

## Implementation notes

- A document with no viewBox and no `width`/`height` gets 300 x 150 by inserting those attributes on the root before it is parsed (so usvg, GPUI's renderer and the reported size agree); otherwise the size is `usvg::Tree::size`.
- `ImageViewer::in_workspace(path, zoom, root, ..)` carries the workspace root for the link rule; a saved layout keeps it (`root` in the viewer's state).
- svgz is unpacked by a small bounded inflater in `image_viewer.rs` (output cut off at 8 MiB), because `usvg::decompress_svgz` is unbounded and a gzip trailer can be forged. The first render is 2x, which GPUI caps at 8192 px a side (at most 67 megapixels).
- `image.open_as_text` applies in `ImageViewer && svg` (the viewer adds `svg` to its key context). `image.open_as_image` applies in `editor` and does nothing on a non-SVG: the editor's key context is not in this card's scope.
- `Pane::open_as_image` leaves an editor with unsaved changes in place, with a toast. A read for `open_as_text` is dropped if its tab has closed or moved, or by `open_as_image`.
- An ordinary open of an image path ignores a peer editor of it in another pane, so it opens the viewer.
- `Cargo.lock` gains the `usvg` edge under `fe-gpui`; no package version changes. The C4 wording "does not change" is about versions: a new dependency edge cannot be left out of the lockfile.
- Only an uncompressed `.svg` offers Open as Text (the Text button and the command); an `.svgz` is a viewer only, as editing and saving its gzip bytes as text would corrupt it.
- `image.open_as_image` is offered in the palette for any editor (its guard is the `.svg` path): the editor's key context is in `view.rs`, which another card holds.
