use fe_git::conflict_markers::parse;

#[test]
fn two_way_conflicts_split_at_their_markers() {
    let text = "a\n<<<<<<< HEAD\nx\ny\n=======\nz\n>>>>>>> feature\nmid\n<<<<<<< HEAD\nq\n=======\nr\n>>>>>>> feature\nend\n";
    let c = parse(text);
    assert_eq!(c.len(), 2);
    let first = &c[0];
    assert_eq!(first.start.line, 2..15);
    assert_eq!(first.start.label, "HEAD");
    assert_eq!(&text[first.current.clone()], "x\ny\n");
    assert_eq!(&text[first.splitter.clone()], "=======\n");
    assert_eq!(first.splitter, 19..27);
    assert_eq!(&text[first.incoming.clone()], "z\n");
    assert_eq!(first.incoming, 27..29);
    assert_eq!(first.end.label, "feature");
    assert_eq!(first.end.line, 29..45);
    assert_eq!(first.range(), 2..45);
    assert!(first.bases.is_empty());
    let second = &c[1];
    assert_eq!(&text[second.current.clone()], "q\n");
    assert_eq!(&text[second.incoming.clone()], "r\n");
    assert_eq!(second.start.label, "HEAD");
    assert_eq!(second.end.label, "feature");
    assert_eq!(
        &text[second.range()],
        "<<<<<<< HEAD\nq\n=======\nr\n>>>>>>> feature\n"
    );
}

#[test]
fn diff3_bases_are_kept() {
    let text = "<<<<<<< ours\nA\n||||||| parent of 1a2b3c4 (msg)\nB\n=======\nC\n>>>>>>> theirs\n";
    let c = parse(text);
    assert_eq!(c.len(), 1);
    assert_eq!(&text[c[0].current.clone()], "A\n");
    assert_eq!(c[0].bases.len(), 1);
    assert_eq!(c[0].bases[0].0.label, "parent of 1a2b3c4 (msg)");
    assert_eq!(&text[c[0].bases[0].1.clone()], "B\n");
    assert_eq!(&text[c[0].incoming.clone()], "C\n");

    let text = "<<<<<<< ours\nA\n||||||| one\nB\n||||||| two\nD\nE\n=======\nC\n>>>>>>> theirs\n";
    let c = parse(text);
    assert_eq!(c[0].bases.len(), 2);
    assert_eq!(c[0].bases[0].0.label, "one");
    assert_eq!(&text[c[0].bases[0].1.clone()], "B\n");
    assert_eq!(c[0].bases[1].0.label, "two");
    assert_eq!(&text[c[0].bases[1].1.clone()], "D\nE\n");
    assert_eq!(&text[c[0].current.clone()], "A\n");
}

#[test]
fn crlf_and_a_last_line_without_a_break() {
    let text = "<<<<<<< HEAD\r\nx\r\n=======\r\ny\r\n>>>>>>> feature";
    let c = parse(text);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].start.label, "HEAD");
    assert_eq!(c[0].end.label, "feature");
    assert_eq!(&text[c[0].current.clone()], "x\r\n");
    assert_eq!(&text[c[0].splitter.clone()], "=======\r\n");
    assert_eq!(&text[c[0].incoming.clone()], "y\r\n");
    assert_eq!(c[0].range(), 0..text.len());
}

#[test]
fn stray_markers_are_content() {
    let text = "Title\n=======\nbody\n>>>>>>> nope\n";
    assert!(parse(text).is_empty());

    let text = "<<<<<<< HEAD\nx\n";
    assert!(parse(text).is_empty());

    let text =
        "<<<<<<< a\n1\n=======\n2\n>>>>>>> b\n<<<<<<< c\n3\n<<<<<<< d\n4\n=======\n5\n>>>>>>> e\n";
    let c = parse(text);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].start.label, "a");
}

#[test]
fn an_empty_side_has_an_empty_range() {
    let text = "<<<<<<< HEAD\n=======\nz\n>>>>>>> f\n<<<<<<< HEAD\nx\n=======\n>>>>>>> f\n";
    let c = parse(text);
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].current, 13..13);
    assert_eq!(&text[c[0].incoming.clone()], "z\n");
    assert_eq!(&text[c[1].current.clone()], "x\n");
    assert!(c[1].incoming.is_empty());
}
