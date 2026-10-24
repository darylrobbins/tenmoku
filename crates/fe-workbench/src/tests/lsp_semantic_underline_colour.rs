//! A `mutable` token underlines in its own highlight name's colour.

use super::*;

#[gpui::test]
fn mutable_underlines_by_the_foreground_name(cx: &mut TestAppContext) {
    let (mut s, editor) = underline::setup(cx);
    s.until("the tokens", |s| !painted(s, &editor).is_empty());
    let names: Vec<_> = editor.read_with(&s.cx, |view, _| {
        let model = view.model();
        let mut found: Vec<_> = (model
            .decorations(0..model.snapshot().text().len())
            .into_iter())
        .filter_map(|item| match item.kind {
            DecorationKind::SyntaxUnderline(name) => Some((item.range, name.to_string())),
            _ => None,
        })
        .collect();
        found.sort_by_key(|(range, _)| range.start);
        found
    });
    // `fn` is a mutable variable; `{}` a mutable label with no name.
    assert_eq!(
        names,
        vec![(0..2, "variable".to_owned()), (10..12, String::new())]
    );
}
