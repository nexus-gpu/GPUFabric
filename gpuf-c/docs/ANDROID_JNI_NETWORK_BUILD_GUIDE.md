# Android JNI 网络库构建指南

## 📋 概述

本文档记录了在 Android 平台上构建包含网络依赖的 Rust JNI 库时遇到的问题、解决方案和经验教训。

**项目背景：**
- 目标：构建包含 llama.cpp 推理 + 网络通信功能的 Android JNI 库
- 技术栈： Rust + JNI + llama.cpp + reqwest + tokio-rustls + aws-lc-rs
- 构建方式：三步构建法（静态库 → 静态库 → 动态库）

---

## 🚨 遇到的主要问题

### 问题 1: aws-lc-sys 编译失败

**错误现象：**
```bash
ld.lld: error: cannot open crtbegin_dynamic.o: No such file or directory
ld.lld: error: cannot open crtend_android.o: No such file or directory
```

**根本原因：**
- 依赖链：`reqwest` → `tokio-rustls` → `rustls` → `aws-lc-rs` → `aws-lc-sys`
- `aws-lc-sys` 使用 CMake 编译 C/C++ 代码，但目标 triple 配置错误
- CMake 使用了 `aarch64-none-linux-android21` 而不是正确的 `aarch64-linux-android21`

**解决方案：**
```bash
# 关键环境变量配置
export RUSTFLAGS="-A warnings -C target-feature=+crt-static"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-A warnings -C target-feature=+crt-static"
```

---

### 问题 2: C++ 运行时库链接错误

**错误现象：**
```bash
dlopen failed: cannot locate symbol "_ZNSt6__ndk112basic_stringIcNS_11char_traitsIcEENS_9allocatorIcEEED2Ev"
```

**根本原因：**
- Rust 代码通过某些依赖使用了 C++ 特性
- 需要链接 `libc++_shared.so` 但运行时找不到

**解决方案：**
1. **编译时链接：**
```bash
$NDK_CLANG -shared -o libgpuf_c.so \
    [其他参数...] \
    -lc++_shared
```

2. **运行时预加载：**
```c
// 预加载 C++ 运行时库
void* cpp_handle = dlopen("/data/local/tmp/libc++_shared.so", RTLD_NOW | RTLD_GLOBAL);
void* handle = dlopen("/data/local/tmp/libgpuf_c.so", RTLD_NOW | RTLD_GLOBAL);
```

---

### 问题 3: cargo-ndk 与 C++ 链接冲突

**问题：**
- `cargo-ndk` 简化了构建流程，但会出现 C++ 链接问题
- 网络依赖需要复杂的链接配置

**解决方案：**
- **避免使用 cargo-ndk**
- **改用手动构建流程：**
```bash
# 步骤 1: 编译 Rust 静态库
cargo rustc --target aarch64-linux-android --release --lib -- --crate-type=staticlib

# 步骤 2: NDK 链接所有静态库
$NDK_CLANG -shared -o libgpuf_c.so \
    -Wl,--whole-archive \
    libgpuf_c.a \
    libllama.a \
    libggml*.a \
    -Wl,--no-whole-archive \
    -lc++_shared -llog -ldl -lm -latomic
```

---

### 问题 4: 函数调用段错误

**错误现象：**
```bash
Segmentation fault (exit code 139)
```

**根本原因：**
- 测试程序直接调用函数，缺乏错误处理
- 可能是 Rust 运行时初始化问题

**解决方案：**
- **渐进式测试策略：**
```c
// 1. 基础加载测试
void* handle = dlopen("libgpuf_c.so", RTLD_NOW | RTLD_GLOBAL);

// 2. 符号解析测试（不调用）
void* func = dlsym(handle, "gpuf_version");

// 3. 安全调用测试（带信号处理）
signal(SIGSEGV, signal_handler);
const char* version = gpuf_version();
```

---

## 🏗️ x86_64 Android 构建专题

### 问题 5: x86_64 架构 llama.cpp 编译失败

**错误现象：**
```bash
/home/jack/codedir/GPUFabric/llama.cpp/src/llama-mmap.cpp:294:71: 
error: use of undeclared identifier 'POSIX_MADV_WILLNEED'
/home/jack/codedir/GPUFabric/llama.cpp/src/llama-mmap.cpp:300:51: 
error: use of undeclared identifier 'POSIX_MADV_RANDOM'
```

**根本原因：**
- `posix_madvise` 函数在 Android x86_64 上不可用
- `POSIX_MADV_WILLNEED` 和 `POSIX_MADV_RANDOM` 宏未定义
- llama.cpp 依赖的 POSIX API 在 x86_64 模拟器上缺失

