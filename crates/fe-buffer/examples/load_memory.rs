//! Measures resident memory for a 100 MB buffer (budget: at most 1.5× the file).
//! Run: `cargo run --release -p fe-buffer --example load_memory`

use fe_buffer::Buffer;

fn resident_bytes() -> usize {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<usize>()
        .unwrap_or(0)
        * 1024
}

fn main() {
    let line =
        "    let value = compute_something(argument_one, argument_two, &mut state)?; // é 😀\n";
    let source = line.repeat(100 * 1024 * 1024 / line.len());

    // Measured while the source string is still alive: once freed, the
    // allocator keeps its pages resident, which would inflate the number.
    let before = resident_bytes();
    let buffer = Buffer::new(&source);
    let held = resident_bytes() - before;
    println!(
        "file {:.1} MB, buffer {:.1} MB, {:.2}x (budget 1.50x)",
        source.len() as f64 / 1e6,
        held as f64 / 1e6,
        held as f64 / source.len() as f64
    );
    std::hint::black_box(&buffer);
}
