# Android SDK 测试脚本

Write-Host "=== Android ARM64 SDK Test ===" -ForegroundColor Cyan

$soFile = "D:\codedir\GPUFabric\target\aarch64-linux-android\release\libgpuf_c.so"
$targetDir = "C:\temp\android_test\jniLibs\arm64-v8a"

# 检查文件是否存在
if (-not (Test-Path $soFile)) {
    Write-Host "❌ .so file not found: $soFile" -ForegroundColor Red
    Write-Host "Please run: cargo ndk -t arm64-v8a build --release" -ForegroundColor Yellow
    exit 1
}

Write-Host "✓ Found .so file: $soFile" -ForegroundColor Green
Write-Host "  Size: $((Get-Item $soFile).Length / 1MB) MB" -ForegroundColor White

# 创建测试目录
New-Item -ItemType Directory -Force -Path $targetDir | Out-Null

# 复制文件
Copy-Item $soFile $targetDir -Force
Write-Host "✓ Copied to: $targetDir" -ForegroundColor Green

Write-Host "`n=== Next Steps ===" -ForegroundColor Cyan
Write-Host "1. Create Android Studio project" -ForegroundColor White
Write-Host "2. Copy the following files to your project:" -ForegroundColor White
Write-Host "   - $targetDir\libgpuf_c.so → app/src/main/jniLibs/arm64-v8a/" -ForegroundColor Cyan
Write-Host "   - D:\codedir\GPUFabric\gpuf-c\gpuf_c.h → app/src/main/cpp/" -ForegroundColor Cyan
Write-Host "3. Add JNA dependency to build.gradle" -ForegroundColor White
Write-Host "4. Use the test code provided" -ForegroundColor White

Write-Host "`n=== File Info ===" -ForegroundColor Cyan
Write-Host "Architecture: ARM64 (aarch64-linux-android)" -ForegroundColor White
Write-Host "File Type: Shared Library (.so)" -ForegroundColor White
Write-Host "Platform: Android" -ForegroundColor White

Write-Host "`n✅ Ready for Android testing!" -ForegroundColor Green
