//! CSS colour literals, read without a language server: hex (`#rgb`,
//! `#rgba`, `#rrggbb`, `#rrggbbaa`), `rgb()`, `hsl()`, `hwb()`, `lab()`,
//! `lch()`, `oklab()`, `oklch()`, `color(srgb …)` and the named colours, as
//! CSS Color 4 has them, each to sRGB.
//!
//! A colour outside sRGB is clipped, each channel to `0..=1`: CSS's gamut
//! mapping, which lowers the chroma instead, is left out. A function with
//! anything in it that isn't a literal (`var()`, `calc()`, a relative colour,
//! a component missing or one too many, a unit where none goes) is no colour,
//! though a hex literal inside it is one of its own.

use std::ops::Range;

/// A colour in some text: its bytes, and it in sRGB.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FoundColor {
    pub(crate) range: Range<usize>,
    /// Red, green, blue and alpha, each in `0..=1`.
    pub(crate) rgba: [f32; 4],
}

/// `literal`, the whole of it a colour, in sRGB: red, green, blue and alpha,
/// each in `0..=1`, clipped there when it is outside sRGB.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "a colour picker's, later; the tests' now")
)]
pub(crate) fn parse_color(literal: &str) -> Option<[f32; 4]> {
    let bytes = literal.as_bytes();
    let (end, rgba) = match bytes.first()? {
        b'#' => hex_at(bytes, 0, &[3, 4, 6, 8])?,
        _ => {
            let name = ident_end(bytes, 0);
            match bytes.get(name) {
                Some(b'(') => function_at(bytes, 0, name)?,
                _ => (name, named(&literal[..name])?),
            }
        }
    };
    (end == bytes.len()).then_some(rgba)
}

/// Every colour in `text`, CSS: hex, functions and named colours, in order.
/// A name is a colour only as a word of its own: not part of a longer one, a
/// `--custom` name, a `$variable` or a function's name.
pub(crate) fn css_colors(text: &str) -> Vec<FoundColor> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == b'#' {
            if let Some((end, rgba)) = hex_at(bytes, at, &[3, 4, 6, 8]) {
                found.push(FoundColor {
                    range: at..end,
                    rgba,
                });
                at = end;
                continue;
            }
        } else if byte.is_ascii_alphabetic()
            && !at.checked_sub(1).is_some_and(|before| {
                is_ident(bytes[before]) || matches!(bytes[before], b'$' | b'@' | b'#' | b'.' | b'%')
            })
        {
            let name = ident_end(bytes, at);
            if bytes.get(name) == Some(&b'(') {
                if let Some((end, rgba)) = function_at(bytes, at, name) {
                    found.push(FoundColor {
                        range: at..end,
                        rgba,
                    });
                    at = end;
                } else {
                    // Inside, a hex literal is still one.
                    at = name + 1;
                }
                continue;
            }
            if let Some(rgba) = named(&text[at..name]) {
                found.push(FoundColor {
                    range: at..name,
                    rgba,
                });
            }
            at = name;
            continue;
        } else if is_ident(byte) {
            // The rest of a word that isn't a name's start.
            at = ident_end(bytes, at);
            continue;
        }
        at += 1;
    }
    found
}

/// Every `#rrggbb` and `#rrggbbaa` in `text`, in order.
pub(crate) fn hex_colors(text: &str) -> Vec<FoundColor> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(hash) = bytes[at..].iter().position(|&byte| byte == b'#') {
        let start = at + hash;
        match hex_at(bytes, start, &[6, 8]) {
            Some((end, rgba)) => {
                found.push(FoundColor {
                    range: start..end,
                    rgba,
                });
                at = end;
            }
            None => at = start + 1,
        }
    }
    found
}

/// Whether `byte` can be in an identifier: an ASCII letter or digit, `_`,
/// `-`, or any byte of a character past ASCII.
fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') || byte >= 0x80
}

/// Where the identifier at `at` ends.
fn ident_end(bytes: &[u8], at: usize) -> usize {
    (bytes[at..].iter())
        .position(|&byte| !is_ident(byte))
        .map_or(bytes.len(), |len| at + len)
}

