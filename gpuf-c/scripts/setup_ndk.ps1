# NDK 设置脚本
# 使用方法：
# 1. 下载 NDK: https://developer.android.com/ndk/downloads
# 2. 解压到某个目录，例如 D:\android-ndk-r26d
# 3. 修改下面的路径为你的 NDK 路径
# 4. 运行此脚本: .\setup_ndk.ps1

# ===== 修改这里 =====
$NDK_PATH = "D:\android-ndk-r26d"  # 修改为你的 NDK 路径
# ====================

Write-Host "Setting up Android NDK..." -ForegroundColor Green

# 检查路径是否存在
if (-not (Test-Path $NDK_PATH)) {
    Write-Host "Error: NDK path not found: $NDK_PATH" -ForegroundColor Red
    Write-Host "Please download NDK from: https://developer.android.com/ndk/downloads" -ForegroundColor Yellow
    Write-Host "Then extract it and update the NDK_PATH variable in this script." -ForegroundColor Yellow
    exit 1
}

# 临时设置（当前会话）
$env:ANDROID_NDK_HOME = $NDK_PATH
Write-Host "✓ Temporary environment variable set for current session" -ForegroundColor Green
Write-Host "  ANDROID_NDK_HOME = $NDK_PATH" -ForegroundColor Cyan

# 永久设置（用户级别）
Write-Host "`nDo you want to set this permanently? (Y/N): " -NoNewline -ForegroundColor Yellow
$response = Read-Host

if ($response -eq "Y" -or $response -eq "y") {
    [System.Environment]::SetEnvironmentVariable('ANDROID_NDK_HOME', $NDK_PATH, 'User')
    Write-Host "✓ Permanent environment variable set" -ForegroundColor Green
    Write-Host "  Note: You may need to restart your terminal/IDE for changes to take effect" -ForegroundColor Yellow
} else {
    Write-Host "Skipped permanent setup. Variable is only set for current session." -ForegroundColor Yellow
}

Write-Host "`n=== Current NDK Configuration ===" -ForegroundColor Cyan
Write-Host "ANDROID_NDK_HOME = $env:ANDROID_NDK_HOME"

# 验证 NDK
$ndkBuildPath = Join-Path $env:ANDROID_NDK_HOME "ndk-build.cmd"
if (Test-Path $ndkBuildPath) {
    Write-Host "✓ NDK installation verified" -ForegroundColor Green
} else {
    Write-Host "⚠ Warning: ndk-build.cmd not found. Please verify your NDK installation." -ForegroundColor Yellow
}

Write-Host "`nYou can now run: cargo ndk -t arm64-v8a build --release" -ForegroundColor Green
