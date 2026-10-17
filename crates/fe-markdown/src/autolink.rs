//! GitHub's autolink literals, which pulldown-cmark doesn't find: bare
//! `http://` and `https://` URLs and `www.` hosts, which cmark-gfm finds in
//! a paragraph's source as it reads it, and e-mail addresses, which it finds
//! afterwards in the decoded text.

/// The longest domain a link may have, as DNS allows. Longer runs of domain
/// characters aren't links, which keeps each try at one short.
const MAX_DOMAIN: usize = 253;

/// A URL or `www.` literal at the start of `source`: its length in bytes
/// and what goes in front of its text to make its destination. `before` is
/// the content character before it, `None` at the start of a line.
pub(crate) fn url(source: &str, before: Option<char>) -> Option<(usize, &'static str)> {
    http(source, before)
        .map(|len| (len, ""))
        .or_else(|| www(source, before).map(|len| (len, "http://")))
}

/// An `http://` or `https://` URL, not straight after a letter, as
/// cmark-gfm reads the scheme back from its `:` over letters only.
fn http(source: &str, before: Option<char>) -> Option<usize> {
    if before.is_some_and(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let scheme = ["https://", "http://"].into_iter().find(|scheme| {
        source
            .get(..scheme.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    })?;
    // `http://localhost` is a link too: the host needs no period.
    linked_domain(source, scheme.len(), 1)
}

/// A `www.` host at the start of a line, after whitespace, or after `*`,
/// `_`, `~` or `(`.
fn www(source: &str, before: Option<char>) -> Option<usize> {
    let starts = before.is_none_or(|c| c.is_whitespace() || matches!(c, '*' | '_' | '~' | '('));
    if !starts || !source.starts_with("www.") {
        return None;
    }
    linked_domain(source, 0, 2)
}

/// The length of the link whose domain starts at `start`, if the domain is
/// valid: alphanumerics, `-`, `_` and `.`, in at least `segments` segments,
/// each with an alphanumeric, and no `_` in its last two. A final `.` or `_`
/// the link leaves out, as a sentence or emphasis ends it, doesn't count.
fn linked_domain(source: &str, start: usize, segments: usize) -> Option<usize> {
    let rest = &source[start..];
    let len = rest
        .char_indices()
        .take(MAX_DOMAIN + 1)
        .find(|(_, c)| !(c.is_alphanumeric() || matches!(c, '-' | '_' | '.')))
        .map_or(rest.len(), |(ix, _)| ix);
    if len > MAX_DOMAIN || !valid_host(rest[..len].trim_end_matches(['.', '_']), segments) {
        return None;
    }
    // The final `.`s and `_`s count if the link goes on past them.
    let domain_end = start + len;
    if extends_past(&source[domain_end..]) && !valid_host(&rest[..len], segments) {
        return None;
    }
    link_end(source, domain_end)
}

/// Whether a host's segments are all there, with an alphanumeric each,
/// there are at least `segments` of them, and the last two have no `_`. A
/// final `.`, as in `www.a.com./`, is a fully qualified name's.
fn valid_host(host: &str, segments: usize) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    let mut count = 0;
    for (ix, segment) in host.rsplit('.').enumerate() {
        let underscore = ix < 2 && segment.contains('_');
        if underscore || !segment.chars().any(char::is_alphanumeric) {
            return false;
        }
        count += 1;
    }
    count >= segments
}

/// Whether a link goes on into `rest`, the source after its domain: whether
/// anything up to the next whitespace or `<` survives [`link_end`]'s
/// trimming. Stops at the first character that does.
fn extends_past(rest: &str) -> bool {
    let mut chars = rest.char_indices().peekable();
    while let Some((ix, c)) = chars.next() {
        match c {
            c if c.is_whitespace() || c == '<' => return false,
            // With no `(` before it, every `)` is unmatched.
            '?' | '!' | '.' | ',' | ':' | '*' | '_' | '~' | '\'' | '"' | ';' | ')' => {}
            // An entity reference is trimmed with the `;` that ends it.
            '&' => {
                let name = rest[ix + 1..]
                    .find(|c: char| !c.is_ascii_alphanumeric())
                    .unwrap_or(rest.len() - ix - 1);
                if name == 0 || !rest[ix + 1 + name..].starts_with(';') {
                    return true;
                }
                while chars.next_if(|&(at, _)| at <= ix + 1 + name).is_some() {}
            }
            _ => return true,
        }
    }
    false
}

/// Where a link whose domain ends at `domain_end` ends: at whitespace or
/// `<`, less trailing punctuation, an entity reference such as `&amp;`, and
/// `)`s without a `(` to match in the link. `None` if nothing is left.
fn link_end(source: &str, domain_end: usize) -> Option<usize> {
    let mut end = source[domain_end..]
        .find(|c: char| c.is_whitespace() || c == '<')
        .map_or(source.len(), |len| domain_end + len);
    let opening = source[..end].matches('(').count();
    let mut closing = source[..end].matches(')').count();
    loop {
        let link = &source[..end];
        match link.chars().next_back()? {
            '?' | '!' | '.' | ',' | ':' | '*' | '_' | '~' | '\'' | '"' => end -= 1,
            ';' => {
                let name = link[..end - 1].trim_end_matches(|c: char| c.is_ascii_alphanumeric());
                end = match name.strip_suffix('&') {
                    Some(before) if name.len() < end - 1 => before.len(),
                    _ => end - 1,
                };
            }
            ')' if closing > opening => {
                end -= 1;
                closing -= 1;
            }
            _ => break,
        }
    }
    (end > 0).then_some(end)
}

/// An e-mail address at the start of `text`, decoded text, as cmark-gfm
/// finds one: a local part of alphanumerics and `.+-_` not straight after
/// `/`, then a host of alphanumerics, `-` and `_` with at least one inner
/// `.`, ending in a letter. Its length in bytes.
pub(crate) fn email(text: &str, before: Option<char>) -> Option<usize> {
    let local_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-' | '_');
    if before.is_some_and(|c| local_char(c) || c == '/') {
        return None;
    }
    let local = text.find(|c: char| !local_char(c)).unwrap_or(text.len());
    if local == 0 || !text[local..].starts_with('@') {
        return None;
    }
    let bytes = text.as_bytes();
    let mut end = local + 1;
    let mut periods = 0;
    while let Some(&b) = bytes.get(end) {
        match b {
            b if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' => {}
            b'.' if bytes.get(end + 1).is_some_and(u8::is_ascii_alphanumeric) => periods += 1,
            _ => break,
        }
        end += 1;
    }
    if periods == 0 || !bytes[end - 1].is_ascii_alphabetic() {
        return None;
    }
    Some(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(source: &str) -> Option<&str> {
        url(source, None).map(|(len, _)| &source[..len])
    }

    fn found_email(text: &str) -> Option<&str> {
        email(text, None).map(|len| &text[..len])
    }

    #[test]
    fn cmark_gfm_trims_entities_and_unmatched_parens() {
        assert_eq!(found("www.a.com/b&amp;"), Some("www.a.com/b"));
        assert_eq!(found("www.a.com/b&hl;"), Some("www.a.com/b"));
        assert_eq!(found("www.a.com/b;c;"), Some("www.a.com/b;c"));
        assert_eq!(found("www.a.com/b_(c))"), Some("www.a.com/b_(c)"));
        assert_eq!(found("https://a.b/c<d"), Some("https://a.b/c"));
    }

    #[test]
    fn a_long_run_of_unmatched_parens_is_trimmed_in_linear_time() {
        let source = format!("https://a.com/b{}", ")".repeat(200_000));
        let started = std::time::Instant::now();
        assert_eq!(found(&source), Some("https://a.com/b"));
        assert!(started.elapsed().as_secs() < 5, "{:?}", started.elapsed());
    }

    #[test]
    fn domains_need_real_segments_and_no_underscore_in_their_last_two() {
        assert_eq!(found("www.a_b.c.d"), Some("www.a_b.c.d"));
        assert_eq!(found("www.a.b_c.d"), None);
        // A sentence's period isn't a segment.
        assert_eq!(found("www.a.b_c.d."), None);
        assert_eq!(found("www.a.com."), Some("www.a.com"));
        assert_eq!(found("www.a.com_"), Some("www.a.com"));
        assert_eq!(found("www.a.com_!)&amp;"), Some("www.a.com"));
        assert_eq!(found("http://"), None);
        assert_eq!(found("https://."), None);
        assert_eq!(found("https://-.-"), None);
        assert_eq!(found("www."), None);
        assert_eq!(found("www..com"), None);
        assert_eq!(
            found("http://localhost:8080/x"),
            Some("http://localhost:8080/x")
        );
    }

    #[test]
    fn a_final_underscore_counts_when_the_link_goes_on() {
        for source in [
            "https://a.com_/x",
            "www.a.com_/x",
            "https://a.com_:8080",
            "www.a.com_?q=1",
            "www.a.com_&amp;x",
        ] {
            assert_eq!(found(source), None, "{source}");
        }
        // A fully qualified name's final `.` is fine.
        assert_eq!(found("www.a.com./x"), Some("www.a.com./x"));
    }

    #[test]
    fn domains_longer_than_dns_allows_are_not_links() {
        let long = format!("www.{}.com", "a".repeat(MAX_DOMAIN));
        assert_eq!(found(&long), None);
        let fits = format!("www.{}.com", "a".repeat(MAX_DOMAIN - 8));
        assert_eq!(found(&fits), Some(fits.as_str()));
    }

    #[test]
    fn email_addresses_end_with_a_letter() {
        assert_eq!(found_email("a.b+c@d.e."), Some("a.b+c@d.e"));
        assert_eq!(found_email("a@b.c-"), None);
        assert_eq!(found_email("a@b.c_"), None);
        assert_eq!(found_email("a@b.c1"), None);
        assert_eq!(found_email("a@b"), None);
        assert_eq!(found_email("@b.c"), None);
        assert_eq!(email("a@b.c", Some('/')), None);
    }

    #[test]
    fn urls_start_anywhere_but_after_a_letter() {
        assert_eq!(
            url("https://a.com", Some('"')).map(|(len, _)| len),
            Some(13)
        );
        assert_eq!(
            url("https://a.com", Some(':')).map(|(len, _)| len),
            Some(13)
        );
        assert_eq!(url("https://a.com", Some('x')), None);
        // `www.` hosts need a line start, whitespace or `*_~(`.
        assert_eq!(url("www.a.com", Some('"')), None);
        assert_eq!(url("www.a.com", Some('(')).map(|(len, _)| len), Some(9));
    }
}
