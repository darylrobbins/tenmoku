//! Compiles the two generated parsers, which `README.md` says how to regenerate.

fn main() {
    for dialect in ["csv", "tsv"] {
        let parser = format!("{dialect}/src/parser.c");
        cc::Build::new()
            .std("c11")
            .include(format!("{dialect}/src"))
            .file(&parser)
            .flag_if_supported("-Wno-unused-but-set-variable")
            .flag_if_supported("-Wno-unused-value")
            .flag_if_supported("-Wno-implicit-fallthrough")
            .compile(&format!("fe-grammar-{dialect}"));
        println!("cargo:rerun-if-changed={parser}");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
