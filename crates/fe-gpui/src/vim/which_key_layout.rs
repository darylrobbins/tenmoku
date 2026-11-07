//! How the which-key popup ([`super::which_key::popup_layer`]) shows its
//! list, as which-key.nvim does: each key as a glyph (`<Space>` as `␣`) in
//! a column as wide as the widest, so every arrow starts at one x, and
//! colours that tell a group from an item. Display only: the keys listed and
//! matched keep vim's notation.

use fe_vim::{WhichKeyEntry, WhichKeyLabel};
use gpui::Hsla;

use crate::EditorTheme;

/// `key`, one key in vim notation, as the popup shows it: a named key as
/// its glyph (`<Space>` `␣`, `<Tab>` `⇥`, `<CR>` `↵`, `<Esc>` `⎋`, `<BS>`
/// `⌫`, `<Bar>` `|`, `<lt>` `<`, `<Del>` `⌦`, the arrows `↑ ↓ ← →`);
/// anything else, `<C-x>` among them, as written.
pub(crate) fn key_glyph(key: &str) -> String {
    let glyph = match key {
        "<Space>" => "␣",
        "<Tab>" => "⇥",
        "<CR>" => "↵",
        "<Esc>" => "⎋",
        "<BS>" => "⌫",
        "<Bar>" => "|",
        "<lt>" => "<",
        "<Del>" => "⌦",
        "<Up>" => "↑",
        "<Down>" => "↓",
        "<Left>" => "←",
        "<Right>" => "→",
        _ => key,
    };
    glyph.to_owned()
}

/// `keys`, a prefix of keys in vim notation (`<Space>f`, `,<C-w>`), each
/// key as [`key_glyph`] shows it.
pub(crate) fn keys_glyphs(keys: &str) -> String {
    let mut shown = String::new();
    let mut rest = keys;
    while let Some(ch) = rest.chars().next() {
        let named = (ch == '<').then(|| rest.find('>')).flatten();
        let len = named.map_or(ch.len_utf8(), |end| end + 1);
        shown.push_str(&key_glyph(&rest[..len]));
        rest = &rest[len..];
    }
    shown
}

/// How many `ch` (`ch_width` wide) the key column takes: enough for the
/// widest of `glyphs`, each `width` wide, and at least one.
pub(crate) fn key_column<'a>(
    glyphs: impl IntoIterator<Item = &'a str>,
    mut width: impl FnMut(&str) -> f32,
    ch_width: f32,
) -> f32 {
    let widest = (glyphs.into_iter()).map(&mut width).fold(0.0_f32, f32::max);
    if ch_width <= 0.0 {
        return 1.0;
    }
    // A hair over a whole `ch` is rounding, not another column.
    (widest / ch_width - 0.05).ceil().max(1.0)
}

/// The colours of one entry's key, arrow and title.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EntryColors {
    pub(crate) key: Hsla,
    pub(crate) arrow: Hsla,
    pub(crate) title: Hsla,
}

/// Whether `entry` is a group: more keys go on past it.
pub(crate) fn is_group(entry: &WhichKeyEntry) -> bool {
    entry.group || matches!(entry.label, WhichKeyLabel::Group { .. })
}

/// `entry`'s colours: its key in the warning colour, the arrow dim, and its
/// title in the text's colour for an item; for a group, in its icon's
/// ([`super::which_key_icons::entry_icon`]), so the two share one, or the
/// information colour without one.
pub(crate) fn entry_colors(entry: &WhichKeyEntry, theme: &EditorTheme) -> EntryColors {
    let group = || match super::which_key_icons::entry_icon(entry, theme) {
        Some((_, colour)) => colour,
        None => theme.information,
    };
    EntryColors {
        key: theme.warning,
        arrow: theme.gutter_foreground,
        title: match is_group(entry) {
            true => group(),
            false => theme.foreground,
        },
    }
}

/// The hint along the popup's last row.
pub(crate) const FOOTER: &str = "esc close";
