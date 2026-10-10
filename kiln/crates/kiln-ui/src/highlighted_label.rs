// SPDX-License-Identifier: LicenseRef-Proprietary

use std::ops::Range;

use gpui::{
    App, FontWeight, HighlightStyle, Hsla, IntoElement, ParentElement as _, RenderOnce,
    SharedString, Styled as _, StyledText, Window, div,
};

use crate::UiTheme;

/// A label with some of its text in an accent colour and bold, such as the
/// characters a fuzzy query matched.
#[derive(IntoElement)]
pub struct HighlightedLabel {
    pub text: SharedString,
    /// Byte ranges of `text`, on char boundaries.
    pub highlights: Vec<Range<usize>>,
    color: Hsla,
    accent: Hsla,
}

impl HighlightedLabel {
    pub fn new(
        text: impl Into<SharedString>,
        highlights: Vec<Range<usize>>,
        theme: &UiTheme,
    ) -> Self {
        Self {
            text: text.into(),
            highlights,
            color: theme.text,
            accent: theme.tab_active_text,
        }
    }
}

impl RenderOnce for HighlightedLabel {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let style = HighlightStyle {
            color: Some(self.accent),
            font_weight: Some(FontWeight::BOLD),
            ..HighlightStyle::default()
        };
        let highlights = self.highlights.into_iter().map(|range| (range, style));
        div()
            .text_color(self.color)
            .child(StyledText::new(self.text).with_highlights(highlights))
    }
}

/// The byte ranges of `text`'s chars at `char_indices`, which ascend:
/// adjacent chars merge into one range. Indices past the end are ignored.
pub fn char_ranges(text: &str, char_indices: &[usize]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut wanted = char_indices.iter().copied().peekable();
    for (ix, (start, ch)) in text.char_indices().enumerate() {
        while wanted.next_if(|&want| want < ix).is_some() {}
        if wanted.next_if_eq(&ix).is_none() {
            continue;
        }
        let end = start + ch.len_utf8();
        match ranges.last_mut() {
            Some(last) if last.end == start => last.end = end,
            _ => ranges.push(start..end),
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::char_ranges;

    #[test]
    fn char_indices_become_merged_byte_ranges() {
        // `ï` and `é` take two bytes each.
        let text = "naïve/café.rs";
        assert_eq!(char_ranges(text, &[0, 1, 2, 6, 9]), [0..4, 7..8, 10..12]);
        assert_eq!(char_ranges(text, &[6, 12, 40]), [7..8, 14..15]);
        assert!(char_ranges(text, &[]).is_empty());
    }
}
