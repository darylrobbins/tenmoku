//! The fonts Tenmoku bundles, compiled in with `include_bytes!`: Instrument
//! Sans for the UI, Spline Sans Mono for code and Instrument Serif for
//! titles (`docs/design/design-system.md`, "Three families"). All three are
//! OFL-1.1; each family's `OFL.txt` is beside its files in `fonts/`, and
//! `fonts/README.md` says where they come from.
//!
//! Upstream ships Instrument Sans and Spline Sans Mono as variable fonts;
//! GPUI's macOS text system registers an in-memory file as one face at its
//! default weight, so the files here are static instances at the weights we
//! use.
//!
//! This crate must never depend on GPUI: [`all`] gives the bytes, and the
//! app hands them to its text system.

use std::borrow::Cow;

/// The UI's family.
pub const UI: &str = "Instrument Sans";
/// The code family: the editor's, the terminal's.
pub const CODE: &str = "Spline Sans Mono";
/// The title family, for large display text.
pub const TITLE: &str = "Instrument Serif";

/// One bundled file.
#[derive(Clone, Copy, Debug)]
pub struct Face {
    /// Its path under `fonts/`.
    pub file: &'static str,
    /// Its family: [`UI`], [`CODE`] or [`TITLE`].
    pub family: &'static str,
    /// Its CSS weight: 400, 500 or 600.
    pub weight: u16,
    pub italic: bool,
    pub bytes: &'static [u8],
}

macro_rules! face {
    ($file:literal, $family:expr, $weight:literal, $italic:literal) => {
        Face {
            file: $file,
            family: $family,
            weight: $weight,
            italic: $italic,
            bytes: include_bytes!(concat!("../fonts/", $file)),
        }
    };
}

/// Every bundled file.
pub const FACES: &[Face] = &[
    face!("instrument-sans/InstrumentSans-Regular.ttf", UI, 400, false),
    face!("instrument-sans/InstrumentSans-Medium.ttf", UI, 500, false),
    face!(
        "instrument-sans/InstrumentSans-SemiBold.ttf",
        UI,
        600,
        false
    ),
    face!(
        "spline-sans-mono/SplineSansMono-Regular.ttf",
        CODE,
        400,
        false
    ),
    face!(
        "spline-sans-mono/SplineSansMono-Medium.ttf",
        CODE,
        500,
        false
    ),
    face!(
        "spline-sans-mono/SplineSansMono-SemiBold.ttf",
        CODE,
        600,
        false
    ),
    face!(
        "spline-sans-mono/SplineSansMono-Italic.ttf",
        CODE,
        400,
        true
    ),
    face!(
        "instrument-serif/InstrumentSerif-Regular.ttf",
        TITLE,
        400,
        false
    ),
];

/// The bytes of every bundled file, for GPUI's `TextSystem::add_fonts`.
pub fn all() -> Vec<Cow<'static, [u8]>> {
    FACES.iter().map(|face| Cow::Borrowed(face.bytes)).collect()
}
