# GPUFabric Mobile SDK Scripts

这个目录包含了构建和测试 GPUFabric Mobile SDK 的所有脚本。

## 📁 脚本说明

### 🔧 `build_mobile.ps1` - 主构建脚本
**用途**：构建 Android 和 iOS 库文件
```powershell
# 构建所有平台
.\build_mobile.ps1

# 只构建 Android
.\build_mobile.ps1 -Platform android

# 只构建 iOS（需要 macOS）
.\build_mobile.ps1 -Platform ios
```

**功能**：
- ✅ Android NDK 构建（arm64-v8a, armeabi-v7a, x86_64）
- ✅ iOS 构建（aarch64-apple-ios, x86_64-apple-ios）
- ✅ 自动 UPX 压缩（如果已安装）
- ✅ 生成 C 头文件

### ⚙️ `setup_ndk.ps1` - 环境配置
**用途**：设置 Android NDK 环境
```powershell
# 修改脚本中的 NDK_PATH，然后运行
.\setup_ndk.ps1
```

**功能**：
- ✅ 检查 NDK 安装
- ✅ 设置 ANDROID_NDK_HOME 环境变量
- ✅ 验证配置

### 📱 `test_android.ps1` - 测试准备
**用途**：准备 Android 测试文件
```powershell
.\test_android.ps1
```

**功能**：
- ✅ 复制 .so 文件到测试目录
- ✅ 生成测试项目结构
- ✅ 验证文件完整性

## 🚀 快速开始

### 1. 环境准备
```powershell
# 安装 NDK（如果还没有）
.\setup_ndk.ps1

# 安装 UPX（可选，用于压缩）
# 下载：https://upx.github.io/
# 或运行：winget install UPX
```

### 2. 构建 SDK
```powershell
# 构建 Android 库
.\build_mobile.ps1 -Platform android

# 准备测试文件
.\test_android.ps1
```

### 3. 测试
1. 打开 Android Studio
2. 导入 `C:\temp\android_test` 项目
3. 连接 ARM64 设备
4. 运行测试

## 📂 输出文件

构建完成后，重要文件位于：

```
gpuf-c/
├── target/aarch64-linux-android/release/
│   └── libgpuf_c.so                    # Android ARM64 库
├── target/armv7-linux-androideabi/release/
│   └── libgpuf_c.so                    # Android ARMv7 库
├── target/x86_64-linux-android/release/
│   └── libgpuf_c.so                    # Android x86_64 库
└── gpuf_c.h                            # C 头文件

C:\temp\android_test\                    # 测试项目
├── jniLibs/arm64-v8a/libgpuf_c.so      # 测试用库文件
└── README.md                            # 测试说明
```

## ⚠️ 注意事项

1. **Windows 专用**：这些脚本为 Windows PowerShell 设计
2. **管理员权限**：某些操作可能需要管理员权限
3. **网络要求**：首次构建需要下载依赖
4. **磁盘空间**：完整构建需要约 2GB 空间

## 🔍 故障排除

### NDK 相关问题
```powershell
# 检查 NDK 是否正确设置
echo $env:ANDROID_NDK_HOME

# 重新设置 NDK
.\setup_ndk.ps1
```

### 构建失败
```powershell
# 清理构建缓存
cargo clean

# 重新构建
.\build_mobile.ps1 -Platform android
```

### UPX 压缩问题
```powershell
# 检查 UPX 是否安装
upx --version

# 手动压缩
upx --best --lzma libgpuf_c.so
```

## 📝 更新日志

- **2025-11-18**: 创建脚本目录，整理构建流程
- **2025-11-18**: 添加 UPX 自动压缩
- **2025-11-18**: 集成 llama.cpp 支持
