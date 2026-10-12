//! How large a file may be and still get a full syntax tree.
//!
//! A tree costs a multiple of its source in memory, and the multiple depends
//! on the language: about 30× for pretty-printed JSON, 35–40× for Rust and
//! 70–85× for Markdown (PERF.md, "Tree memory": malloc growth across a parse,
//! printed by `cargo bench -p fe-syntax`). Every language gets the same tree
//! budget, [`TREE_BUDGET_BYTES`], so its parse limit is the budget divided by
//! its ratio. Those three ratios are hard-coded from that section, using the
//! largest typical ratio for each language. No ratio is measured at runtime.
//!
//! The other languages' ratios come from `cargo bench -p fe-syntax --bench
//! tree_ratio` (PERF.md, "Tree memory by language"), which parses each one's
//! golden sample repeated to about 4 MB. Each is the measured ratio rounded
//! up, and at least [`MIN_RATIO`].
//!
//! The limits bound the tree of a typical file, not the worst. A JSON array of
//! single digits costs about 96× its source and still reaches ~960 MB at
//! JSON's 10 MiB limit; that was accepted.
//!
//! A file over its language's limit has no tree. A window around the visible
//! text is parsed instead (see `Syntax::window_job`).

use crate::Language;

/// The most memory a file's syntax tree may take, near enough, below its
/// language's parse limit.
pub const TREE_BUDGET_BYTES: usize = 300 * 1024 * 1024;

/// Tree bytes per source byte for a language not in [`RATIOS`]. It uses the
/// code ratio (Rust).
const DEFAULT_RATIO: usize = 40;

/// The smallest ratio in [`RATIOS`], so a sample that happens to be cheap
/// can't lift a limit past 15 MiB.
const MIN_RATIO: usize = 20;

/// Tree bytes per source byte, by language name. Rust, JSON and Markdown are
/// PERF.md, "Tree memory" rows: Rust "generated items", JSON "pretty-printed
/// objects", Markdown "short paragraphs". The rest are PERF.md, "Tree memory
/// by language", measured on helium.
const RATIOS: &[(&str, usize)] = &[
    ("rust", 40),
    ("json", 30),
    ("markdown", 85),
    ("python", 44),
    ("javascript", 36),
    ("typescript", 40),
    ("tsx", 35),
    ("toml", 36),
    ("go", 39),
    ("c", 29),
    ("cpp", 33),
    ("bash", 45),
    // Measured 18.5×.
    ("html", MIN_RATIO),
    ("css", 26),
    ("yaml", 54),
    ("java", 27),
    ("ruby", 69),
    ("lua", 41),
    ("php", 44),
    ("csharp", 45),
    ("scala", 54),
    ("xml", 46),
    ("swift", 49),
    ("kotlin", 55),
    ("sql", 42),
    ("dockerfile", 24),
    ("make", 33),
];

impl Language {
    /// The largest file, in bytes, that is parsed whole:
    /// `TREE_BUDGET_BYTES / ratio` for this language.
    pub fn parse_limit(&self) -> usize {
        let name = self.name();
        let ratio = RATIOS
            .iter()
            .find_map(|&(n, ratio)| (n == name).then_some(ratio))
            .unwrap_or(DEFAULT_RATIO);
        TREE_BUDGET_BYTES / ratio
    }
}
