//! Heading anchors, as VS Code's `githubSlugifier` makes them.

use std::collections::HashMap;

/// Numbers repeated slugs in document order, as GitHub does: the second
/// `hello-world` is `hello-world-1`.
#[derive(Default)]
pub(crate) struct Slugs {
    seen: HashMap<String, usize>,
}

impl Slugs {
    pub(crate) fn next(&mut self, heading: &str) -> String {
        let original = slugify(heading);
        let mut slug = original.clone();
        // As github-slugger does: `a-1` might be a heading's own slug
        // already, so count on to one that isn't.
        while self.seen.contains_key(&slug) {
            let count = self.seen.get_mut(&original).expect("seen");
            *count += 1;
            slug = format!("{original}-{count}");
        }
        self.seen.insert(slug.clone(), 0);
        slug
    }
}

/// The heading's text trimmed and lowercased, ASCII punctuation other than
/// `-` and `_` removed and each whitespace char made a `-`.
pub(crate) fn slugify(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_ascii_punctuation() || matches!(c, '-' | '_'))
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeats_are_numbered_past_existing_slugs() {
        let mut slugs = Slugs::default();
        assert_eq!(slugs.next("a"), "a");
        assert_eq!(slugs.next("a-1"), "a-1");
        assert_eq!(slugs.next("a"), "a-2");
        assert_eq!(slugs.next("A"), "a-3");
    }
}
