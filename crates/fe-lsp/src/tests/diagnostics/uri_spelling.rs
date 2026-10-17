//! A pushed publication is matched to an open document by decoded path, so a
//! server that escapes `[ ] ( )` still reaches it.

use fe_buffer::Buffer;
use fe_text::Text;
use lsp_types::Url;

use super::{marker, next, over, publish};
use crate::fake::Script;
use crate::tests::Started;

#[test]
fn publications_with_a_respelled_uri_reach_the_open_buffer() {
    let mut s = Started::new(Script::default());
    let text = "one two\n";
    let mut first = Buffer::new(text);
    let first_uri = s.open_synced(&mut first, "app/[slug]/page.txt");
    let mut second = Buffer::new(text);
    let second_uri = s.open_synced(&mut second, "app/(group)/other.txt");

    for (buffer, uri, plain, escaped) in [
        (&first, &first_uri, "[slug]", "%5Bslug%5D"),
        (&second, &second_uri, "(group)", "%28group%29"),
    ] {
        let respelled = Url::parse(&uri.as_str().replace(plain, escaped)).unwrap();
        assert_ne!(&respelled, uri, "{escaped} is a different spelling");
        publish(
            &s,
            &respelled,
            Some(0),
            vec![over(text, "two", false, "two")],
        );
        let published = next(&s.events);
        assert_eq!(published.buffer, buffer.id());
        assert_eq!(published.version, buffer.version());
        let [item] = &published.items[..] else {
            panic!("one item")
        };
        assert_eq!(Text::new(text).slice_to_string(item.range.clone()), "two");
    }

    // No such open document: dropped.
    let other = Url::parse(&first_uri.as_str().replace("[slug]", "%5Bother%5D")).unwrap();
    publish(&s, &other, Some(0), vec![over(text, "one", false, "stray")]);
    marker(&s, &first_uri, 0, text, "the unopened respelling");
}
