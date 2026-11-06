use super::*;

fn scan_c(text: &str, size: usize) -> Vec<String> {
    let mut scanner = Scanner::new(0, Dialect::C);
    let mut out = Vec::new();
    for chunk in text.as_bytes().chunks(size) {
        scanner.feed(chunk, &mut out);
    }
    out.iter()
        .map(|s| text[s.range.clone()].to_string())
        .collect()
}

fn whole(text: &str) -> Vec<String> {
    scan_c(text, text.len().max(1))
}

#[test]
fn chunking_never_changes_the_c_spans() {
    let text = "a %s %5.2f %-8s %ld %llu %zu %#x %*d %1$d %hhd %%x %1$*2$.*3$d % %5 %y %'d %.f %%%";
    let all = whole(text);
    assert!(all.contains(&"%1$d".to_string()), "{all:?}");
    assert!(all.contains(&"%hhd".to_string()), "{all:?}");
    for size in 1..=text.len() {
        assert_eq!(scan_c(text, size), all, "size {size}");
    }
}

#[test]
fn every_c_form_is_one_directive() {
    for d in [
        "%s", "%5.2f", "%-8s", "%ld", "%llu", "%zu", "%#x", "%*d", "%1$d", "%hhd", "%05d", "%.*f",
        "%*.*d", "%1$*2$d", "%.3s",
    ] {
        assert_eq!(whole(&format!("a {d} b")), vec![d.to_string()], "{d}");
    }
    assert_eq!(whole("%%"), vec!["%%".to_string()]);
}

#[test]
fn invalid_c_directives_paint_nothing() {
    for t in ["% ", "%5", "%y", "%", "a %", "%*1d"] {
        assert!(whole(t).is_empty(), "{t}");
    }
    assert_eq!(whole("%y%d"), vec!["%d".to_string()]);
}
