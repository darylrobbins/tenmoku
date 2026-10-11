// SPDX-License-Identifier: LicenseRef-Proprietary

//! Pixel comparison of a capture against its golden.

use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};

/// How far each channel of a pixel may drift and still match (out of 255).
pub const CHANNEL_TOLERANCE: u8 = 2;

/// The share of pixels that may differ before a scene fails.
pub const MAX_DIFFERING: f64 = 0.001;

const MAGENTA: Rgba<u8> = Rgba([255, 0, 255, 255]);

/// Why a capture failed, with a diff image: the capture, dimmed, with
/// differing pixels in magenta. When the sizes differ the diff covers both
/// images, and the area only one of them has is magenta too.
pub struct Mismatch {
    pub kind: MismatchKind,
    pub diff: RgbaImage,
}

/// How a capture differs from its golden.
pub enum MismatchKind {
    Size {
        golden: (u32, u32),
        actual: (u32, u32),
    },
    /// Too many pixels differ.
    Pixels { differing: u64, total: u64 },
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            MismatchKind::Size { golden, actual } => write!(
                f,
                "size {}×{} differs from the golden's {}×{}",
                actual.0, actual.1, golden.0, golden.1
            ),
            MismatchKind::Pixels { differing, total } => write!(
                f,
                "{differing} of {total} pixels differ ({:.3}%, limit {:.3}%)",
                differing as f64 * 100. / total as f64,
                MAX_DIFFERING * 100.
            ),
        }
    }
}

/// Compares `actual` with `golden`, within [`CHANNEL_TOLERANCE`] and
/// [`MAX_DIFFERING`]. Images of different sizes never match.
pub fn compare(golden: &RgbaImage, actual: &RgbaImage) -> Result<(), Mismatch> {
    let width = golden.width().max(actual.width());
    let height = golden.height().max(actual.height());
    let mut differing = 0;
    let diff = RgbaImage::from_fn(width, height, |x, y| {
        let expected = golden.get_pixel_checked(x, y);
        let got = actual.get_pixel_checked(x, y);
        match (expected, got) {
            (Some(expected), Some(got))
                if expected
                    .0
                    .iter()
                    .zip(got.0)
                    .all(|(&a, b)| a.abs_diff(b) <= CHANNEL_TOLERANCE) =>
            {
                let [r, g, b, a] = got.0;
                Rgba([r / 3, g / 3, b / 3, a])
            }
            _ => {
                differing += 1;
                MAGENTA
            }
        }
    });
    let kind = if golden.dimensions() != actual.dimensions() {
        MismatchKind::Size {
            golden: golden.dimensions(),
            actual: actual.dimensions(),
        }
    } else {
        let total = u64::from(width) * u64::from(height);
        if differing as f64 <= total as f64 * MAX_DIFFERING {
            return Ok(());
        }
        MismatchKind::Pixels { differing, total }
    };
    Err(Mismatch { kind, diff })
}

/// Names the variable that lets a capture at another display scale than its
/// golden skip the pixel comparison instead of failing on size. CI sets it:
/// GitHub's macos-15 runners have a 1× display and the goldens are 2×. The
/// scene must still render and settle.
pub const ALLOW_OTHER_SCALE: &str = "SCREENSHOTS_ALLOW_OTHER_SCALE";

/// Whether `actual` should skip comparison with `golden`: it's another
/// size with the same aspect ratio, as at another scale, and
/// [`ALLOW_OTHER_SCALE`] is set.
pub fn skip_other_scale(golden: &RgbaImage, actual: &RgbaImage) -> bool {
    std::env::var_os(ALLOW_OTHER_SCALE).is_some() && other_scale(golden, actual)
}

fn other_scale(golden: &RgbaImage, actual: &RgbaImage) -> bool {
    golden.dimensions() != actual.dimensions()
        && u64::from(golden.width()) * u64::from(actual.height())
            == u64::from(golden.height()) * u64::from(actual.width())
}

