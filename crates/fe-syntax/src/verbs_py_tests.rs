use super::*;

fn scan_with(dialect: Dialect, text: &str, size: usize) -> Vec<String> {
    let mut scanner = Scanner::new(0, dialect);
    let mut out = Vec::new();
    for chunk in text.as_bytes().chunks(size) {
        scanner.feed(chunk, &mut out);
    }
    out.iter()
        .map(|s| text[s.range.clone()].to_string())
        .collect()
}

fn pct(text: &str) -> Vec<String> {
    scan_with(Dialect::PyPercent, text, text.len().max(1))
}

fn brace(text: &str) -> Vec<String> {
    scan_with(Dialect::Brace, text, text.len().max(1))
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn chunking_never_changes_the_python_spans() {
    let pct_text = "a %s %5.2f %(k)d %(a(b))-8.3ls %% %*.*d %lld %y %5 %(x %%%";
    let brace_text =
        "a {} {0} {name!r:>{width}} {{x}} {a.b[0]!s} {x:{y}} {1:{2{3}}} {bad name} {open";
    let (p, b) = (pct(pct_text), brace(brace_text));
    assert!(p.contains(&"%(a(b))-8.3ls".to_string()), "{p:?}");
    assert!(b.contains(&"{name!r:>{width}}".to_string()), "{b:?}");
    for size in 1..=pct_text.len() {
        assert_eq!(
            scan_with(Dialect::PyPercent, pct_text, size),
            p,
            "size {size}"
        );
    }
    for size in 1..=brace_text.len() {
        assert_eq!(
            scan_with(Dialect::Brace, brace_text, size),
            b,
            "size {size}"
        );
    }
    assert!(brace("{ab").is_empty() && brace("a {x:{y}").is_empty());
}

#[test]
fn percent_forms() {
    assert_eq!(
        pct("%s: %5.2f %(k)d %%"),
        strs(&["%s", "%5.2f", "%(k)d", "%%"])
    );
    assert!(pct("% %5 %y %(k").is_empty());
    assert_eq!(pct("%hd %Lf %-5s"), strs(&["%hd", "%Lf", "%-5s"]));
    assert!(pct("%hhd").is_empty());
}

#[test]
fn brace_forms() {
    assert_eq!(
        brace("{} {0} {name!r:>{width}} {a.b[1]}"),
        strs(&["{}", "{0}", "{name!r:>{width}}", "{a.b[1]}"])
    );
    assert_eq!(brace("{{ }}"), strs(&["{{", "}}"]));
    assert!(brace("{a b} } {!x}").is_empty());
}
