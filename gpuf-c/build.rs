use std::env;
use std::path::PathBuf;

fn main() {
    // Temporarily disable cbindgen to avoid syntax errors
    // let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    
    // Configure cbindgen directly, without relying on external config files
    // cbindgen::Builder::new()
    //     .with_crate(crate_dir)
    //     .with_language(cbindgen::Language::C)
    //     .with_pragma_once(true)
    //     .with_include_guard("GPUF_C_H")
    //     .with_documentation(true)
    //     .generate()
    //     .expect("Unable to generate bindings")
    //     .write_to_file("gpuf_c.h");
    
    // Get the target OS from Cargo environment variable
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    println!("cargo:warning=Target OS detected: {}", target_os);

    // Configure NVML library path for Windows target
    if target_os == "windows" {
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
            // Note: checking path existence works only if cross-compiling on Windows or if paths are mapped
            // For cross-compilation from Linux, this usually won't find anything, which is fine
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
    
    // Link OpenMP on Linux target explicitly (LLVM OpenMP)
    // This is required because llama.cpp is compiled with Clang and uses __kmpc_* symbols
    if target_os == "linux" {
        // 1. Check if LIBOMP_PATH environment variable is set
        if let Ok(libomp_path) = env::var("LIBOMP_PATH") {
            println!("cargo:rustc-link-search=native={}", libomp_path);
        } else {
            // 2. Check common LLVM library paths for libomp.so to avoid hardcoding specific versions
            let possible_llvm_paths = vec![
                "/usr/lib/llvm-19/lib",
                "/usr/lib/llvm-18/lib",
                "/usr/lib/llvm-17/lib",
                "/usr/lib/llvm-16/lib",
                "/usr/lib/llvm-15/lib",
                "/usr/lib/llvm-14/lib",
            ];

            for path in possible_llvm_paths {
                if std::path::Path::new(path).join("libomp.so").exists() {
                    println!("cargo:rustc-link-search=native={}", path);
                    break;
                }
            }
        }

        println!("cargo:rustc-link-lib=omp");
    }
    
    // For Android, link the static llama.cpp library
    if target_os == "android" {
        println!("cargo:warning=Linking static llama.cpp library for Android...");
        
        // Get the absolute path to the llama library
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let llama_lib_dir = PathBuf::from(&manifest_dir).join("llama-android-ndk");
        
        // Use absolute paths for Android NDK
        let ndk_root = env::var("ANDROID_NDK_ROOT").unwrap_or_else(|_| "/home/jack/android-ndk-r27d".to_string());
        let sysroot = format!("{}/toolchains/llvm/prebuilt/linux-x86_64/sysroot", ndk_root);
        let lib_path = format!("{}/usr/lib/aarch64-linux-android/28", sysroot);
        
        // Add Android system library paths first
        println!("cargo:rustc-link-search=native={}", lib_path);
        println!("cargo:rustc-link-search=native={}/usr/lib/aarch64-linux-android", sysroot);
        
        // Link system libraries first
        println!("cargo:rustc-link-lib=log");
        println!("cargo:rustc-link-lib=dl");
        println!("cargo:rustc-link-lib=m");
        println!("cargo:rustc-link-lib=c++_shared");
        
        // Link the static library with whole-archive - each as separate argument
        let llama_lib_path = llama_lib_dir.join("libllama.a");
        println!("cargo:rustc-link-arg=-Wl,--whole-archive,{},--no-whole-archive", llama_lib_path.display());
        
        // Force export of dynamic symbols
        println!("cargo:rustc-link-arg=-Wl,--export-dynamic");
        
        println!("cargo:warning=Linked static llama.cpp Android library at: {}", llama_lib_dir.display());
    }
    
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");
}
