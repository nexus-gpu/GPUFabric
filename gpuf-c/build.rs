use std::env;
use std::path::PathBuf;

fn main() {
    // Get the target OS from Cargo environment variable
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    // println!("cargo:warning=Target OS detected: {}", target_os); // Commented out to reduce warning noise

    // Configure CUDA compilation flags for Position Independent Code
    // This is required for linking CUDA code into shared libraries
    if cfg!(feature = "cuda") {
        println!("cargo:rustc-env=CUDA_NVCC_FLAGS=-Xcompiler -fPIC");
        println!("cargo:rustc-env=CUDAFLAGS=-Xcompiler -fPIC");
        // println!("cargo:warning=CUDA feature enabled - adding -fPIC flag"); // Commented out to reduce warning noise
    }

    // Bundle CUDA runtime DLLs on Windows so gpuf-c.exe can run without requiring users to edit PATH.
    // This is best-effort: if DLLs are not found, we only emit warnings.
    if target_os == "windows" && cfg!(feature = "cuda") {
        println!("cargo:rerun-if-env-changed=CUDA_PATH");
        println!("cargo:rerun-if-env-changed=GPUF_BUNDLE_CUDA_DLLS");
        println!("cargo:rerun-if-env-changed=GPUF_BUNDLE_TAR");

        let bundle_enabled = env::var("GPUF_BUNDLE_CUDA_DLLS")
            .ok()
            .map(|v| v != "0" && v.to_lowercase() != "false")
            .unwrap_or(true);

        if bundle_enabled {
            if let Err(e) = bundle_cuda_dlls_windows() {
                println!("cargo:warning=CUDA DLL bundling failed: {}", e);
            }
        }
    }

    if target_os == "windows" && (cfg!(feature = "cuda") || cfg!(feature = "nvml")) {
        println!("cargo:rerun-if-env-changed=NVML_LIB_PATH");
        if let Err(e) = bundle_nvml_dll_windows() {
            println!("cargo:warning=NVML DLL bundling failed: {}", e);
        }
    }

    if target_os == "windows" && cfg!(feature = "cuda") {
        let bundle_tar_enabled = env::var("GPUF_BUNDLE_TAR")
            .ok()
            .map(|v| v != "0" && v.to_lowercase() != "false")
            .unwrap_or(true);

        if bundle_tar_enabled {
            if let Err(e) = bundle_windows_tar() {
                println!("cargo:warning=Tar bundling failed: {}", e);
            }
        }
    }

    // Configure NVML library path for Windows target
    if target_os == "windows" {
        // Common NVIDIA NVML library locations on Windows
        let possible_paths = vec![
            r"C:\Program Files\NVIDIA Corporation\NVSMI",
            r"C:\Windows\System32",
            r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.0\lib\x64",
            r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v11.8\lib\x64",
            r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v11.7\lib\x64",
             r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.0\lib\x64",
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

        let project_root = env::var("CARGO_MANIFEST_DIR").unwrap();
        let icon_path = format!("{}\\{}", project_root, "gpuf_icon.ico");
        let icon_rc_content = format!(
            r#"#include <windows.h>
1 ICON "{}"
"#,
            icon_path.replace("\\", "\\\\") // 转义反斜杠
        );

        std::fs::write("icon.rc", icon_rc_content).expect("Failed to write icon.rc file");

        embed_resource::compile("icon.rc", &[] as &[&str]);
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

        println!("cargo:warning=Linking static llama.cpp library for Android...");

        // Get the absolute path to the llama library - now in target directory
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let manifest_path = PathBuf::from(&manifest_dir);
        let workspace_root = manifest_path.parent().unwrap(); // Go to GPUFabric/
        let llama_lib_dir = workspace_root.join("target").join("llama-android-ndk");

        // Check if llama-android-ndk directory exists
        if !llama_lib_dir.exists() {
            println!(
                "cargo:warning=llama-android-ndk directory not found at: {}",
                llama_lib_dir.display()
            );
            println!(
                "cargo:warning=Please run generate_sdk.sh first to build the static libraries"
            );
            panic!("llama-android-ndk directory not found");
        }

        // Use absolute paths for Android NDK
        let ndk_root = env::var("ANDROID_NDK_ROOT")
            .unwrap_or_else(|_| "/home/jack/android-ndk-r27d".to_string());
        let sysroot = format!("{}/toolchains/llvm/prebuilt/linux-x86_64/sysroot", ndk_root);
        let lib_path = format!("{}/usr/lib/aarch64-linux-android/28", sysroot);

        // Add Android system library paths first
        println!("cargo:rustc-link-search=native={}", lib_path);
        println!(
            "cargo:rustc-link-search=native={}/usr/lib/aarch64-linux-android",
            sysroot
        );

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
        println!(
            "cargo:warning=Looking for libmtmd.a at: {}",
            mtmd_lib_path.display()
        );
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
                println!(
                    "cargo:warning=libggml.a not found at: {}",
                    ggml_lib_path.display()
                );
                panic!("libggml.a not found - please run generate_sdk.sh first");
            }

            let output = std::process::Command::new("ar")
                .args([
                    "-x",
                    &ggml_lib_path.to_string_lossy(),
                    "ggml-backend-reg.cpp.o",
                ])
                .current_dir(&llama_lib_dir)
                .output()
                .expect("Failed to execute ar command");

            if !output.status.success() {
                println!(
                    "cargo:warning=Failed to extract object file: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                panic!("Failed to extract ggml-backend-reg.cpp.o");
            }
        }

        // Create a new ggml.a without the backend registration object to avoid duplicates
        let ggml_lib_without_backend = llama_lib_dir.join("libggml-no-backend.a");
        let output = std::process::Command::new("ar")
            .args([
                "-d",
                &ggml_lib_path.to_string_lossy(),
                "ggml-backend-reg.cpp.o",
            ])
            .current_dir(&llama_lib_dir)
            .output()
            .expect("Failed to remove object from archive");

        if output.status.success() {
            // Copy the modified archive
            std::fs::copy(&ggml_lib_path, &ggml_lib_without_backend)
                .expect("Failed to copy ggml library");

            // Restore the original archive
            let _output = std::process::Command::new("ar")
                .args([
                    "-r",
                    &ggml_lib_path.to_string_lossy(),
                    "ggml-backend-reg.cpp.o",
                ])
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
        println!(
            "cargo:rustc-link-arg=-Wl,--whole-archive,{},--no-whole-archive",
            llama_lib_path.display()
        );
        if ggml_lib_without_backend.exists() {
            println!(
                "cargo:rustc-link-arg=-Wl,--whole-archive,{},--no-whole-archive",
                ggml_lib_without_backend.display()
            );
        } else {
            println!(
                "cargo:rustc-link-arg=-Wl,--whole-archive,{},--no-whole-archive",
                ggml_lib_path.display()
            );
        }
        println!(
            "cargo:rustc-link-arg=-Wl,--whole-archive,{},--no-whole-archive",
            ggml_cpu_lib_path.display()
        );
        println!(
            "cargo:rustc-link-arg=-Wl,--whole-archive,{},--no-whole-archive",
            ggml_base_lib_path.display()
        );
        println!("cargo:rustc-link-arg=-Wl,--end-group");

        // Force export of dynamic symbols
        println!("cargo:rustc-link-arg=-Wl,--export-dynamic");

        // Ensure all symbols from static libraries are available
        println!("cargo:rustc-link-arg=-Wl,--whole-archive");
        println!("cargo:rustc-link-arg=-Wl,--no-whole-archive");

        // Additional: Force symbol visibility
        println!("cargo:rustc-link-arg=-Wl,--retain-symbols-file=/dev/null");

        println!(
            "cargo:warning=Linked static llama.cpp Android library at: {}",
            llama_lib_dir.display()
        );
    }
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");
}

fn bundle_cuda_dlls_windows() -> Result<(), Box<dyn std::error::Error>> {
    use std::ffi::OsStr;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn is_truthy(name: &OsStr) -> bool {
        let s = name.to_string_lossy().to_ascii_lowercase();
        s.ends_with(".dll")
            && (s.starts_with("cublas64_")
                || s.starts_with("cublaslt64_")
                || s.starts_with("cudart64_")
                || s.starts_with("curand64_")
                || s.starts_with("cufft64_")
                || s.starts_with("cusolver64_")
                || s.starts_with("cusparse64_"))
    }

    // Figure out where cargo will place the final binary.
    // OUT_DIR looks like: <target>/<profile>/build/<crate>/out
    // or: <target>/<triple>/<profile>/build/<crate>/out
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let profile = env::var("PROFILE")?;

    let mut p: &Path = out_dir.as_path();
    let mut output_dir: Option<PathBuf> = None;
    while let Some(parent) = p.parent() {
        if p.file_name().and_then(|s| s.to_str()) == Some(profile.as_str()) {
            output_dir = Some(p.to_path_buf());
            break;
        }
        p = parent;
    }

    let output_dir = output_dir.ok_or("Failed to detect target output directory from OUT_DIR")?;

    // Candidate CUDA bin directories
    let mut cuda_bin_dirs: Vec<PathBuf> = Vec::new();
    if let Ok(cuda_path) = env::var("CUDA_PATH") {
        cuda_bin_dirs.push(PathBuf::from(cuda_path).join("bin\\x64"));
    }

    // Common install locations (best-effort)
    cuda_bin_dirs.push(PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.0\bin\x64"));
    cuda_bin_dirs.push(PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.6\bin\x64"));
    cuda_bin_dirs.push(PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.5\bin\x64"));
    cuda_bin_dirs.push(PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.4\bin\x64"));
    cuda_bin_dirs.push(PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.3\bin\x64"));
    cuda_bin_dirs.push(PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.2\bin\x64"));
    cuda_bin_dirs.push(PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.1\bin\x64"));
    cuda_bin_dirs.push(PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.0\bin\x64"));
    cuda_bin_dirs.push(PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v11.8\bin\x64"));

    // Find a bin dir that actually exists
    let cuda_bin = cuda_bin_dirs.into_iter().find(|p| p.exists());
    let Some(cuda_bin) = cuda_bin else {
        return Err("CUDA bin directory not found. Set CUDA_PATH or install CUDA Toolkit.".into());
    };

    // Copy all relevant DLLs from CUDA bin to output dir
    let mut copied = 0usize;
    for entry in fs::read_dir(&cuda_bin)? {
        let entry = entry?;
        let file_name = entry.file_name();
        if !is_truthy(&file_name) {
            continue;
        }
        let src = entry.path();
        let dst = output_dir.join(&file_name);
        // Always overwrite to keep incremental builds consistent.
        if let Err(e) = fs::copy(&src, &dst) {
            println!(
                "cargo:warning=Failed to copy CUDA DLL {} -> {}: {}",
                src.display(),
                dst.display(),
                e
            );
            continue;
        }
        copied += 1;
    }

    if copied == 0 {
        println!(
            "cargo:warning=No CUDA runtime DLLs were copied from {}. gpuf-c.exe may fail to run without PATH configured.",
            cuda_bin.display()
        );
    } else {
        println!(
            "cargo:warning=Bundled {} CUDA runtime DLL(s) into {}",
            copied,
            output_dir.display()
        );
    }

    Ok(())
}

fn bundle_windows_tar() -> Result<(), Box<dyn std::error::Error>> {
    use std::ffi::OsStr;
    use std::fs::File;
    use std::path::{Path, PathBuf};

    fn is_runtime_dll(name: &OsStr) -> bool {
        let s = name.to_string_lossy().to_ascii_lowercase();
        s.ends_with(".dll")
            && (s == "nvml.dll"
                || s.starts_with("cublas64_")
                || s.starts_with("cublaslt64_")
                || s.starts_with("cudart64_")
                || s.starts_with("curand64_")
                || s.starts_with("cufft64_")
                || s.starts_with("cusolver64_")
                || s.starts_with("cusparse64_"))
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let profile = env::var("PROFILE")?;

    let mut p: &Path = out_dir.as_path();
    let mut output_dir: Option<PathBuf> = None;
    while let Some(parent) = p.parent() {
        if p.file_name().and_then(|s| s.to_str()) == Some(profile.as_str()) {
            output_dir = Some(p.to_path_buf());
            break;
        }
        p = parent;
    }

    let output_dir = output_dir.ok_or("Failed to detect target output directory from OUT_DIR")?;

    let exe_path = output_dir.join("gpuf-c.exe");
    if !exe_path.is_file() {
        println!(
            "cargo:warning=Skipping tar bundling because {} is not present yet (build scripts run before final link). Re-run cargo build to generate the tar.",
            exe_path.display()
        );
        return Ok(());
    }

    let tar_path = output_dir.join("gpuf-c-bundle.tar");
    let file = File::create(&tar_path)?;
    let mut builder = tar::Builder::new(file);

    builder.append_path_with_name(&exe_path, "gpuf-c.exe")?;

    for entry in std::fs::read_dir(&output_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if !is_runtime_dll(name.as_os_str()) {
            continue;
        }
        let src = entry.path();
        if !src.is_file() {
            continue;
        }
        let dst_name = name.to_string_lossy();
        builder.append_path_with_name(&src, dst_name.as_ref())?;
    }

    builder.finish()?;

    println!(
        "cargo:warning=Created bundle tar at {}",
        tar_path.display()
    );

    Ok(())
}

fn bundle_nvml_dll_windows() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use std::path::{Path, PathBuf};

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let profile = env::var("PROFILE")?;

    let mut p: &Path = out_dir.as_path();
    let mut output_dir: Option<PathBuf> = None;
    while let Some(parent) = p.parent() {
        if p.file_name().and_then(|s| s.to_str()) == Some(profile.as_str()) {
            output_dir = Some(p.to_path_buf());
            break;
        }
        p = parent;
    }

    let output_dir = output_dir.ok_or("Failed to detect target output directory from OUT_DIR")?;

    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(nvml_path) = env::var("NVML_LIB_PATH") {
        candidates.push(PathBuf::from(&nvml_path).join("nvml.dll"));
        candidates.push(PathBuf::from(&nvml_path).join("nvml.lib"));
    }

    candidates.push(PathBuf::from(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvml.dll"));
    candidates.push(PathBuf::from(r"C:\Windows\System32\nvml.dll"));

    if let Ok(cuda_path) = env::var("CUDA_PATH") {
        candidates.push(PathBuf::from(cuda_path).join("bin").join("nvml.dll"));
    }

    let src = candidates
        .into_iter()
        .find(|p| p.is_file() && p.file_name().and_then(|s| s.to_str()) == Some("nvml.dll"));

    let Some(src) = src else {
        return Err("nvml.dll not found. Install NVIDIA driver (NVSMI) or set NVML_LIB_PATH.".into());
    };

    let dst = output_dir.join("nvml.dll");
    fs::copy(&src, &dst)?;

    println!(
        "cargo:warning=Bundled nvml.dll into {} (from {})",
        output_dir.display(),
        src.display()
    );

    Ok(())
}
