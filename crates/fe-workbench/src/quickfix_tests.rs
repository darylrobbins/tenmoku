//! The quickfix model ([`crate::quickfix`]): moving through a list, the
//! stack of lists, and per-pane location lists, as nvim 0.12.4 does them.

use std::path::{Path, PathBuf};

use crate::quickfix::{
    QuickfixError, QuickfixItem, QuickfixKind, QuickfixList, QuickfixStack, Quickfixes, STACK_DEPTH,
};

fn item(path: &str, row: u32, text: &str) -> QuickfixItem {
    QuickfixItem {
        path: PathBuf::from(path),
        row,
        col: 0,
        text: text.to_owned(),
        kind: QuickfixKind::Error,
    }
}

/// Three items: two in `/p/a.rs`, then one in `/p/b.rs`.
fn three() -> QuickfixList {
    QuickfixList::new(
        "three",
        vec![
            item("/p/a.rs", 0, "one"),
            item("/p/a.rs", 4, "two"),
            item("/p/b.rs", 1, "three"),
        ],
    )
}

fn texts(found: Result<&QuickfixItem, QuickfixError>) -> Result<&str, QuickfixError> {
    found.map(|item| item.text.as_str())
}

#[test]
fn next_and_previous_do_not_wrap_and_report_e553_and_e42() {
    let mut list = three();
    assert_eq!(list.current, 0);
    assert_eq!(texts(list.next(1)), Ok("two"));
    assert_eq!(list.landed_message(), "(2 of 3) error: two");
    assert_eq!(texts(list.next(1)), Ok("three"));
    // Past the end: no wrap, the item stays.
    assert_eq!(texts(list.next(1)), Err(QuickfixError::NoMoreItems));
    assert_eq!(list.current, 2);
    assert_eq!(
        QuickfixError::NoMoreItems.to_string(),
        "E553: No more items"
    );
    // A count past the end stops at the last, as long as it moves.
    list.current = 0;
    assert_eq!(texts(list.next(9)), Ok("three"));
    assert_eq!(texts(list.previous(9)), Ok("one"));
    assert_eq!(texts(list.previous(1)), Err(QuickfixError::NoMoreItems));
    assert_eq!(list.current, 0);
    assert_eq!(texts(list.last()), Ok("three"));
    assert_eq!(texts(list.first()), Ok("one"));

    // `:cc N` above the count selects the last; `:cc` alone the current.
    assert_eq!(texts(list.go(Some(2))), Ok("two"));
    assert_eq!(texts(list.go(None)), Ok("two"));
    assert_eq!(texts(list.go(Some(99))), Ok("three"));
    assert_eq!(list.current, 2);
    assert_eq!(list.landed_message(), "(3 of 3) error: three");

    // An empty list, or none, is E42.
    let mut empty = QuickfixList::new("none", Vec::new());
    assert_eq!(texts(empty.next(1)), Err(QuickfixError::NoErrors));
    assert_eq!(texts(empty.previous(1)), Err(QuickfixError::NoErrors));
    assert_eq!(texts(empty.go(Some(1))), Err(QuickfixError::NoErrors));
    assert_eq!(texts(empty.first()), Err(QuickfixError::NoErrors));
    assert_eq!(texts(empty.last()), Err(QuickfixError::NoErrors));
    assert_eq!(texts(empty.next_file(1)), Err(QuickfixError::NoErrors));
    let mut stack = QuickfixStack::default();
    assert_eq!(stack.current_mut().err(), Some(QuickfixError::NoErrors));
    assert_eq!(QuickfixError::NoErrors.to_string(), "E42: No Errors");
}

#[test]
fn cnfile_goes_to_the_next_files_first_item_and_cnfile_back_to_the_last() {
    let mut list = QuickfixList::new(
        "files",
        vec![
            item("/p/a.rs", 0, "a1"),
            item("/p/a.rs", 1, "a2"),
            item("/p/b.rs", 0, "b1"),
            item("/p/b.rs", 1, "b2"),
            item("/p/c.rs", 0, "c1"),
        ],
    );
    assert_eq!(texts(list.next_file(1)), Ok("b1"));
    list.current = 1;
    assert_eq!(
        texts(list.next_file(1)),
        Ok("b1"),
        "from the file's second item"
    );
    assert_eq!(
        texts(list.next_file(9)),
        Ok("c1"),
        "a count stops at the last file"
    );
    assert_eq!(texts(list.next_file(1)), Err(QuickfixError::NoMoreItems));
    // `:cNfile` lands on the last item of the file before, as nvim's does.
    assert_eq!(texts(list.previous_file(1)), Ok("b2"));
    assert_eq!(texts(list.previous_file(1)), Ok("a2"));
    assert_eq!(
        texts(list.previous_file(1)),
        Err(QuickfixError::NoMoreItems)
    );
    list.current = 4;
    assert_eq!(texts(list.previous_file(2)), Ok("a2"));
}

