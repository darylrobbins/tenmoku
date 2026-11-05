// SPDX-License-Identifier: Apache-2.0

use kiln_command::{ContextFrame, ContextPredicate, ContextStack};

const DEEP: usize = 100_000;

#[test]
fn deep_predicates_are_refused() {
    let nots = format!("{}a", "!".repeat(DEEP));
    let parens = format!("{}a{}", "(".repeat(DEEP), ")".repeat(DEEP));
    let ands = vec!["a"; DEEP].join(" && ");
    let ors = vec!["a"; DEEP].join(" || ");
    for source in [nots, parens, ands, ors] {
        let error = ContextPredicate::parse(&source).unwrap_err();
        assert_eq!(error.message, "predicate too complex");
    }
}

#[test]
fn predicates_up_to_the_limit_still_parse() {
    let stack = ContextStack::new().with(ContextFrame::new("a"));

    // 256 operators: 256 `&&` between 257 operands.
    let ands = vec!["a"; 257].join(" && ");
    assert!(ContextPredicate::parse(&ands).unwrap().eval(&stack));
    // 257 operators: the 257th `&&` starts at 2 + 5 * 256.
    let error = ContextPredicate::parse(&vec!["a"; 258].join(" && ")).unwrap_err();
    assert_eq!(error.message, "predicate too complex");
    assert_eq!(error.offset, 2 + 5 * 256);

    let nots = format!("{}a", "!".repeat(256));
    assert!(ContextPredicate::parse(&nots).unwrap().eval(&stack));
    let error = ContextPredicate::parse(&format!("{}a", "!".repeat(257))).unwrap_err();
    assert_eq!(error.message, "predicate too complex");
    assert_eq!(error.offset, 256);
}