/// The hex literal at `at`, the `#`, with one of `lengths` digits: where it
/// ends and its colour. It can't follow a letter, a digit, `_` or `&` (as
/// `&#123456;` does), nor be followed by one or by `-`.
fn hex_at(bytes: &[u8], at: usize, lengths: &[usize]) -> Option<(usize, [f32; 4])> {
    if at.checked_sub(1).is_some_and(|before| {
        bytes[before].is_ascii_alphanumeric() || matches!(bytes[before], b'_' | b'&')
    }) {
        return None;
    }
    let digits = &bytes[at + 1..];
    let len = (digits.iter())
        .position(|byte| !byte.is_ascii_hexdigit())
        .unwrap_or(digits.len());
    if !lengths.contains(&len)
        || (digits.get(len)).is_some_and(|&byte| is_ident(byte) && byte < 0x80)
    {
        return None;
    }
    let digit = |index: usize| f32::from((digits[index] as char).to_digit(16).unwrap_or(0) as u8);
    let channel = |index: usize| match len {
        3 | 4 => digit(index) * 17. / 255.,
        _ => (digit(2 * index) * 16. + digit(2 * index + 1)) / 255.,
    };
    let alpha = match len {
        4 | 8 => channel(3),
        _ => 1.,
    };
    Some((at + 1 + len, [channel(0), channel(1), channel(2), alpha]))
}

/// One token of a colour function's arguments.
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Percent(f64),
    Dimension(f64, String),
    /// `none`, a missing component: zero.
    None,
    Ident(String),
    Comma,
    Slash,
}

/// The colour function whose name is `at..name`, `bytes[name]` its `(`:
/// where it ends, after its `)`, and its colour.
fn function_at(bytes: &[u8], at: usize, name: usize) -> Option<(usize, [f32; 4])> {
    let function = std::str::from_utf8(&bytes[at..name])
        .ok()?
        .to_ascii_lowercase();
    if !matches!(
        function.as_str(),
        "rgb" | "rgba" | "hsl" | "hsla" | "hwb" | "lab" | "lch" | "oklab" | "oklch" | "color"
    ) {
        return None;
    }
    let (tokens, end) = arguments(bytes, name + 1)?;
    let args = Arguments::of(tokens)?;
    let rgba = match function.as_str() {
        "rgb" | "rgba" => rgb(&args)?,
        "hsl" | "hsla" => hsl(&args)?,
        "hwb" => hwb(&args)?,
        "lab" => lab(&args, false)?,
        "lch" => lab(&args, true)?,
        "oklab" => oklab(&args, false)?,
        "oklch" => oklab(&args, true)?,
        _ => color(&args)?,
    };
    Some((end, rgba))
}

/// The tokens from `at` to the function's `)`, and where that ends. None for
/// anything that isn't a token here, a function among them, or no `)`.
fn arguments(bytes: &[u8], mut at: usize) -> Option<(Vec<Token>, usize)> {
    let mut tokens = Vec::new();
    loop {
        while bytes.get(at).is_some_and(|byte| byte.is_ascii_whitespace()) {
            at += 1;
        }
        let byte = *bytes.get(at)?;
        let starts_number = |from: usize| match bytes.get(from) {
            Some(byte) if byte.is_ascii_digit() => true,
            Some(b'.') => bytes.get(from + 1).is_some_and(u8::is_ascii_digit),
            _ => false,
        };
        match byte {
            b')' => return Some((tokens, at + 1)),
            b',' => {
                tokens.push(Token::Comma);
                at += 1;
            }
            b'/' => {
                tokens.push(Token::Slash);
                at += 1;
            }
            _ if starts_number(at) || (matches!(byte, b'+' | b'-') && starts_number(at + 1)) => {
                let (value, end) = number(bytes, at)?;
                at = end;
                if bytes.get(at) == Some(&b'%') {
                    tokens.push(Token::Percent(value));
                    at += 1;
                } else if bytes
                    .get(at)
                    .is_some_and(|&byte| byte.is_ascii_alphabetic())
                {
                    let unit = ident_end(bytes, at);
                    let text = std::str::from_utf8(&bytes[at..unit]).ok()?;
                    tokens.push(Token::Dimension(value, text.to_ascii_lowercase()));
                    at = unit;
                } else {
                    tokens.push(Token::Number(value));
                }
            }
            _ if byte.is_ascii_alphabetic() || matches!(byte, b'-' | b'_') => {
                let end = ident_end(bytes, at);
                if bytes.get(end) == Some(&b'(') {
                    return None;
                }
                let text = std::str::from_utf8(&bytes[at..end])
                    .ok()?
                    .to_ascii_lowercase();
                tokens.push(match text.as_str() {
                    "none" => Token::None,
                    _ => Token::Ident(text),
                });
                at = end;
            }
            _ => return None,
        }
    }
}

