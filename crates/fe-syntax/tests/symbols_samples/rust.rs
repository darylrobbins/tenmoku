use std::fmt;

pub struct Point {
    pub x: i32,
    y: i32,
}

pub struct Wrapper(u8, String);

pub struct Unit;

union Bits {
    int: u32,
    float: f32,
}

#[derive(Debug)]
pub enum Shape {
    Empty,
    Circle(f64),
    Rect { width: f64, height: f64 },
    Coded = 3,
}

pub const ORIGIN: Point = Point { x: 0, y: 0 };

static mut COUNTER: u32 = 0;

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

mod inner {
    pub struct Nested {
        pub depth: usize,
    }
}

fn main() {
    let local = Point { x: 1, y: 2 };
    let _ = (local, Wrapper(0, String::new()), Unit, Shape::Empty);
}
