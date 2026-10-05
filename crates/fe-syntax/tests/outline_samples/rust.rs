use std::fmt::{self, Display};

/// Shapes and how to draw them.
mod shapes {
    pub struct Foo<T> {
        value: T,
    }

    pub enum Kind {
        Round,
        Square,
    }

    pub trait Draw {
        fn draw(&self) -> String;

        fn name(&self) -> &str {
            "shape"
        }
    }

    union Bits {
        int: u32,
        float: f32,
    }
}

pub struct Foo;

impl Display for Foo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "foo")
    }
}

impl<T: Clone> shapes::Draw
    for shapes::Foo<T>
{
    fn draw(&self) -> String {
        fn helper() -> String {
            String::new()
        }
        helper()
    }
}

impl Foo {
    #[inline]
    pub const fn new() -> Self {
        Foo
    }
}

type Pair = (u32, u32);

const LIMIT: usize = 10;

static GREETING: &str = "hello";

macro_rules! square {
    ($x:expr) => {
        $x * $x
    };
}

fn main() {
    println!("{}", square!(LIMIT));
}
