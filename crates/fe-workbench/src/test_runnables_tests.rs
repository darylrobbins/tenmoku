//! The tests a file has, found in the tree its parse already has.

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

use crate::test_runnables::{Runnable, RunnableKind, runnable_at, runnables};

/// The runnables of `text` parsed in full as `language`.
fn found(language: &str, text: &str) -> Vec<Runnable> {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run());
    runnables(language, &syntax, buffer.text())
}

/// Each runnable as (kind, selector, rows).
fn shown(runnables: &[Runnable]) -> Vec<(RunnableKind, &str, std::ops::Range<u32>)> {
    (runnables.iter())
        .map(|runnable| {
            (
                runnable.kind,
                runnable.selector.as_str(),
                runnable.rows.clone(),
            )
        })
        .collect()
}

use RunnableKind::{Module, Test};

#[test]
fn rust_test_attributes_and_cfg_test_mods() {
    let text = "\
fn adds() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {}

    /// Says why.
    #[test]
    #[should_panic]
    fn panics() {}

    #[cfg(test)]
    fn helper() {}

    mod nested {
        #[test]
        fn deep() {}
    }
}

#[cfg(all(test, unix))]
mod unix_tests;

#[cfg(all(test, unix))]
mod on_unix {}

mod not_tests {}
";
    let found = found("rust", text);
    assert_eq!(
        shown(&found),
        [
            (Module, "tests", 2..22),
            (Test, "tests::adds", 6..8),
            (Test, "tests::panics", 10..13),
            (Test, "tests::nested::deep", 18..20),
            (Module, "unix_tests", 23..25),
            (Module, "on_unix", 26..28),
        ]
    );
    assert_eq!(found[1].name, "adds");
    assert!(found[1].exact);
    assert_eq!(found[3].name, "deep");
}

#[test]
fn rust_async_test_and_rstest() {
    let text = "\
#[tokio::test]
async fn fetches() {}

#[rstest]
#[case(1)]
#[case(2)]
fn doubles(#[case] n: u32) {}

#[test_case(1 ; \"one\")]
fn cases(n: u32) {}

#[gpui::test]
fn draws(cx: &mut TestAppContext) {}

#[tokio::main]
async fn main() {}
";
    let found = found("rust", text);
    assert_eq!(
        shown(&found),
        [
            (Test, "fetches", 0..2),
            (Test, "doubles", 3..7),
            (Test, "cases", 8..10),
            (Test, "draws", 11..13),
        ]
    );
    // A test of cases is a module of them, run by what's under it.
    let exact: Vec<bool> = found.iter().map(|runnable| runnable.exact).collect();
    assert_eq!(exact, [true, false, false, true]);
}

#[test]
fn go_test_functions_need_the_testing_t_parameter() {
    let text = "\
package adder

import \"testing\"

func TestAdds(t *testing.T) {
}

func TestOnes(t *testing.T) { t.Run(\"one\", func(t *testing.T) {}) }

func Test(t *testing.T) {}

func Testing(t *testing.T) {}

func TestNoParam() {}

func TestMain(m *testing.M) {}

func BenchmarkAdds(b *testing.B) {}

func helper(t *testing.T) {}

func (s *Suite) TestMethod(t *testing.T) {}
";
    let found = found("go", text);
    assert_eq!(
        shown(&found),
        [
            (Test, "TestAdds", 4..6),
            (Test, "TestOnes", 7..8),
            (Test, "Test", 9..10),
        ]
    );
    assert_eq!(found[0].name, "TestAdds");
}

#[test]
fn python_methods_in_test_classes() {
    let text = "\
import pytest

def test_adds():
    assert 1 + 1 == 2

@pytest.mark.parametrize(\"n\", [1, 2])
def test_doubles(n):
    pass

class TestMaths:
    def setup_method(self):
        pass

    def test_one(self):
        pass

    @pytest.mark.slow
    async def test_two(self):
        pass

class Helper:
    def test_not_collected(self):
        pass

def helper():
    pass
";
    let found = found("python", text);
    assert_eq!(
        shown(&found),
        [
            (Test, "test_adds", 2..4),
            (Test, "test_doubles", 5..8),
            (Module, "TestMaths", 9..19),
            (Test, "TestMaths::test_one", 13..15),
            (Test, "TestMaths::test_two", 16..19),
        ]
    );
    assert_eq!(found[3].name, "test_one");
}

#[test]
fn no_runnables_in_plain_code() {
    let rust = "\
#[derive(Debug)]
struct Point;

#[inline]
fn add(a: u32, b: u32) -> u32 { a + b }

mod math {
    pub fn test_like() {}
}

impl Point {
    #[test]
    fn method() {}
}
";
    assert_eq!(found("rust", rust), []);
    let go = "package main\n\nfunc main() {}\n\nfunc TestLike() int { return 0 }\n";
    assert_eq!(found("go", go), []);
    let python = "def helper():\n    pass\n\nclass Thing:\n    def test_it(self):\n        pass\n";
    assert_eq!(found("python", python), []);
    // A language with no runner has none, tests or not.
    assert_eq!(found("javascript", "test('adds', () => {});\n"), []);
}

#[test]
fn innermost_runnable_at_a_row_is_chosen() {
    let text = "\
#[cfg(test)]
mod tests {
    #[test]
    fn one() {
        assert!(true);
    }

    fn helper() {}

    mod inner {
        #[test]
        fn two() {}
    }
}

fn outside() {}
";
    let found = found("rust", text);
    let at = |row| runnable_at(&found, row).map(|runnable| runnable.selector.as_str());
    // On the attribute, in the body, on the closing brace.
    assert_eq!(at(2), Some("tests::one"));
    assert_eq!(at(4), Some("tests::one"));
    assert_eq!(at(5), Some("tests::one"));
    // Between tests, the module.
    assert_eq!(at(7), Some("tests"));
    assert_eq!(at(0), Some("tests"));
    // A module that isn't a test module isn't run: its enclosing one is.
    assert_eq!(at(9), Some("tests"));
    assert_eq!(at(11), Some("tests::inner::two"));
    assert_eq!(at(15), None);
}

#[test]
fn rust_raw_identifiers_are_named_as_libtest_names_them() {
    let text = "\
#[cfg(test)]
mod r#move {
    #[test]
    fn r#match() {}
}
";
    let found = found("rust", text);
    assert_eq!(
        shown(&found),
        [(Module, "move", 0..5), (Test, "move::match", 2..4)]
    );
    assert_eq!(found[1].name, "match");
}

#[test]
fn a_test_module_in_a_file_of_its_own_runs_from_its_declaration() {
    let text = "\
pub fn add() {}

#[cfg(test)]
#[path = \"add_tests.rs\"]
mod tests;

mod plain;
";
    let found = found("rust", text);
    assert_eq!(shown(&found), [(Module, "tests", 2..5)]);
    let at = |row| runnable_at(&found, row).map(|runnable| runnable.selector.as_str());
    assert_eq!(at(4), Some("tests"));
    assert_eq!(at(6), None);
}
