//! A publication under another URI scheme is dropped even when its decoded
//! path is an open file's.

use fe_buffer::Buffer;
use lsp_types::Url;

use super::{marker, over, publish};
use crate::fake::Script;
use crate::tests::Started;

#[test]
fn non_file_publications_for_an_open_files_path_are_dropped() {
    let mut s = Started::new(Script::default());
    let text = "one two\n";
    let mut buffer = Buffer::new(text);
    let uri = s.open_synced(&mut buffer, "app/[slug]/page.txt");
    let path = uri.path().replace("[slug]", "%5Bslug%5D");

    for scheme in ["custom", "untitled"] {
        let foreign = Url::parse(&format!("{scheme}://{path}")).unwrap();
        assert_ne!(foreign.scheme(), "file");
        publish(&s, &foreign, Some(0), vec![over(text, "two", false, "two")]);
    }
    marker(&s, &uri, 0, text, "the file's own URI");
}