**解决方案对比：**
1. ❌ **修改 llama.cpp 源码** - 违反不修改第三方库原则
2. ❌ **宏定义禁用** - `CMAKE_C_FLAGS="-DHAVE_POSIX_MADVISE=0"` 无法完全解决问题
3. ✅ **API 兼容层** - 创建纯 Rust 实现的 llama.cpp 接口

### x86_64 兼容层实现方案

**核心思路：** 不追求真实推理，实现 API 完全兼容

```rust
// 兼容层类型定义
#[repr(C)]
pub struct llama_model {
    _private: [u8; 0], // 零大小类型
}

#[repr(C)]
pub struct llama_context_params {
    pub n_ctx: u32,
    pub n_batch: u32,
    pub n_gpu_layers: i32,
    // ... 其他参数
}

// 兼容层函数实现
#[no_mangle]
pub extern "C" fn llama_print_system_info() -> *const c_char {
    let info = CString::new("x86_64 Android (ARM64 Compatibility Layer)\nArchitecture: x86_64\nPlatform: Android Emulator\nLLAMA Backend: Simulated").unwrap();
    info.into_raw()
}

#[no_mangle]
pub extern "C" fn llama_load_model_from_file(
    path_model: *const c_char,
    params: llama_model_params,
) -> *mut llama_model {
    if path_model.is_null() { return std::ptr::null_mut(); }
    
    unsafe {
        let path = CStr::from_ptr(path_model);
        if let Ok(path_str) = path.to_str() {
            println!("📁 [x86_64 COMPAT] Attempting to load model: {}", path_str);
            
            if path_str.ends_with(".gguf") {
                println!("✅ [x86_64 COMPAT] Model file format recognized");
                let model = Box::new(());
                Box::into_raw(model) as *mut llama_model
            } else {
                std::ptr::null_mut()
            }
        } else {
            std::ptr::null_mut()
        }
    }
}
```

### x86_64 构建脚本

**兼容层构建 (`build_x86_64_with_arm64_lib.sh`)：**
```bash
#!/bin/bash
set -e

# x86_64 专用环境配置
export ANDROID_NDK_ROOT="/home/jack/android-ndk-r27d"
export TARGET_TRIPLE="x86_64-linux-android21"
export CC="$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/x86_64-linux-android21-clang"
export RUSTFLAGS="-A warnings -C target-feature=+crt-static"

# 创建兼容版本的 Cargo.toml
cat > Cargo.toml.x86_64_compat << 'EOF'
[package]
name = "gpuf-c"
version = "0.1.0"
edition = "2021"

[lib]
name = "gpuf_c"
crate-type = ["cdylib", "staticlib", "rlib"]

[dependencies]
jni = "0.21"
libc = "0.2"
log = "0.4"
env_logger = "0.10"

[target.'cfg(target_os = "android")'.dependencies]
android_logger = "0.13"

[features]
android = []
network = []  # x86_64 版本禁用网络依赖
default = []
EOF

# 使用兼容版本
cp Cargo.toml.x86_64_compat Cargo.toml
cp src/lib_compat_x86_64.rs src/lib.rs

# 编译 Rust 静态库
cargo clean
cargo rustc --target x86_64-linux-android --release --lib -- --crate-type=staticlib

# NDK 链接（无 C++ 依赖）
$NDK_CLANG -shared -o libgpuf_c_compat_x86_64.so \
    -Wl,--whole-archive \
    /home/jack/codedir/GPUFabric/target/x86_64-linux-android/release/libgpuf_c.a \
    -Wl,--no-whole-archive \
    -llog -ldl -lm -latomic
```

### x86_64 兼容性测试

**完整测试程序 (`test_compat_x86_64.c`)：**
```c
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dlfcn.h>

int main() {
    printf("🧪 x86_64 Android COMPAT Library Test\n");
    
    // 加载兼容 x86_64 库
    void* handle = dlopen("/data/local/tmp/libgpuf_c_compat_x86_64.so", RTLD_NOW);
    if (!handle) {
        printf("❌ Failed to load library: %s\n", dlerror());
        return 1;
    }
    
    // 测试 llama.cpp API 兼容性
    typedef const char* (*llama_print_system_info_func)();
    typedef void* (*llama_load_model_from_file_func)(const char* path_model, llama_model_params params);
    
    llama_print_system_info_func llama_print_system_info = dlsym(handle, "llama_print_system_info");
    
    if (llama_print_system_info) {
        printf("🖥️  Llama System Info:\n%s\n", llama_print_system_info());
    }
    
    // 测试高级接口
    typedef int (*gpuf_test_llama_compatibility_func)();
    gpuf_test_llama_compatibility_func gpuf_test_llama_compatibility = dlsym(handle, "gpuf_test_llama_compatibility");
    
    if (gpuf_test_llama_compatibility) {
        printf("🧪 Testing llama.cpp API compatibility...\n");
        int result = gpuf_test_llama_compatibility();
        printf("   Compatibility result: %d\n", result);
    }
    
    printf("✅ x86_64 compatibility test completed!\n");
    dlclose(handle);
    return 0;
}
```

