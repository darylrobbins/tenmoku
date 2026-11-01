//! The token probe: which of the workbench scene's pixels don't come from
//! the theme. docs/design-language.md says why, under Colour tokens.
//!
//! The scene renders under two probe themes that differ in every kiln token
//! and every editor and syntax colour. Theme A's channels are all at most
//! 100 and theme B's all at least 155, so any blend of A's colours is at
//! least 55 from any blend of B's, even where a glyph's edge mixes two
//! tokens. A pixel the same in both captures therefore took nothing from the
//! theme: a hard-coded colour. The check fails when there are more of those
//! than [`ALLOWED_FIXED_PIXELS`], so a new one has to be fixed or named here.

use std::{fs, path::PathBuf, sync::Arc};

use fe_config::{EditorColors, EditorPalette};
use fe_ui::{Appearance, Color, Theme, ThemeData};
use gpui::VisualTestAppContext;
use kiln_platform::screenshots::{CHANNEL_TOLERANCE, Result, RgbaImage, image::Rgba};
use toml::{Table, Value};

use crate::scenes::{SCENE, SCENE_SIZE, capture, write_project};

const MAGENTA: Rgba<u8> = Rgba([255, 0, 255, 255]);

/// Identical pixels the scene is allowed. T1273 measured none, so there's no
/// region to name: everything this scene draws follows the theme. It's in
/// device pixels, but zero is zero at any scale, so unlike the golden the check
/// holds on CI's 1× Mac (`SCREENSHOTS_ALLOW_OTHER_SCALE` doesn't excuse it).
/// Raising this means naming the new region's source here and under Gaps in the
/// design language, and scaling it for that Mac.
const ALLOWED_FIXED_PIXELS: u64 = 0;

/// The grid regions are found on, in device pixels: identical pixels this
/// close together are reported as one region, so a line of text is one.
const CELL: u32 = 8;

/// Hands out distinct colours: each channel is one of six levels, offset so
/// that all of theme A's are at most 100 and all of theme B's at least 155.
struct ProbeColors {
    next: u32,
    offset: u32,
}

impl ProbeColors {
    fn new(offset: u32) -> Self {
        Self { next: 0, offset }
    }

    fn next(&mut self) -> Color {
        let n = self.next;
        self.next += 1;
        assert!(n < 6 * 6 * 6, "more theme colours than probe colours");
        let channel = |digit: u32| digit * 20 + self.offset;
        let (r, g, b) = (channel(n % 6), channel(n / 6 % 6), channel(n / 36));
        Color::rgba(r << 24 | g << 16 | b << 8 | 0xff)
    }
}

/// Every kiln token and every editor and syntax colour replaced, the base's
/// other fields kept. Both go through the same path a theme file does, so a
/// colour added to either table is probed without a change here.
fn probe_theme(offset: u32) -> (Theme, EditorPalette) {
    let mut colors = ProbeColors::new(offset);
    let base = ThemeData::base(Appearance::Dark).clone();
    let tokens = base.tokens.map(|_| colors.next());

    let editor: Table = (EditorColors::KEYS.iter())
        .map(|key| (key.to_string(), Value::String(colors.next().to_string())))
        .collect();
    let data = ThemeData {
        extensions: [("editor".to_owned(), editor)].into_iter().collect(),
        ..base.clone()
    };
    let (mut palette, warnings) = EditorPalette::for_theme(&data);
    assert_eq!(warnings, [] as [String; 0], "the probe's editor table");
    for (_, color) in &mut palette.syntax {
        *color = colors.next();
    }
    (Theme::new(ThemeData { tokens, ..base }), palette)
}

/// The scene under the probe theme at `offset`, in a project of its own so
/// nothing the first capture leaves on disk reaches the second.
fn capture_under(cx: &mut VisualTestAppContext, offset: u32) -> Result<RgbaImage> {
    let (theme, palette) = probe_theme(offset);
    cx.update(|cx| {
        fe_ui::set_theme(cx, theme);
        fe_gpui::set_editor_palette(cx, Arc::new(palette.clone()));
    });
    let parent = tempfile::tempdir()?;
    let root = write_project(parent.path())?;
    capture(cx, &root, SCENE_SIZE, true, &palette)
}

