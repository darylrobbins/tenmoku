use gpui::{Font, FontFallbacks, Hsla, Pixels, SharedString, rgb};

/// How a terminal looks: its font, and the colours its cells resolve to.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalStyle {
    pub font_family: SharedString,
    /// Fonts to take a character from when `font_family` lacks it, in order.
    pub font_fallbacks: Vec<SharedString>,
    pub font_size: Pixels,
    /// A cell's height as a multiple of the font size, rounded to whole
    /// pixels. [`TerminalStyle::LINE_HEIGHT`] by default.
    pub line_height: f32,
    /// The default background, and the colour of `Color::Background`.
    pub background: Hsla,
    /// The default foreground, and the colour of `Color::Foreground`.
    pub foreground: Hsla,
    pub cursor: Hsla,
    pub selection: Hsla,
    /// ANSI colours 0–15: `Color::Named` and `Color::Indexed` 0–15.
    pub ansi: [Hsla; 16],
}

impl TerminalStyle {
    /// Tighter than the editor's 1.6, so box drawing and full-screen
    /// programs' rows meet.
    pub const LINE_HEIGHT: f32 = 1.25;

    /// The font cells are shaped in: `font_family`, with `font_fallbacks`
    /// when there are any.
    pub fn font(&self) -> Font {
        let mut font = gpui::font(self.font_family.clone());
        if !self.font_fallbacks.is_empty() {
            font.fallbacks = Some(FontFallbacks::from_fonts(
                self.font_fallbacks
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            ));
        }
        font
    }

    /// The 16 ANSI colours for a dark background: xterm's hues, with the
    /// normal blue lifted so it reads on black.
    pub fn ansi_dark() -> [Hsla; 16] {
        [
            0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x3b6fe8, 0xcd00cd, 0x00cdcd, 0xe5e5e5,
            0x7f7f7f, 0xff4040, 0x40ff40, 0xffff40, 0x7090ff, 0xff40ff, 0x40ffff, 0xffffff,
        ]
        .map(|hex| rgb(hex).into())
    }

    /// The 16 ANSI colours for a light background: xterm's hues, darkened so
    /// yellow, cyan and the whites read on white.
    pub fn ansi_light() -> [Hsla; 16] {
        [
            0x000000, 0xb00000, 0x007a00, 0x8a7000, 0x0000c8, 0xa000a0, 0x007a8a, 0x8a8a8a,
            0x5a5a5a, 0xd02020, 0x10a010, 0xa08a00, 0x3050e0, 0xc020c0, 0x0090a0, 0x606060,
        ]
        .map(|hex| rgb(hex).into())
    }

    /// [`TerminalStyle::ansi_light`] on a background lighter than 0.5,
    /// [`TerminalStyle::ansi_dark`] otherwise.
    pub fn ansi_for(background: Hsla) -> [Hsla; 16] {
        if background.l > 0.5 {
            Self::ansi_light()
        } else {
            Self::ansi_dark()
        }
    }
}
