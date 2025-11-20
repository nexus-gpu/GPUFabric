# Building gpuf-c on Windows 11

## Issue
The compilation fails with:
```
LINK : fatal error LNK1181: cannot open input file 'nvml.lib'
```

## Root Cause
The `nvml-wrapper` crate requires NVIDIA Management Library (NVML), which is distributed with NVIDIA CUDA Toolkit, not just the GPU drivers.

## ✅ Quick Solution (Recommended)

**Build without NVML support:**
```powershell
cargo build --release --bin gpuf-c --no-default-features
```

This builds gpuf-c without NVIDIA GPU monitoring. The binary will work but won't report GPU metrics.

## Alternative Solutions

### Option 1: Install CUDA Toolkit (For NVIDIA GPU monitoring)

1. Download and install NVIDIA CUDA Toolkit from:
   https://developer.nvidia.com/cuda-downloads

2. After installation, nvml.lib will be located at:
   ```
   C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.x\lib\x64\nvml.lib
   ```

3. Build again:
   ```powershell
   cargo build --release --bin gpuf-c
   ```

### Option 2: Set NVML_LIB_PATH Environment Variable

If you have nvml.lib in a custom location:

```powershell
$env:NVML_LIB_PATH = "C:\path\to\nvml\lib"
cargo build --release --bin gpuf-c
```

### Option 3: Build With NVML Support Enabled

If you have CUDA Toolkit installed and want GPU monitoring:

```powershell
cargo build --release --bin gpuf-c --features nvml
```

Or enable it by default (already configured in Cargo.toml):
```powershell
cargo build --release --bin gpuf-c
```

**Note:** The code has been updated to make NVML optional. By default, it's enabled but will gracefully fall back if CUDA Toolkit is not installed.

### Option 4: Use Dynamic Loading (Advanced)

Instead of linking nvml.lib at compile time, load nvml.dll dynamically at runtime. This requires more code changes but allows the binary to work on systems without CUDA Toolkit.

## Verification

After successful build, verify:
```powershell
.\target\release\gpuf-c.exe --version
```

## Notes

- The build.rs has been updated to automatically search for nvml.lib in common CUDA Toolkit locations
- If you're developing on a machine without NVIDIA GPU, Option 3 is recommended
- For production deployment on NVIDIA GPU systems, Option 1 is recommended
