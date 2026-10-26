use super::apply;

/// `(matched, replacement, result)`, each worked through VS Code's
/// `buildReplaceStringWithCasePreserved` (`src/vs/base/common/search.ts`).
const RULES: &[(&str, &str, &str)] = &[
    // The spec's table.
    ("Foo", "bar", "Bar"),
    ("FOO", "bar", "BAR"),
    ("foo", "BAR", "bar"),
    ("foo_bar", "baz_qux", "baz_qux"),
    ("Foo_Bar", "baz_qux", "Baz_Qux"),
    ("fOo", "bar", "bar"),
    // First char upper, the rest mixed: only the first char changes.
    ("FoO", "bar", "Bar"),
    ("Foo", "bAR", "BAR"),
    ("fOO", "Bar", "bar"),
    // Hyphens as underscores; each part by the whole rules.
    ("Foo-BAR", "baz-qux", "Baz-QUX"),
    ("FOO_bar_Baz", "one_two_three", "ONE_two_Three"),
    // Parts that don't line up: the whole match's rules.
    ("Foo_Bar", "bazqux", "Bazqux"),
    ("FOO_BAR", "baz_qux_quux", "BAZ_QUX_QUUX"),
    ("Foo_Bar", "baz-qux", "Baz-qux"),
    // Both separators in both: neither splits.
    ("Foo-Bar_Baz", "one-two_three", "One-two_three"),
    // An empty part is left as written.
    ("Foo__Bar", "baz__qux", "Baz__Qux"),
    ("__Foo", "__bar", "__Bar"),
    // No letter in the match reads as all upper case, as in VS Code.
    ("123", "abc", "ABC"),
    // A first char with no case: as written.
    ("1Foo", "bar", "bar"),
    // Empty either side.
    ("", "Bar", "Bar"),
    ("Foo", "", ""),
    // Beyond ASCII.
    ("Éa", "über", "Über"),
    ("ÉCOLE", "straße", "STRASSE"),
];

#[test]
fn rules() {
    for &(matched, replacement, want) in RULES {
        assert_eq!(
            apply(matched, replacement),
            want,
            "apply({matched:?}, {replacement:?})"
        );
    }
}