fn identical(a: &Rgba<u8>, b: &Rgba<u8>) -> bool {
    a.0.iter()
        .zip(b.0)
        .all(|(&a, b)| a.abs_diff(b) <= CHANNEL_TOLERANCE)
}

/// A connected group of identical pixels: their bounding box and count.
#[derive(Debug, PartialEq)]
struct Region {
    min: (u32, u32),
    max: (u32, u32),
    pixels: u64,
}

/// Groups the `fixed` pixels of a `width` by `height` image by the [`CELL`]s
/// they fall in, cells that touch (corners too) making one region.
fn regions(fixed: impl Iterator<Item = (u32, u32)>, width: u32, height: u32) -> Vec<Region> {
    let (columns, rows) = (width.div_ceil(CELL), height.div_ceil(CELL));
    let mut cells: Vec<Option<Region>> = (0..columns * rows).map(|_| None).collect();
    for (x, y) in fixed {
        let cell = cells[(y / CELL * columns + x / CELL) as usize].get_or_insert(Region {
            min: (x, y),
            max: (x, y),
            pixels: 0,
        });
        merge(
            cell,
            &Region {
                min: (x, y),
                max: (x, y),
                pixels: 1,
            },
        );
    }
    let mut found = Vec::new();
    for start in 0..cells.len() {
        let Some(mut region) = cells[start].take() else {
            continue;
        };
        let mut stack = vec![start as u32];
        while let Some(cell) = stack.pop() {
            let (column, row) = (cell % columns, cell / columns);
            for dy in 0..3 {
                for dx in 0..3 {
                    let (x, y) = (column + dx, row + dy);
                    if x == 0 || y == 0 || x > columns || y > rows {
                        continue;
                    }
                    let neighbour = (y - 1) * columns + (x - 1);
                    if let Some(other) = cells[neighbour as usize].take() {
                        merge(&mut region, &other);
                        stack.push(neighbour);
                    }
                }
            }
        }
        found.push(region);
    }
    found
}

fn merge(into: &mut Region, other: &Region) {
    into.min = (into.min.0.min(other.min.0), into.min.1.min(other.min.1));
    into.max = (into.max.0.max(other.max.0), into.max.1.max(other.max.1));
    into.pixels += other.pixels;
}

/// Where `workbench_tokens.diff.png` goes: `target/screenshots` beside the
/// golden scene's failures. As in `Goldens::new`, `cargo run`'s run-time
/// `CARGO_MANIFEST_DIR` wins over a shared target's compile-time one.
fn out_dir() -> PathBuf {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
    manifest.join("../../target/screenshots")
}

/// Renders the two captures, writes the diff and prints what it found.
/// Returns whether the scene is within [`ALLOWED_FIXED_PIXELS`].
pub(crate) fn check(cx: &mut VisualTestAppContext) -> Result<bool> {
    let a = capture_under(cx, 0)?;
    let b = capture_under(cx, 155)?;
    assert_eq!(a.dimensions(), b.dimensions(), "the same scene, twice");

    let mut count = 0;
    let diff = RgbaImage::from_fn(a.width(), a.height(), |x, y| {
        let (pixel_a, pixel_b) = (a.get_pixel(x, y), b.get_pixel(x, y));
        if identical(pixel_a, pixel_b) {
            count += 1;
            MAGENTA
        } else {
            let [r, g, b, alpha] = pixel_a.0;
            Rgba([r / 3, g / 3, b / 3, alpha])
        }
    });
    let dir = out_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{SCENE}_tokens.diff.png"));
    diff.save(&path)?;

    println!("token probe: {count} identical pixels, allowed {ALLOWED_FIXED_PIXELS}");
    let fixed = diff
        .enumerate_pixels()
        .filter(|(_, _, pixel)| **pixel == MAGENTA)
        .map(|(x, y, _)| (x, y));
    for region in regions(fixed, a.width(), a.height()) {
        let Region { min, max, pixels } = region;
        println!("  {min:?}..={max:?}: {pixels} pixels");
    }
    println!("        diff:   {}", path.display());

    if count > ALLOWED_FIXED_PIXELS {
        println!(
            "FAILED  {SCENE} tokens: {count} pixels are the same under both probe themes, \
             allowed {ALLOWED_FIXED_PIXELS}: a colour bypasses the theme"
        );
        return Ok(false);
    }
    println!("ok      {SCENE} tokens");
    Ok(true)
}
