# Android 构建脚本 - 一键完成所有配置和构建
# 使用方法: .\build-android.ps1

Write-Host "=== Android Build Script ===" -ForegroundColor Cyan
Write-Host ""

# Step 1: 配置环境变量
Write-Host "[1/4] Configuring environment variables..." -ForegroundColor Yellow
$env:Path = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path","User")
$ndkPath = "C:\Users\admin\AppData\Local\Android\Sdk\ndk\29.0.14206865"
$env:CMAKE_GENERATOR = "Ninja"
$env:ANDROID_NDK_HOME = $ndkPath
$env:ANDROID_NDK_ROOT = $ndkPath
$env:ANDROID_NDK = $ndkPath

# 验证环境
Write-Host "  ✓ CMAKE_GENERATOR  = $env:CMAKE_GENERATOR" -ForegroundColor Green
Write-Host "  ✓ ANDROID_NDK_ROOT = $env:ANDROID_NDK_ROOT" -ForegroundColor Green

# 验证工具
try {
    $ninjaVersion = ninja --version 2>&1
    Write-Host "  ✓ Ninja version    = $ninjaVersion" -ForegroundColor Green
} catch {
    Write-Host "  ✗ Ninja not found! Please install: winget install Ninja-build.Ninja" -ForegroundColor Red
    exit 1
}

try {
    $cmakeVersion = cmake --version 2>&1 | Select-Object -First 1
    Write-Host "  ✓ CMake found" -ForegroundColor Green
} catch {
    Write-Host "  ✗ CMake not found! Please install: winget install Kitware.CMake" -ForegroundColor Red
    exit 1
}

# 检查 LLVM (libclang)
$llvmPath = "C:\Program Files\LLVM\bin"
if (Test-Path $llvmPath) {
    $env:LIBCLANG_PATH = $llvmPath
    Write-Host "  ✓ LLVM/Clang found at $llvmPath" -ForegroundColor Green
} else {
    Write-Host "  ⚠ LLVM not found. If build fails, install: winget install LLVM.LLVM" -ForegroundColor Yellow
}

Write-Host ""

# Step 2: 清理构建缓存
Write-Host "[2/4] Cleaning build cache..." -ForegroundColor Yellow
Remove-Item -Recurse -Force .\target\aarch64-linux-android\release\build -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force .\target\armv7-linux-androideabi\release\build -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force .\target\x86_64-linux-android\release\build -ErrorAction SilentlyContinue
Write-Host "  ✓ Build cache cleaned" -ForegroundColor Green
Write-Host ""

# Step 3: 验证 Rust 目标
Write-Host "[3/4] Verifying Rust targets..." -ForegroundColor Yellow
$targets = @("aarch64-linux-android", "armv7-linux-androideabi", "x86_64-linux-android")
$installedTargets = rustup target list --installed
foreach ($target in $targets) {
    if ($installedTargets -contains $target) {
        Write-Host "  ✓ $target installed" -ForegroundColor Green
    } else {
        Write-Host "  ⚠ Installing $target..." -ForegroundColor Yellow
        rustup target add $target
    }
}
Write-Host ""

# Step 4: 开始构建
Write-Host "[4/4] Starting build..." -ForegroundColor Yellow
Write-Host "  Command: cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build --release" -ForegroundColor Cyan
Write-Host ""

cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build --release

if ($LASTEXITCODE -eq 0) {
    Write-Host ""
    Write-Host "=== Build Successful! ===" -ForegroundColor Green
    Write-Host "Output libraries are in:" -ForegroundColor Cyan
    Write-Host "  - target\aarch64-linux-android\release\" -ForegroundColor White
    Write-Host "  - target\armv7-linux-androideabi\release\" -ForegroundColor White
    Write-Host "  - target\x86_64-linux-android\release\" -ForegroundColor White
} else {
    Write-Host ""
    Write-Host "=== Build Failed ===" -ForegroundColor Red
    Write-Host "Exit code: $LASTEXITCODE" -ForegroundColor Red
    exit $LASTEXITCODE
}