/// Writes `<name>.actual.png` and `<name>.diff.png` to `dir`, returning
/// their paths.
pub fn save_failure(
    dir: &Path,
    name: &str,
    actual: &RgbaImage,
    mismatch: &Mismatch,
) -> image::ImageResult<(PathBuf, PathBuf)> {
    std::fs::create_dir_all(dir)?;
    let actual_path = dir.join(format!("{name}.actual.png"));
    let diff_path = dir.join(format!("{name}.diff.png"));
    actual.save(&actual_path)?;
    mismatch.diff.save(&diff_path)?;
    Ok((actual_path, diff_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            Rgba([(x * 7) as u8, (y * 13) as u8, 100, 255])
        })
    }

    #[test]
    fn identical_images_match() {
        assert!(compare(&image(40, 30), &image(40, 30)).is_ok());
    }

    #[test]
    fn pixel_within_tolerance_matches() {
        // Every pixel is off by the tolerance in one channel, so only the
        // channel tolerance (not the share allowed to differ) can pass it.
        let mut actual = image(40, 30);
        actual
            .pixels_mut()
            .for_each(|pixel| pixel.0[2] += CHANNEL_TOLERANCE);
        assert!(compare(&image(40, 30), &actual).is_ok());
    }

    #[test]
    fn pixel_beyond_tolerance_fails() {
        // One pixel of 100 is 1%, over the 0.1% allowance.
        let mut actual = image(10, 10);
        actual.get_pixel_mut(3, 4).0[2] += CHANNEL_TOLERANCE + 1;
        let Err(Mismatch {
            kind: MismatchKind::Pixels { differing, .. },
            diff,
        }) = compare(&image(10, 10), &actual)
        else {
            panic!("expected a pixel mismatch");
        };
        assert_eq!(differing, 1);
        assert_eq!(*diff.get_pixel(3, 4), MAGENTA);
        assert_ne!(*diff.get_pixel(0, 0), MAGENTA);
    }

    #[test]
    fn few_differing_pixels_pass() {
        // One pixel of 2000 is 0.05%, within the allowance.
        let mut actual = image(50, 40);
        actual.get_pixel_mut(3, 4).0 = [0, 0, 0, 0];
        assert!(compare(&image(50, 40), &actual).is_ok());
    }

    #[test]
    fn size_mismatch_fails() {
        let Err(Mismatch {
            kind: MismatchKind::Size { golden, actual },
            diff,
        }) = compare(&image(40, 30), &image(38, 31))
        else {
            panic!("expected a size mismatch");
        };
        assert_eq!((golden, actual), ((40, 30), (38, 31)));
        // The diff covers both images; only the shared area can match.
        assert_eq!(diff.dimensions(), (40, 31));
        assert_ne!(*diff.get_pixel(0, 0), MAGENTA);
        assert_eq!(*diff.get_pixel(39, 0), MAGENTA);
        assert_eq!(*diff.get_pixel(0, 30), MAGENTA);
    }

    #[test]
    fn other_scale_is_same_aspect_another_size() {
        assert!(other_scale(&image(80, 48), &image(40, 24)));
        assert!(!other_scale(&image(40, 24), &image(40, 24)));
        assert!(!other_scale(&image(80, 48), &image(40, 25)));
    }

    #[test]
    fn failures_save_actual_and_diff() {
        let dir = std::env::temp_dir().join(format!("kiln-screenshots-{}", std::process::id()));
        let black = RgbaImage::from_pixel(40, 30, Rgba([0, 0, 0, 255]));
        for (name, actual) in [("size", image(40, 31)), ("pixels", black)] {
            let mismatch = compare(&image(40, 30), &actual).expect_err("a mismatch");
            let (actual_path, diff_path) = save_failure(&dir, name, &actual, &mismatch).unwrap();
            assert_eq!(actual_path, dir.join(format!("{name}.actual.png")));
            assert_eq!(diff_path, dir.join(format!("{name}.diff.png")));
            assert_eq!(image::open(&actual_path).unwrap().into_rgba8(), actual);
            assert_eq!(image::open(&diff_path).unwrap().into_rgba8(), mismatch.diff);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