---

## 🛠️ 完整解决方案

### 构建脚本

**完整版 (`build_android_with_network.sh`)：**
```bash
#!/bin/bash
set -e

# 环境配置
export ANDROID_NDK_ROOT="/home/jack/android-ndk-r27d"
export RUSTFLAGS="-A warnings -C target-feature=+crt-static"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-A warnings -C target-feature=+crt-static"

# 恢复原版 Cargo.toml（包含网络依赖）
cp Cargo.toml.backup Cargo.toml

# 编译 Rust 静态库
cargo rustc --target aarch64-linux-android --release --lib -- --crate-type=staticlib

# NDK 链接
$NDK_CLANG -shared -o libgpuf_c.so \
    -Wl,--whole-archive \
    /path/to/libgpuf_c.a \
    llama-android-ndk/libllama.a \
    llama-android-ndk/libggml*.a \
    -Wl,--no-whole-archive \
    -lc++_shared -llog -ldl -lm -latomic
```

### 测试程序

**安全测试 (`test_safe_jni.c`)：**
```c
#include <stdio.h>
#include <dlfcn.h>
#include <signal.h>
#include <setjmp.h>

jmp_buf jump_buffer;
void signal_handler(int sig) {
    longjmp(jump_buffer, 1);
}

int main() {
    signal(SIGSEGV, signal_handler);
    
    if (setjmp(jump_buffer) != 0) {
        printf("Signal caught, aborting\n");
        return 1;
    }
    
    // 预加载 C++ 运行时
    void* cpp_handle = dlopen("libc++_shared.so", RTLD_NOW | RTLD_GLOBAL);
    void* handle = dlopen("libgpuf_c.so", RTLD_NOW | RTLD_GLOBAL);
    
    // 安全测试函数
    typedef const char* (*gpuf_version_func)(void);
    gpuf_version_func gpuf_version = dlsym(handle, "gpuf_version");
    printf("Version: %s\n", gpuf_version());
    
    dlclose(handle);
    dlclose(cpp_handle);
    return 0;
}
```

---

## 📚 经验总结

### 1. 依赖管理经验

**教训：** 网络库依赖链复杂，容易引发编译问题
**经验：**
- 提前分析依赖链：`reqwest` → `tokio-rustls` → `rustls` → `aws-lc-rs`
- 使用 `cargo tree` 查看完整依赖树
- 准备最小化配置作为备选方案

### 2. 架构兼容性经验

**教训：** ARM64 成功不等于 x86_64 也能成功
**经验：**
- 不同架构的 Android API 支持程度不同
- POSIX 函数在 x86_64 模拟器上可能缺失
- 需要为不同架构准备不同的构建策略
- API 兼容层是解决架构差异的有效方案

### 3. 构建工具选择

**教训：** `cargo-ndk` 虽然方便，但复杂项目会有限制
**经验：**
- 简单项目：可以使用 `cargo-ndk`
- 复杂项目（C++/网络依赖）：使用 `cargo rustc` + 手动 NDK 链接
- x86_64 项目：必须使用手动构建以控制依赖
- 保持构建过程的可控性和可调试性

### 4. 链接配置经验

**教训：** Android NDK 链接配置细节繁多
**经验：**
- 使用 `--whole-archive` 确保符号完整性
- 始终链接 C++ 运行时库 `-lc++_shared`
- x86_64 版本可以避免 C++ 依赖，简化链接
- 检查 NDK 版本兼容性

### 5. 第三方库集成经验

**教训：** 修改第三方源码会带来维护噩梦
**经验：**
- 坚持不修改第三方库源码的原则
- 使用兼容层或适配器模式解决接口问题
- 通过条件编译或宏定义处理平台差异
- 保持代码的可维护性和可升级性

### 6. 测试策略经验

**教训：** 直接测试容易掩盖问题
**经验：**
- 渐进式测试：加载 → 符号解析 → 安全调用 → 完整功能
- 使用信号处理防止崩溃
- 在设备上测试，而不只是编译
- 为不同架构准备不同的测试用例

---

## ⚠️ 相同场景注意事项

### 1. 项目规划阶段

**技术选型：**
- 评估是否真的需要网络功能
- 考虑替代方案：最小依赖 + 分离网络模块
- 提前验证关键依赖的 Android 兼容性

**依赖分析：**
```bash
# 分析依赖链
cargo tree --target aarch64-linux-android

# 检查问题依赖
cargo tree -i aws-lc-sys
```

### 2. 构建环境配置

