#[inline(never)]
fn compute(a: i32, b: i32) -> i32 {
    let total = a + b;
    let doubled = total * 2;
    doubled
}

fn main() {
    let result = compute(1, 2);
    println!("{result}");
}
