use std::env;
use std::path::PathBuf;

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
    
    // Configure NVML library path for Windows
    #[cfg(target_os = "windows")]
    {
        // Common NVIDIA NVML library locations on Windows
        let possible_paths = vec![
            r"C:\Program Files\NVIDIA Corporation\NVSMI",
            r"C:\Windows\System32",
            r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.0\lib\x64",
            r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v11.8\lib\x64",
            r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v11.7\lib\x64",
        ];
        
        // Check if NVML_LIB_PATH environment variable is set
        if let Ok(nvml_path) = env::var("NVML_LIB_PATH") {
            println!("cargo:rustc-link-search=native={}", nvml_path);
        } else {
            // Try to find nvml.lib in common locations
            for path in possible_paths {
                let nvml_lib = PathBuf::from(path).join("nvml.lib");
                if nvml_lib.exists() {
                    println!("cargo:rustc-link-search=native={}", path);
                    println!("cargo:warning=Found nvml.lib at: {}", path);
                    break;
                }
            }
        }
    }
    
    println!("cargo:rerun-if-changed=src/lib.rs");
}