**NDK 配置：**
- 使用稳定版本的 NDK（推荐 r27d）
- 确保目标架构匹配（aarch64-linux-android）
- 配置正确的环境变量

**Rust 配置：**
```bash
# 关键配置
export RUSTFLAGS="-A warnings -C target-feature=+crt-static"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-A warnings -C target-feature=+crt-static"
```

### 3. 构建流程设计

**推荐流程：**
1. **分离构建步骤** - 便于调试和问题定位
2. **保留中间文件** - 便于分析和复用
3. **自动化脚本** - 减少人为错误
4. **版本控制** - 跟踪配置变更

### 4. 测试验证策略

**测试层次：**
1. **编译测试** - 确保能成功构建
2. **链接测试** - 确保库能正常加载
3. **符号测试** - 确保接口存在
4. **功能测试** - 确保实际工作
5. **集成测试** - 确保在应用中正常

**错误处理：**
- 使用信号处理器捕获崩溃
- 实现渐进式测试策略
- 保留详细的错误日志

### 5. 部署和维护

**部署注意：**
- 确保目标设备的架构兼容
- 部署所有必需的运行时库
- 测试不同 Android 版本的兼容性

**维护建议：**
- 定期更新依赖版本
- 监控上游库的变更
- 保持构建脚本的更新

---

## 🎯 最佳实践总结

### ✅ 推荐做法

1. **依赖最小化** - 只包含真正需要的功能
2. **构建可控化** - 使用手动构建而非自动化工具
3. **测试渐进化** - 分步骤验证每个环节
4. **错误可追踪** - 保留完整的构建和测试日志
5. **配置版本化** - 将成功的配置纳入版本控制
6. **架构特定策略** - 为不同架构准备不同的构建方案
7. **API 兼容层设计** - 使用兼容层解决架构差异问题
8. **不修改第三方源码** - 保持代码的可维护性

### ❌ 避免做法

1. **盲目依赖 cargo-ndk** - 复杂项目容易出问题
2. **忽略 C++ 依赖** - Rust 依赖可能间接引入 C++ 代码
3. **跳过测试步骤** - 编译成功不代表运行正常
4. **忽略环境变量** - 正确的配置是成功的关键
5. **单一构建方案** - 准备备选方案应对问题
6. **假设架构一致性** - ARM64 成功不等于 x86_64 成功
7. **直接修改第三方库** - 会带来维护和升级问题
8. **忽略平台差异** - 不同平台的 API 支持程度不同

---

## 🏗️ 最终解决方案矩阵

| 架构 | 构建脚本 | 库文件 | 大小 | llama.cpp | 适用场景 |
|------|----------|--------|------|-----------|----------|
| **ARM64** | `build_android_with_network.sh` | `libgpuf_c.so` | 40MB | ✅ Real | 真实设备生产环境 |
| **x86_64** | `build_x86_64_with_arm64_lib.sh` | `libgpuf_c_compat_x86_64.so` | 5.8MB | ✅ API | 模拟器开发测试 |

### 使用指南

**ARM64 真实设备（完整功能）：**
```bash
./build_android_with_network.sh
# 生成 libgpuf_c.so (40MB) - 包含完整 llama.cpp 推理功能
```

**x86_64 模拟器（接口开发）：**
```bash
./build_x86_64_with_arm64_lib.sh
# 生成 libgpuf_c_compat_x86_64.so (5.8MB) - llama.cpp API 兼容
```

**测试验证：**
```bash
# 编译测试程序
export NDK_CLANG="/home/jack/android-ndk-r27d/toolchains/llvm/prebuilt/linux-x86_64/bin/x86_64-linux-android21-clang"
$NDK_CLANG -o test_compat_x86_64 test_compat_x86_64.c -ldl

# 部署并测试
adb push test_compat_x86_64 libgpuf_c_compat_x86_64.so /data/local/tmp/
adb shell /data/local/tmp/test_compat_x86_64
```

---

## 📞 技术支持

**常见问题排查：**
1. **编译失败** - 检查环境变量和 NDK 配置
2. **链接错误** - 确认所有静态库和运行时库
3. **运行时崩溃** - 使用 logcat 和符号表分析
4. **性能问题** - 检查 LTO 和优化配置
5. **x86_64 llama.cpp 失败** - 使用 API 兼容层方案

**调试工具：**
- `nm` - 检查符号表
- `readelf` - 分析库文件
- `adb logcat` - 查看运行时日志
- `addr2line` - 符号化崩溃地址

**架构特定问题：**
- **ARM64**: 关注 C++ 运行时和网络依赖
- **x86_64**: 关注 POSIX API 兼容性和模拟器限制

---

*本文档基于实际项目经验编写，包含 ARM64 和 x86_64 双架构构建方案，持续更新中...*