#[test]
fn stack_keeps_ten_lists_and_colder_cnewer_walk_it() {
    let mut stack = QuickfixStack::default();
    assert_eq!(stack.older(1), Err(QuickfixError::AtBottom));
    assert_eq!(stack.newer(1), Err(QuickfixError::AtTop));
    for n in 0..=STACK_DEPTH {
        stack.push(QuickfixList::new(
            format!("list {n}"),
            vec![item("/a", n as u32, "x")],
        ));
    }
    // The eleventh push dropped the oldest.
    assert_eq!(stack.len(), STACK_DEPTH);
    assert_eq!(stack.current().unwrap().title, "list 10");
    assert_eq!(stack.summary(), "error list 10 of 10; 1 errors     list 10");
    assert_eq!(stack.newer(1), Err(QuickfixError::AtTop));
    assert_eq!(
        stack.older(3).as_deref(),
        Ok("error list 7 of 10; 1 errors      list 7")
    );
    // A count past the oldest stops there; then it is E380.
    assert!(stack.older(99).is_ok());
    assert_eq!(stack.current().unwrap().title, "list 1");
    assert_eq!(stack.older(1), Err(QuickfixError::AtBottom));
    assert_eq!(
        QuickfixError::AtBottom.to_string(),
        "E380: At bottom of quickfix stack"
    );
    assert!(stack.newer(2).is_ok());
    assert_eq!(stack.current().unwrap().title, "list 3");
    // A list pushed from an older one drops those newer than it.
    stack.push(QuickfixList::new("new", Vec::new()));
    assert_eq!(stack.len(), 4);
    assert_eq!(stack.current().unwrap().title, "new");
    assert_eq!(stack.newer(1), Err(QuickfixError::AtTop));
    assert_eq!(
        stack.older(1).unwrap(),
        "error list 3 of 4; 1 errors       list 3"
    );
    // Each list keeps its own selection.
    stack.current_mut().unwrap().current = 0;
    stack.newer(1).unwrap();
    assert_eq!(stack.current().unwrap().items.len(), 0);
}

#[test]
fn location_lists_belong_to_their_pane() {
    let mut lists = Quickfixes::<u64>::default();
    assert_eq!(
        lists.location(&1).err(),
        Some(QuickfixError::NoLocationList)
    );
    assert_eq!(
        QuickfixError::NoLocationList.to_string(),
        "E776: No location list"
    );
    lists.push_location(1, three());
    lists.push_location(2, QuickfixList::new("other", vec![item("/p/c.rs", 0, "c")]));
    lists
        .location(&1)
        .unwrap()
        .current_mut()
        .unwrap()
        .next(1)
        .unwrap();
    assert_eq!(lists.location(&1).unwrap().current().unwrap().current, 1);
    assert_eq!(lists.location(&2).unwrap().current().unwrap().current, 0);
    assert_eq!(
        lists.location(&2).unwrap().current().unwrap().title,
        "other"
    );
    // The quickfix stack is apart from both.
    assert!(lists.stack(false, &1).unwrap().current().is_none());
    assert_eq!(
        lists.stack(true, &1).unwrap().current().unwrap().title,
        "three"
    );

    lists.drop_pane(&1);
    assert!(!lists.has_location(&1));
    assert_eq!(
        lists.location(&1).err(),
        Some(QuickfixError::NoLocationList)
    );
    assert!(lists.has_location(&2));
}

#[test]
fn messages_show_one_line_and_clist_caps_its_items() {
    let mut list = QuickfixList::new(
        "kinds",
        vec![
            QuickfixItem {
                kind: QuickfixKind::Warning,
                col: 2,
                ..item("/p/src/a.rs", 3, "  two\n   more")
            },
            QuickfixItem {
                kind: QuickfixKind::Note,
                ..item("/elsewhere/b.rs", 0, "n")
            },
            QuickfixItem {
                kind: QuickfixKind::Info,
                ..item("/p/c.rs", 0, "i")
            },
        ],
    );
    assert_eq!(list.landed_message(), "(1 of 3) warning: two more");
    assert_eq!(
        list.listing(Some(Path::new("/p"))),
        "1 src/a.rs:4 col 3 warning: two more | 2 /elsewhere/b.rs:1 col 1 note: n \
         | 3 c.rs:1 col 1 info: i"
    );
    list.items = (0..15).map(|n| item("/p/a.rs", n, "x")).collect();
    let listing = list.listing(Some(Path::new("/p")));
    assert!(
        listing.starts_with("1 a.rs:1 col 1 error: x | 2 a.rs:2"),
        "{listing}"
    );
    assert!(
        listing.ends_with("10 a.rs:10 col 1 error: x | … 5 more"),
        "{listing}"
    );
}
