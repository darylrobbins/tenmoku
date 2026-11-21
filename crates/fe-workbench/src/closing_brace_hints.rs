//! Closing-brace hints: `} fn render` after the last row of a long block,
//! from the editor's outline, in every language that has one. They are
//! inlay hints of kind [`InlayHintKind::Closing`] (faint text, no pill),
//! merged with the language server's in `lsp_inlay_hints.rs`, where the
//! outline is found; nothing here is part of the buffer.

use fe_gpui::{InlayHint, InlayHintKind};
use fe_syntax::{Outline, OutlineKind};
use fe_text::Text;
use kiln_l10n::verbatim;

/// The keyword a hint for an item of `kind` starts with, if it gets one.
fn keyword(kind: OutlineKind) -> Option<&'static str> {
    Some(match kind {
        OutlineKind::Function => "fn",
        OutlineKind::Impl => "impl",
        OutlineKind::Trait => "trait",
        OutlineKind::Struct => "struct",
        OutlineKind::Enum => "enum",
        OutlineKind::Class => "class",
        OutlineKind::Module => "mod",
        OutlineKind::Interface => "interface",
        _ => return None,
    })
}

/// The row `offset` is on.
fn row_of(text: &Text, offset: usize) -> u32 {
    let (mut low, mut high) = (0, text.row_count());
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        if text.row_start(mid) <= offset {
            low = mid;
        } else {
            high = mid;
        }
    }
    low
}

/// The hints for the items of `outline` spanning at least `min_rows` rows
/// of `text`, each at the end of its last row, in order of offset.
pub(crate) fn hints(outline: &Outline, text: &Text, min_rows: u32) -> Vec<InlayHint> {
    let mut found: Vec<InlayHint> = Vec::new();
    for item in &outline.items {
        let Some(keyword) = keyword(item.kind) else {
            continue;
        };
        let end = item.range.end.min(text.len());
        if item.range.start >= end {
            continue;
        }
        // A node that takes its line break in ends on the row before it.
        let last = match text.char_at(end - 1) {
            Some('\n') if end - 1 > item.range.start => end - 1,
            _ => end,
        };
        let (first_row, last_row) = (row_of(text, item.range.start), row_of(text, last));
        if last_row - first_row + 1 < min_rows.max(2) {
            continue;
        }
        let offset = text.row_start(last_row) + text.row_len(last_row) as usize;
        found.push(InlayHint {
            offset,
            // An impl's outline name already starts with its keyword.
            label: if item.name.starts_with(keyword) && item.name[keyword.len()..].starts_with(' ')
            {
                item.name.clone()
            } else {
                [keyword, item.name.as_str()].join(verbatim!(" "))
            },
            kind: Some(InlayHintKind::Closing),
            padding_left: true,
            padding_right: false,
            data: None,
            tooltip: None,
            edits: Vec::new(),
        });
    }
    found.sort_by_key(|hint| hint.offset);
    found
}

/// `closing` less the hints `served` has at the same offset with the same
/// text: the server's wins.
pub(crate) fn without_served(closing: &[InlayHint], served: &[InlayHint]) -> Vec<InlayHint> {
    (closing.iter())
        .filter(|hint| {
            !served
                .iter()
                .any(|s| s.offset == hint.offset && s.label.trim() == hint.label.trim())
        })
        .cloned()
        .collect()
}
