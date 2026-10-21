use crate::{USER_SETTINGS_TEMPLATE, themes};

#[test]
fn the_template_lists_every_bundled_theme() {
    let paragraph: String = USER_SETTINGS_TEMPLATE
        .lines()
        .skip_while(|line| !line.starts_with("# A bundled theme by name:"))
        .take_while(|line| line.starts_with("# ") && !line.starts_with("# name ="))
        .map(|line| line.trim_start_matches("# "))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(!paragraph.is_empty(), "the theme.name comment is missing");
    for theme in themes::bundled_themes() {
        assert!(
            paragraph.contains(theme.title),
            "the template's theme.name comment doesn't list {:?}",
            theme.title
        );
    }
}
