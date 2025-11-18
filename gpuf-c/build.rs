use std::env;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    
    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_language(cbindgen::Language::C)
        .with_pragma_once(true)
        .with_include_guard("GPUF_C_H")
        .with_documentation(true)
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file("gpuf_c.h");
    
    println!("cargo:rerun-if-changed=src/lib.rs");
}