/// The number at `at`, a sign, digits, a fraction and an exponent, and
/// where it ends.
fn number(bytes: &[u8], at: usize) -> Option<(f64, usize)> {
    let digits = |from: usize| {
        (bytes[from.min(bytes.len())..].iter())
            .position(|byte| !byte.is_ascii_digit())
            .map_or(bytes.len(), |len| from + len)
    };
    let mut end = at;
    if matches!(bytes.get(end), Some(b'+' | b'-')) {
        end += 1;
    }
    end = digits(end);
    if bytes.get(end) == Some(&b'.') && bytes.get(end + 1).is_some_and(u8::is_ascii_digit) {
        end = digits(end + 1);
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        if bytes.get(end + 1 + sign).is_some_and(u8::is_ascii_digit) {
            end = digits(end + 1 + sign);
        }
    }
    let value = std::str::from_utf8(&bytes[at..end]).ok()?.parse().ok()?;
    Some((value, end))
}

/// A function's arguments, laid out.
struct Arguments {
    /// The components, and in the comma syntax the alpha after them.
    values: Vec<Token>,
    /// The alpha after `/`.
    alpha: Option<Token>,
    /// Whether commas part them.
    legacy: bool,
}

impl Arguments {
    /// `tokens` as the comma syntax (`a, b, c[, alpha]`, no `none`) or the
    /// space one (`a b c [/ alpha]`); None for a mix or anything else.
    fn of(tokens: Vec<Token>) -> Option<Self> {
        if tokens.contains(&Token::Comma) {
            // A value, then a comma before each of the rest: not after the last.
            if tokens.len().is_multiple_of(2) {
                return None;
            }
            let mut values = Vec::new();
            for (index, token) in tokens.into_iter().enumerate() {
                match (index % 2, token) {
                    (1, Token::Comma) => {}
                    (0, Token::Comma | Token::Slash | Token::None) | (1, _) => return None,
                    (_, token) => values.push(token),
                }
            }
            return Some(Self {
                values,
                alpha: None,
                legacy: true,
            });
        }
        let mut parts = tokens.split(|token| *token == Token::Slash);
        let values = parts.next()?.to_vec();
        let alpha = match parts.next() {
            Some([alpha]) => Some(alpha.clone()),
            Some(_) => return None,
            None => None,
        };
        if parts.next().is_some() || values.is_empty() {
            return None;
        }
        Some(Self {
            values,
            alpha,
            legacy: false,
        })
    }

    /// The three components and the alpha, from `/` or, in the comma
    /// syntax, a fourth value.
    fn three(&self) -> Option<([&Token; 3], f64)> {
        let (values, alpha) = match (self.legacy, self.values.as_slice(), &self.alpha) {
            (true, [a, b, c], None) => ([a, b, c], 1.),
            (true, [a, b, c, alpha], None) => ([a, b, c], alpha_of(alpha)?),
            (false, [a, b, c], None) => ([a, b, c], 1.),
            (false, [a, b, c], Some(alpha)) => ([a, b, c], alpha_of(alpha)?),
            _ => return None,
        };
        Some((values, alpha))
    }
}

/// An alpha: a number or a percentage, clamped to `0..=1`.
fn alpha_of(token: &Token) -> Option<f64> {
    let alpha = match token {
        Token::Number(value) => *value,
        Token::Percent(value) => value / 100.,
        Token::None => 0.,
        _ => return None,
    };
    Some(alpha.clamp(0., 1.))
}

/// A number, or a percentage of which 100% is `full`; `none` is 0.
fn scaled(token: &Token, full: f64) -> Option<f64> {
    match token {
        Token::Number(value) => Some(*value),
        Token::Percent(value) => Some(value / 100. * full),
        Token::None => Some(0.),
        _ => None,
    }
}

/// A hue in degrees: a number, or an angle in `deg`, `grad`, `rad` or
/// `turn`; `none` is 0.
fn hue(token: &Token) -> Option<f64> {
    let degrees = match token {
        Token::Number(value) => *value,
        Token::None => 0.,
        Token::Dimension(value, unit) => match unit.as_str() {
            "deg" => *value,
            "grad" => value * 0.9,
            "rad" => value.to_degrees(),
            "turn" => value * 360.,
            _ => return None,
        },
        _ => return None,
    };
    Some(degrees.rem_euclid(360.))
}

