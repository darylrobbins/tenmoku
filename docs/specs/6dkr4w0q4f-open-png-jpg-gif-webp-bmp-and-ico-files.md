---
task_uid: "01m5n9hzxyp4nq0h6dkr4w0q4f"
task: "T237"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "Opening a file with an image extension makes an Image Viewer tab (never an editor), restored by saved layouts, never dirty, splittable; the file is decoded on the background executor, refused over 64 MiB or 100 megapixels, with an error message when undecodable"
    required: true
    manual: false
  - id: "C2"
    text: "Zoom: starts at Fit (down only), image.zoom_in/out/fit/actual (secondary-=, secondary--, secondary-0, secondary-1 in the ImageViewer context), ctrl/cmd-wheel and pinch around the pointer, click toggles Fit and 100%, scroll and drag pan; transparency shows a checkerboard"
    required: true
    manual: false
  - id: "C3"
    text: "The status bar shows pixel size, file size and zoom (clicking zoom toggles Fit and 100%); the viewer reloads on activation or window focus when the file changed"
    required: true
    manual: false
  - id: "C4"
    text: "New tests in image_viewer_tests.rs, image_viewer_open_tests.rs and image_viewer_keys.rs pass headless; no new dependency and no screenshot golden changes"
    required: true
    manual: false
---

# Open .png, .jpg, .gif, .webp, .bmp and .ico files in an image viewer tab: fit and zoom, a checkerboard for transparency, and the size in the status bar, as VS Code does (Mac T1117)

## Problem

User-approved 2026-10-23 (browse-first product: images should open like files). Opening a .png in fastedit reads the bytes as lossy text into an editor tab (`loader::open`, crates/fe-workbench/src/loader.rs; `Pane::file_landed`, pane.rs:897). VS Code opens images in an Image Preview tab: the picture centred on a checkerboard, zoom by scroll/click, and the size shown in the status bar. fastedit already draws images in Markdown preview (crates/fe-markdown-view/src/image.rs: GPUI's `Image::from_bytes(format, bytes).to_image_data(cx.svg_renderer())` giving an `Arc<RenderImage>`), so decoding for .png/.jpg/.jpeg/.gif/.webp/.bmp/.ico needs no new dependency. SVG is the next card.

## Change

1. `crates/fe-workbench/src/loader.rs`: `Opened::Image` (no payload: the viewer reads the file itself, off the main thread); `open(path)` returns it, without reading, for a path with extension png, jpg, jpeg, gif, webp, bmp or ico (case-insensitive; `pub(crate) fn is_image(&Path)`). Handle the new variant where `Opened` is matched exhaustively: pane.rs `file_landed` (:897-903, a new `new_image_viewer`), workbench.rs launch match (:346-350), crates/fastedit/src/main.rs `loaded` (:45-65).
2. New `crates/fe-gpui/src/image_viewer.rs` and `image_viewer/pane_item.rs` (`pub use ImageViewer` in lib.rs), a `PaneItem` with `KIND = "fastedit.image_viewer"`, registered in crates/fe-workbench/src/lib.rs next to `FileViewer` so saved layouts restore it (serialize the path and zoom mode; a missing file isn't restored). Like the file viewer it is never dirty, has no save, and splits into another viewer of the same file.
3. The view: decode on the background executor (file over 64 MiB or over 100 megapixels shows "Too large to preview" instead; an undecodable file shows "Can't display this image: <reason>"); the first frame of an animated GIF/WebP is shown, as the Markdown preview does. Background is a checkerboard (two theme-derived greys, 8 px squares) drawn under the image so transparency shows, the area around the image the editor background. Zoom as VS Code's image preview: starts at "Fit" (scaled down to the viewport, never up past 100%); `image.zoom_in`/`image.zoom_out` step through 10%..2000% (25, 50, 75, 100, 150, 200, 300, 400, 800, ...), `image.zoom_fit` ("Fit to Window") and `image.zoom_actual` ("Actual Size", 100%); ctrl/cmd + scroll wheel and trackpad pinch zoom around the pointer; a click on the image toggles Fit and 100% as VS Code's does; when larger than the viewport it scrolls both ways and a drag pans. A 100% image is one image pixel per logical point.
4. Status bar items (kiln `StatusItem`s in `PaneItem::status_items`): the pixel size `640 × 480`, the file size (`1.2 MB`, same formatter as the large-file banner `size_text`), and the zoom (`Fit` or `150%`, clicking it runs `image.zoom_fit`/`image.zoom_actual`). The cursor/encoding items of editors are not shown.
5. The viewer reloads when the file changed on disk (its length or mtime differs) whenever it is activated or the window regains focus, keeping the zoom; a deleted file shows "File not found".
6. Commands `image.zoom_in`, `image.zoom_out`, `image.zoom_fit`, `image.zoom_actual` in a key context `ImageViewer` set on the view's element, bound in crates/fe-keymap/keymaps/fastedit.toml to secondary-= (and secondary-+), secondary--, secondary-0 (fit) and secondary-1 (actual) in that context only (the editor's keys are untouched); listed in `crates/fe-workbench/src/command_catalog_tests.rs`.

## Tests (new files)

- `crates/fe-gpui/src/image_viewer_tests.rs` (#[path] mod at the end of image_viewer.rs; headless GPUI tests, images written to a tempdir with the `image` crate's PNG encoder, which fe-gpui already has): `png_decodes_and_reports_its_dimensions`, `fit_scales_a_large_image_down_and_never_a_small_one_up`, `actual_size_is_one_pixel_per_point`, `zoom_steps_stop_at_the_ends`, `wheel_zoom_keeps_the_pointer_point_fixed`, `click_toggles_fit_and_actual`, `drag_pans_a_zoomed_image`, `transparent_png_paints_the_checkerboard` (painted checkerboard bounds, no pixel golden), `undecodable_file_shows_the_reason`, `status_items_show_dimensions_size_and_zoom`, `reload_on_activation_after_the_file_changed`, `too_large_is_refused`, `restore_round_trips_the_path_and_zoom`.
- `crates/fe-workbench/src/image_viewer_open_tests.rs` (declared in loader.rs under `#[cfg(test)]`, as its other tests are): `opening_a_png_makes_an_image_tab_not_an_editor`, `an_uppercase_extension_opens_the_viewer`, `a_txt_file_still_opens_in_an_editor`, `reopening_activates_the_same_tab`, `quick_open_and_the_tree_open_images_in_the_viewer`.
- `crates/fe-keymap/tests/image_viewer_keys.rs`: the four keys resolve in the `ImageViewer` context on macOS and Linux and not in the editor's.
Linux-OK yes (no screenshot goldens; a Mac-only golden of the checkerboard would be its own small card). No new dependency (decoding is GPUI's, `image` and `gpui` are already dependencies of fe-gpui). Overlaps: pane.rs and workbench.rs (T247), keymaps/fastedit.toml (T937, T1077), view/PaneItem files are new. Complexity m.
