// SPDX-License-Identifier: Apache-2.0

//! A crate's golden screenshots: where they live, checking a capture
//! against one, and rewriting them.

use std::path::{Path, PathBuf};

use image::RgbaImage;

use super::{ALLOW_OTHER_SCALE, Result, compare, save_failure, skip_other_scale};

/// Names the variable that makes [`Goldens::check`] rewrite each golden
/// with its capture instead of comparing them.
pub const UPDATE: &str = "UPDATE_SCREENSHOTS";

/// A directory of golden PNGs, one per scene, and where failures go.
pub struct Goldens {
    dir: PathBuf,
    out_dir: PathBuf,
    update: bool,
}

impl Goldens {
    /// The goldens in `dir`, relative to the crate's directory:
    /// `Goldens::new(env!("CARGO_MANIFEST_DIR"), "examples/screenshots/golden")`.
    /// `cargo run` also sets `CARGO_MANIFEST_DIR` at run time, and that
    /// wins over the compile-time value, which a target directory shared
    /// between checkouts can leave pointing at another (possibly deleted)
    /// one. Failures go to `target/screenshots` in the crate's workspace.
    /// They're rewritten instead of compared when [`UPDATE`] is set.
    pub fn new(manifest_dir: &str, dir: &str) -> Self {
        let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
            .map_or_else(|| PathBuf::from(manifest_dir), PathBuf::from);
        Self {
            dir: manifest_dir.join(dir),
            out_dir: workspace_dir(&manifest_dir).join("target/screenshots"),
            update: std::env::var_os(UPDATE).is_some(),
        }
    }

    /// The goldens in `dir`, with failures written to `out_dir`, compared
    /// rather than rewritten whatever [`UPDATE`] says.
    pub fn at(dir: impl Into<PathBuf>, out_dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            out_dir: out_dir.into(),
            update: false,
        }
    }

    /// Sets whether [`Goldens::check`] rewrites the goldens.
    pub fn updating(mut self, update: bool) -> Self {
        self.update = update;
        self
    }

    /// Whether [`Goldens::check`] rewrites the goldens.
    pub fn is_updating(&self) -> bool {
        self.update
    }

    /// The golden for scene `name`.
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.png"))
    }

    /// Compares `image` with scene `name`'s golden, printing the result:
    /// `ok`, `skipped` for a golden at another display scale when
    /// [`ALLOW_OTHER_SCALE`] is set, or `FAILED` with the capture and its
    /// diff written to the output directory. When updating, writes
    /// `image` as the golden instead. Returns whether it matched; a
    /// missing golden is an error.
    pub fn check(&self, name: &str, image: &RgbaImage) -> Result<bool> {
        let golden_path = self.path(name);
        if self.update {
            std::fs::create_dir_all(&self.dir)?;
            image.save(&golden_path)?;
            println!("updated {}", golden_path.display());
            return Ok(true);
        }
        let golden = image::open(&golden_path)
            .map_err(|error| format!("reading {}: {error}", golden_path.display()))?
            .into_rgba8();
        if skip_other_scale(&golden, image) {
            println!(
                "skipped {name}: {:?} is the golden's {:?} at another scale ({ALLOW_OTHER_SCALE})",
                image.dimensions(),
                golden.dimensions(),
            );
            return Ok(true);
        }
        match compare(&golden, image) {
            Ok(()) => {
                println!("ok      {name}");
                Ok(true)
            }
            Err(mismatch) => {
                let (actual_path, diff_path) = save_failure(&self.out_dir, name, image, &mismatch)?;
                println!("FAILED  {name}: {mismatch}");
                println!("        actual: {}", actual_path.display());
                println!("        diff:   {}", diff_path.display());
                Ok(false)
            }
        }
    }
}

/// The workspace `manifest_dir` belongs to: the nearest directory at or
/// above it whose `Cargo.toml` has a `[workspace]`, or `manifest_dir`.
fn workspace_dir(manifest_dir: &Path) -> PathBuf {
    manifest_dir
        .ancestors()
        .find(|dir| {
            std::fs::read_to_string(dir.join("Cargo.toml"))
                .is_ok_and(|toml| toml.lines().any(|line| line.trim() == "[workspace]"))
        })
        .unwrap_or(manifest_dir)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kiln-goldens-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn workspace_dir_is_the_nearest_with_a_workspace() {
        let root = temp_dir("workspace");
        let krate = root.join("crates/a");
        std::fs::create_dir_all(&krate).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        std::fs::write(krate.join("Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();
        assert_eq!(workspace_dir(&krate), root);
        // Without one, failures go next to the crate.
        let alone = temp_dir("alone");
        assert_eq!(workspace_dir(&alone), alone);
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::remove_dir_all(&alone).unwrap();
    }

    #[test]
    fn check_updates_matches_then_fails_with_a_diff() {
        let dir = temp_dir("check");
        let goldens = Goldens::at(dir.join("golden"), dir.join("out")).updating(true);
        let gray = RgbaImage::from_pixel(20, 10, Rgba([90, 90, 90, 255]));
        assert!(goldens.check("scene", &gray).unwrap());
        assert!(goldens.path("scene").is_file());

        let goldens = goldens.updating(false);
        assert!(goldens.check("scene", &gray).unwrap());
        let white = RgbaImage::from_pixel(20, 10, Rgba([255, 255, 255, 255]));
        assert!(!goldens.check("scene", &white).unwrap());
        assert!(dir.join("out/scene.actual.png").is_file());
        assert!(dir.join("out/scene.diff.png").is_file());
        assert!(goldens.check("missing", &gray).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