/// The colour, its channels clipped to `0..=1`.
fn clipped([red, green, blue]: [f64; 3], alpha: f64) -> [f32; 4] {
    [red, green, blue, alpha].map(|channel| channel.clamp(0., 1.) as f32)
}

fn rgb(args: &Arguments) -> Option<[f32; 4]> {
    let (values, alpha) = args.three()?;
    // The comma syntax has all numbers or all percentages.
    let percents = values
        .iter()
        .filter(|it| matches!(it, Token::Percent(_)))
        .count();
    if args.legacy && percents != 0 && percents != 3 {
        return None;
    }
    let channel = |token: &Token| match token {
        Token::Number(value) => Some(value / 255.),
        _ => scaled(token, 1.),
    };
    let [red, green, blue] = values;
    Some(clipped(
        [channel(red)?, channel(green)?, channel(blue)?],
        alpha,
    ))
}

/// A percentage of saturation, lightness, whiteness or blackness, as a
/// fraction: the comma syntax has only percentages, the space one numbers
/// too.
fn fraction(token: &Token, legacy: bool) -> Option<f64> {
    match token {
        Token::Number(_) if legacy => None,
        _ => scaled(token, 100.).map(|value| value / 100.),
    }
}

/// HSL, the hue in degrees and the rest in `0..=1`, to sRGB, as CSS Color 4.
fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> [f64; 3] {
    let (saturation, lightness) = (saturation.clamp(0., 1.), lightness.clamp(0., 1.));
    let channel = |n: f64| {
        let k = (n + hue / 30.).rem_euclid(12.);
        let a = saturation * lightness.min(1. - lightness);
        lightness - a * (k - 3.).min(9. - k).clamp(-1., 1.)
    };
    [channel(0.), channel(8.), channel(4.)]
}

fn hsl(args: &Arguments) -> Option<[f32; 4]> {
    let ([h, s, l], alpha) = args.three()?;
    let rgb = hsl_to_rgb(
        hue(h)?,
        fraction(s, args.legacy)?,
        fraction(l, args.legacy)?,
    );
    Some(clipped(rgb, alpha))
}

fn hwb(args: &Arguments) -> Option<[f32; 4]> {
    if args.legacy {
        return None;
    }
    let ([h, w, b], alpha) = args.three()?;
    let (white, black) = (fraction(w, false)?.max(0.), fraction(b, false)?.max(0.));
    if white + black >= 1. {
        // Scaled to add up to 100%: a grey.
        let grey = white / (white + black);
        return Some(clipped([grey; 3], alpha));
    }
    let rgb = hsl_to_rgb(hue(h)?, 1., 0.5).map(|channel| channel * (1. - white - black) + white);
    Some(clipped(rgb, alpha))
}

/// `lab(L a b)`, or with `polar` `lch(L C H)`: CIE Lab in D50, to sRGB.
fn lab(args: &Arguments, polar: bool) -> Option<[f32; 4]> {
    if args.legacy {
        return None;
    }
    let ([l, x, y], alpha) = args.three()?;
    let lightness = scaled(l, 100.)?.clamp(0., 100.);
    let (a, b) = match polar {
        false => (scaled(x, 125.)?, scaled(y, 125.)?),
        true => {
            let (chroma, hue) = (scaled(x, 150.)?.max(0.), hue(y)?.to_radians());
            (chroma * hue.cos(), chroma * hue.sin())
        }
    };
    Some(clipped(lab_to_srgb(lightness, a, b), alpha))
}

/// `oklab(L a b)`, or with `polar` `oklch(L C H)`, to sRGB.
fn oklab(args: &Arguments, polar: bool) -> Option<[f32; 4]> {
    if args.legacy {
        return None;
    }
    let ([l, x, y], alpha) = args.three()?;
    let lightness = scaled(l, 1.)?.clamp(0., 1.);
    let (a, b) = match polar {
        false => (scaled(x, 0.4)?, scaled(y, 0.4)?),
        true => {
            let (chroma, hue) = (scaled(x, 0.4)?.max(0.), hue(y)?.to_radians());
            (chroma * hue.cos(), chroma * hue.sin())
        }
    };
    Some(clipped(oklab_to_srgb(lightness, a, b), alpha))
}

/// `color(srgb r g b)`: other spaces aren't colours here.
fn color(args: &Arguments) -> Option<[f32; 4]> {
    let [Token::Ident(space), r, g, b] = args.values.as_slice() else {
        return None;
    };
    if args.legacy || space != "srgb" {
        return None;
    }
    let alpha = match &args.alpha {
        Some(alpha) => alpha_of(alpha)?,
        None => 1.,
    };
    Some(clipped(
        [scaled(r, 1.)?, scaled(g, 1.)?, scaled(b, 1.)?],
        alpha,
    ))
}

