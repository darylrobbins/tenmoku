//! A Git Log commit's ref pills: a branch's in its commit's lane colour,
//! HEAD's filled, a remote's after a cloud, a tag's after a tag, and more
//! than three collapsed into `+N`.

use fe_git::{RefKind, RefLabel};
use fe_ui::Icon;
use gpui::TestAppContext;

use super::{GraphColors, MAX_PILLS, pill_style, visible_pills};

fn label(name: &str, kind: RefKind, is_head: bool) -> RefLabel {
    RefLabel {
        name: name.to_owned(),
        kind,
        is_head,
    }
}

#[gpui::test]
fn lane_colour(cx: &mut TestAppContext) {
    let graph = cx.update(|cx| GraphColors::new(cx));
    let branch = label("topic", RefKind::Branch, false);
    for lane in 0..graph.lanes.len() * 2 {
        let style = pill_style(&branch, lane, &graph);
        assert_eq!(style.color, graph.lane(lane), "lane {lane}");
        assert_eq!(style.color, graph.lanes[lane % graph.lanes.len()]);
    }
    assert_ne!(
        pill_style(&branch, 0, &graph).color,
        pill_style(&branch, 1, &graph).color
    );
}

#[gpui::test]
fn kinds(cx: &mut TestAppContext) {
    let graph = cx.update(|cx| GraphColors::new(cx));
    let style = |name, kind, head| pill_style(&label(name, kind, head), 1, &graph);
    let branch = style("main", RefKind::Branch, false);
    let remote = style("origin/main", RefKind::Remote, false);
    let tag = style("v1", RefKind::Tag, false);
    let head = style("main", RefKind::Branch, true);
    assert_eq!(branch.icon, None);
    assert_eq!(remote.icon, Some(Icon::Cloud));
    assert_eq!(tag.icon, Some(Icon::Tag));
    assert_ne!(remote.icon, tag.icon);
    // HEAD's is on the raised surface in ink; the others are tinted.
    assert_eq!(head.fill, graph.raised);
    assert_eq!(head.text, graph.ink);
    for tinted in [branch, remote, tag] {
        assert_ne!(tinted.fill, tinted.color);
        assert!(tinted.fill.a < 1.0);
        assert_eq!(tinted.text, tinted.color);
    }
    assert_eq!(style("HEAD", RefKind::Head, true).fill, graph.raised);
}

#[test]
fn overflow() {
    let refs: Vec<_> = (0..5)
        .map(|n| label(&format!("b{n}"), RefKind::Branch, false))
        .collect();
    let (shown, more) = visible_pills(&refs);
    assert_eq!(shown, &refs[..MAX_PILLS]);
    assert_eq!(MAX_PILLS, 3);
    assert_eq!(more, 2);
    let (shown, more) = visible_pills(&refs[..3]);
    assert_eq!((shown.len(), more), (3, 0));
    assert_eq!(visible_pills(&[]).1, 0);
}
