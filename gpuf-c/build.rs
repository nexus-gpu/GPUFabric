use std::env;
use std::path::PathBuf;

fn main() {
    // Temporarily disable cbindgen to avoid syntax errors
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    
    // Configure cbindgen directly, without relying on external config files
    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_language(cbindgen::Language::C)
        .with_pragma_once(true)
        .with_include_guard("GPUF_C_H")
        .with_documentation(true)
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file("gpuf_c.h");
    
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
        
        // Get the absolute path to the llama library - now in target directory
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let manifest_path = PathBuf::from(&manifest_dir);
        let workspace_root = manifest_path.parent().unwrap(); // Go to GPUFabric/
        let llama_lib_dir = workspace_root.join("target").join("llama-android-ndk");
        
        // Check if llama-android-ndk directory exists
        if !llama_lib_dir.exists() {
            println!("cargo:warning=llama-android-ndk directory not found at: {}", llama_lib_dir.display());
            println!("cargo:warning=Please run generate_sdk.sh first to build the static libraries");
            panic!("llama-android-ndk directory not found");
        }
        
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
        
        // Link all static libraries with whole-archive - USING GROUP TO RESOLVE DEPENDENCIES
        let llama_lib_path = llama_lib_dir.join("libllama.a");
        let ggml_lib_path = llama_lib_dir.join("libggml.a");
        let ggml_base_lib_path = llama_lib_dir.join("libggml-base.a");
        let ggml_cpu_lib_path = llama_lib_dir.join("libggml-cpu.a");
        
        // Extract the critical backend registration object file and link it directly
        let ggml_backend_reg_obj = llama_lib_dir.join("ggml-backend-reg.cpp.o");
        
        // 🆕 Add multimodal libraries
        let mtmd_lib_path = llama_lib_dir.join("libmtmd.a");
        println!("cargo:warning=Looking for libmtmd.a at: {}", mtmd_lib_path.display());
        println!("cargo:warning=Directory exists: {}", llama_lib_dir.exists());
        println!("cargo:warning=File exists: {}", mtmd_lib_path.exists());
        
        // Check if multimodal libraries exist
        let has_multimodal = mtmd_lib_path.exists(); // Only check libmtmd.a (clip is included)
        if has_multimodal {
            println!("cargo:warning=Found libmtmd - enabling multimodal support");
        } else {
            println!("cargo:warning=libmtmd not found - multimodal support disabled");
            println!("cargo:warning=Expected at: {}", mtmd_lib_path.display());
        }
        if !ggml_backend_reg_obj.exists() {
            // Extract the object file if it doesn't exist
            println!("cargo:warning=Extracting ggml-backend-reg.cpp.o from libggml.a");
            
            // Check if libggml.a exists before trying to extract
            if !ggml_lib_path.exists() {
                println!("cargo:warning=libggml.a not found at: {}", ggml_lib_path.display());
                panic!("libggml.a not found - please run generate_sdk.sh first");
            }
            
            let output = std::process::Command::new("ar")
                .args(&["-x", &ggml_lib_path.to_string_lossy(), "ggml-backend-reg.cpp.o"])
                .current_dir(&llama_lib_dir)
                .output()
                .expect("Failed to execute ar command");
            
            if !output.status.success() {
                println!("cargo:warning=Failed to extract object file: {}", String::from_utf8_lossy(&output.stderr));
                panic!("Failed to extract ggml-backend-reg.cpp.o");
            }
        }
        
        // Create a new ggml.a without the backend registration object to avoid duplicates
        let ggml_lib_without_backend = llama_lib_dir.join("libggml-no-backend.a");
        let output = std::process::Command::new("ar")
            .args(&["-d", &ggml_lib_path.to_string_lossy(), "ggml-backend-reg.cpp.o"])
            .current_dir(&llama_lib_dir)
            .output()
            .expect("Failed to remove object from archive");
        
        if output.status.success() {
            // Copy the modified archive
            std::fs::copy(&ggml_lib_path, &ggml_lib_without_backend).expect("Failed to copy ggml library");
            
            // Restore the original archive
            let _output = std::process::Command::new("ar")
                .args(&["-r", &ggml_lib_path.to_string_lossy(), "ggml-backend-reg.cpp.o"])
                .current_dir(&llama_lib_dir)
                .output()
                .expect("Failed to restore object to archive");
        }
        
        // Link the critical object file directly FIRST
        if ggml_backend_reg_obj.exists() {
            println!("cargo:rustc-link-arg={}", ggml_backend_reg_obj.display());
        }
        
        // Use --start-group and --end-group to resolve circular dependencies
        // Use the modified ggml library without the backend registration object
        println!("cargo:rustc-link-arg=-Wl,--start-group");
        println!("cargo:rustc-link-arg=-Wl,--whole-archive,{},--no-whole-archive", llama_lib_path.display());
        if ggml_lib_without_backend.exists() {
            println!("cargo:rustc-link-arg=-Wl,--whole-archive,{},--no-whole-archive", ggml_lib_without_backend.display());
        } else {
            println!("cargo:rustc-link-arg=-Wl,--whole-archive,{},--no-whole-archive", ggml_lib_path.display());
        }
        println!("cargo:rustc-link-arg=-Wl,--whole-archive,{},--no-whole-archive", ggml_cpu_lib_path.display());
        println!("cargo:rustc-link-arg=-Wl,--whole-archive,{},--no-whole-archive", ggml_base_lib_path.display());
        println!("cargo:rustc-link-arg=-Wl,--end-group");
        
        // Force export of dynamic symbols
        println!("cargo:rustc-link-arg=-Wl,--export-dynamic");
        
        // Ensure all symbols from static libraries are available
        println!("cargo:rustc-link-arg=-Wl,--whole-archive");
        println!("cargo:rustc-link-arg=-Wl,--no-whole-archive");
        
        // Additional: Force symbol visibility
        println!("cargo:rustc-link-arg=-Wl,--retain-symbols-file=/dev/null");
        
        // Additional: Force export all symbols from static libraries
        println!("cargo:rustc-link-arg=-Wl,--whole-archive");
        println!("cargo:rustc-link-arg=-Wl,--no-whole-archive");
        
        // Ensure symbols are not stripped
        println!("cargo:rustc-link-arg=-Wl,--retain-symbols-file=/dev/null");
        
        println!("cargo:warning=Linked static llama.cpp Android library at: {}", llama_lib_dir.display());
    }
    
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");
}