/// `matrix` times `vector`.
fn times(matrix: &[[f64; 3]; 3], vector: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| row[0] * vector[0] + row[1] * vector[1] + row[2] * vector[2])
}

/// Linear light to sRGB's gamma, as CSS Color 4, sign and all.
fn gamma(linear: [f64; 3]) -> [f64; 3] {
    linear.map(|channel| {
        let magnitude = channel.abs();
        match magnitude <= 0.0031308 {
            true => channel * 12.92,
            false => channel.signum() * (1.055 * magnitude.powf(1. / 2.4) - 0.055),
        }
    })
}

/// CIE Lab in D50 to sRGB: through XYZ in D50, Bradford to D65, and linear
/// sRGB, with CSS Color 4's matrices.
fn lab_to_srgb(lightness: f64, a: f64, b: f64) -> [f64; 3] {
    const KAPPA: f64 = 24389. / 27.;
    const EPSILON: f64 = 216. / 24389.;
    const D50: [f64; 3] = [0.3457 / 0.3585, 1., (1. - 0.3457 - 0.3585) / 0.3585];
    const D50_TO_D65: [[f64; 3]; 3] = [
        [0.955473421488075, -0.02309845494876471, 0.06325924320057072],
        [
            -0.0283697093338637,
            1.0099953980813041,
            0.021041441191917323,
        ],
        [
            0.012314014864481998,
            -0.020507649298898964,
            1.330365926242124,
        ],
    ];
    let f1 = (lightness + 16.) / 116.;
    let f0 = a / 500. + f1;
    let f2 = f1 - b / 200.;
    let x = match f0.powi(3) > EPSILON {
        true => f0.powi(3),
        false => (116. * f0 - 16.) / KAPPA,
    };
    let y = match lightness > KAPPA * EPSILON {
        true => f1.powi(3),
        false => lightness / KAPPA,
    };
    let z = match f2.powi(3) > EPSILON {
        true => f2.powi(3),
        false => (116. * f2 - 16.) / KAPPA,
    };
    let xyz = [x * D50[0], y * D50[1], z * D50[2]];
    gamma(times(&XYZ_D65_TO_LINEAR_SRGB, times(&D50_TO_D65, xyz)))
}

/// XYZ in D65 to linear sRGB, as CSS Color 4.
const XYZ_D65_TO_LINEAR_SRGB: [[f64; 3]; 3] = [
    [12831. / 3959., -329. / 214., -1974. / 3959.],
    [-851781. / 878810., 1648619. / 878810., 36519. / 878810.],
    [705. / 12673., -2585. / 12673., 705. / 667.],
];

/// OKLab to sRGB: through Ottosson's LMS and linear sRGB.
fn oklab_to_srgb(lightness: f64, a: f64, b: f64) -> [f64; 3] {
    const TO_LMS: [[f64; 3]; 3] = [
        [1., 0.3963377773761749, 0.2158037573099136],
        [1., -0.1055613458156586, -0.0638541728258133],
        [1., -0.0894841775298119, -1.2914855480194092],
    ];
    const LMS_TO_LINEAR_SRGB: [[f64; 3]; 3] = [
        [4.0767416621, -3.3077115913, 0.2309699292],
        [-1.2684380046, 2.6097574011, -0.3413193965],
        [-0.0041960863, -0.7034186147, 1.7076147010],
    ];
    let lms = times(&TO_LMS, [lightness, a, b]).map(|channel| channel.powi(3));
    gamma(times(&LMS_TO_LINEAR_SRGB, lms))
}

/// The named colour `name`, in any case: CSS Color 4's, and `transparent`.
fn named(name: &str) -> Option<[f32; 4]> {
    if name.len() > 20 {
        return None;
    }
    let name = name.to_ascii_lowercase();
    if name == "transparent" {
        return Some([0.; 4]);
    }
    let index = NAMED
        .binary_search_by(|(known, _)| known.cmp(&name.as_str()))
        .ok()?;
    let rgb = NAMED[index].1;
    let channel = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.;
    Some([channel(16), channel(8), channel(0), 1.])
}

/// CSS Color 4's named colours, in order.
const NAMED: [(&str, u32); 148] = [
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

#[cfg(test)]
#[path = "color_literals_tests.rs"]
mod tests;
