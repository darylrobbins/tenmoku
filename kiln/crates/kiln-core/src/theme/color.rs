// SPDX-License-Identifier: LicenseRef-Proprietary

//! A theme's colours: sRGB with alpha, as a theme file writes them.

use std::{fmt, str::FromStr};

/// An sRGB colour with alpha, packed `0xrrggbbaa` as GPUI's `rgba` takes
/// it. Theme files write it `"#rrggbb"` or `"#rrggbbaa"`, and `"#rgb"` for
/// short; leaving alpha out makes it opaque.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Color(pub u32);

impl Color {
    pub const TRANSPARENT: Color = Color(0);

    /// The colour `0xrrggbbaa`.
    pub const fn rgba(rgba: u32) -> Self {
        Self(rgba)
    }

    /// The opaque colour `0xrrggbb`.
    pub const fn rgb(rgb: u32) -> Self {
        Self((rgb << 8) | 0xff)
    }

    pub const fn r(self) -> u8 {
        (self.0 >> 24) as u8
    }

    pub const fn g(self) -> u8 {
        (self.0 >> 16) as u8
    }

    pub const fn b(self) -> u8 {
        (self.0 >> 8) as u8
    }

    pub const fn a(self) -> u8 {
        self.0 as u8
    }

    /// `0xrrggbbaa`.
    pub const fn to_rgba_u32(self) -> u32 {
        self.0
    }

    /// The same colour at `alpha`.
    pub const fn with_alpha(self, alpha: u8) -> Self {
        Self((self.0 & !0xff) | alpha as u32)
    }

    /// `self` moved `amount` of the way towards `other`, alpha included:
    /// 0 is `self`, 1 is `other`.
    pub fn mix(self, other: Color, amount: f32) -> Self {
        let amount = amount.clamp(0., 1.);
        let channel = |shift: u32| {
            let from = f32::from((self.0 >> shift) as u8);
            let to = f32::from((other.0 >> shift) as u8);
            ((from + (to - from) * amount).round() as u32) << shift
        };
        Self(channel(24) | channel(16) | channel(8) | channel(0))
    }
}

impl FromStr for Color {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let invalid = || format!("{text:?} isn't a colour: expected \"#rrggbb\" or \"#rrggbbaa\"");
        let digits = text.strip_prefix('#').ok_or_else(invalid)?;
        if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let value = u32::from_str_radix(digits, 16).map_err(|_| invalid());
        match digits.len() {
            3 => {
                let short = value?;
                let expand = |nibble: u32| ((short >> nibble) & 0xf) * 0x11;
                Ok(Self::rgb((expand(8) << 16) | (expand(4) << 8) | expand(0)))
            }
            6 => Ok(Self::rgb(value?)),
            8 => Ok(Self::rgba(value?)),
            _ => Err(invalid()),
        }
    }
}

/// `#rrggbb`, or `#rrggbbaa` unless opaque.
impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.a() == 0xff {
            write!(f, "#{:06x}", self.0 >> 8)
        } else {
            write!(f, "#{:08x}", self.0)
        }
    }
}
