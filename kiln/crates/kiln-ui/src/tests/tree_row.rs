// SPDX-License-Identifier: Apache-2.0

use super::ui_theme_compat::distinct;
use crate::{Theme, TreeRow, UiTheme};

/// A row's colours for its state are the flat theme's it was given before
/// it read tokens, under any flat theme an app sets.
#[test]
fn colors_follow_state() {
    for theme in [UiTheme::default_dark(), distinct(0.13), distinct(0.93)] {
        let tokens = &Theme::from_ui_theme(&theme).colors;
        let row = || TreeRow::new("row", "name", &theme);

        let focused = row().selected(true).focused(true).colors(tokens);
        assert_eq!(focused.background, Some(theme.row_selected));
        assert_eq!(focused.edge, Some(tokens.accent.base));
        let unfocused = row().selected(true).colors(tokens);
        assert_eq!(unfocused.background, Some(theme.row_selected_inactive));
        assert_eq!(unfocused.edge, None);
        for unselected in [row(), row().focused(true)] {
            let colors = unselected.colors(tokens);
            assert_eq!((colors.background, colors.edge), (None, None));
        }

        // Dim, not disabled: the chevron and icon keep their strength.
        let ignored = row().ignored(true).colors(tokens);
        assert_eq!(
            (ignored.label, ignored.icon),
            (theme.muted_text, theme.icon)
        );
        let ignored = row().directory(true).ignored(true).colors(tokens);
        assert_eq!(
            (ignored.label, ignored.icon, ignored.chevron),
            (theme.muted_text, theme.folder_icon, theme.chevron)
        );
        for error in [row().error(true), row().directory(false).error(true)] {
            let colors = error.colors(tokens);
            assert_eq!(colors.label, theme.error_text);
            assert_eq!(colors.icon, theme.error_text);
        }

        let dir = row().directory(false).colors(tokens);
        assert_eq!(
            (dir.label, dir.icon, dir.chevron),
            (theme.text, theme.folder_icon, theme.chevron)
        );
        let file = row().colors(tokens);
        assert_eq!((file.label, file.icon), (theme.text, theme.icon));
    }
}

/// Rows as a screen reader sees them: tree items with a level, expanded
/// or collapsed if a directory, selected or not, their place among their
/// siblings and their description.
mod a11y {
    use gpui::{
        Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Render,
        TestAppContext, VisualTestContext, Window, div,
    };

    use crate::{Node, Role, States, TreeRow, UiTheme, node, nodes};

    /// `src/` holding `main.rs` and `lib.rs`, then `README.md`; `src`
    /// expanded or not.
    struct Files {
        expanded: bool,
        selected: &'static str,
    }

    impl Render for Files {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let ui = UiTheme::default();
            let row = |id: usize, name: &'static str| {
                TreeRow::new(id, name, &ui).selected(name == self.selected)
            };
            let mut rows = vec![row(0, "src").directory(self.expanded).in_set(1, 2)];
            if self.expanded {
                rows.push(
                    row(1, "main.rs")
                        .depth(1)
                        .in_set(1, 2)
                        .a11y_description("modified"),
                );
                rows.push(row(2, "lib.rs").depth(1).in_set(2, 2));
            }
            rows.push(row(3, "README.md").in_set(2, 2));
            node(div().id("tree"), window, cx, Role::Tree, "Files").children(rows)
        }
    }

    fn files(cx: &mut TestAppContext) -> (Entity<Files>, &mut VisualTestContext) {
        let (files, cx) = cx.add_window_view(|_, _| Files {
            expanded: false,
            selected: "src",
        });
        cx.run_until_parked();
        (files, cx)
    }

    fn drawn(cx: &mut VisualTestContext) -> Vec<Node> {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            nodes(window, cx)
        })
    }

    fn rows(nodes: &[Node]) -> Vec<(String, States)> {
        nodes
            .iter()
            .filter(|n| n.role == Role::TreeItem)
            .map(|n| (n.label.to_string(), n.states.clone()))
            .collect()
    }

    fn states(
        level: usize,
        expanded: Option<bool>,
        selected: bool,
        (position, size): (usize, usize),
    ) -> States {
        States {
            selected: Some(selected),
            expanded,
            level: Some(level),
            position_in_set: Some(position),
            size_of_set: Some(size),
            ..States::default()
        }
    }

    #[gpui::test]
    fn rows_are_tree_items_with_level_and_expansion(cx: &mut TestAppContext) {
        let (_, cx) = files(cx);
        let got = drawn(cx);
        let tree = got.iter().find(|n| n.role == Role::Tree).expect("a tree");
        assert_eq!(tree.label.as_ref(), "Files");
        assert_eq!(
            rows(&got),
            [
                ("src".into(), states(1, Some(false), true, (1, 2))),
                ("README.md".into(), states(1, None, false, (2, 2))),
            ]
        );
    }

    #[gpui::test]
    fn expanding_a_folder_shows_its_children_a_level_down(cx: &mut TestAppContext) {
        let (files, cx) = files(cx);
        files.update(cx, |files, cx| {
            files.expanded = true;
            files.selected = "main.rs";
            cx.notify();
        });
        let modified = States {
            description: Some("modified".into()),
            ..states(2, None, true, (1, 2))
        };
        assert_eq!(
            rows(&drawn(cx)),
            [
                ("src".into(), states(1, Some(true), false, (1, 2))),
                ("main.rs".into(), modified),
                ("lib.rs".into(), states(2, None, false, (2, 2))),
                ("README.md".into(), states(1, None, false, (2, 2))),
            ]
        );

        files.update(cx, |files, cx| {
            files.expanded = false;
            files.selected = "src";
            cx.notify();
        });
        assert_eq!(
            rows(&drawn(cx))[0],
            ("src".into(), states(1, Some(false), true, (1, 2)))
        );
    }

    /// An empty description says nothing, rather than an empty string.
    #[gpui::test]
    fn an_empty_description_is_none(_: &mut TestAppContext) {
        let ui = UiTheme::default();
        let row = TreeRow::new("row", "name", &ui).a11y_description("");
        assert_eq!(row.a11y_states().description, None);
    }
}
