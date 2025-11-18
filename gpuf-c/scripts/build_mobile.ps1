# GPUFabric Mobile SDK Build Script

param(
    [Parameter(Mandatory=$false)]
    [ValidateSet("android", "ios", "all")]
    [string]$Platform = "all"
)

Write-Host "Building GPUFabric Mobile SDK..." -ForegroundColor Green

# Build Android
if ($Platform -eq "android" -or $Platform -eq "all") {
    Write-Host "`n=== Building for Android ===" -ForegroundColor Cyan
    
    if (-not $env:ANDROID_NDK_HOME) {
        Write-Host "Error: ANDROID_NDK_HOME not set" -ForegroundColor Red
        exit 1
    }
    
    # Android 使用 Vulkan 或 CPU
    cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build --release --features vulkan
    # 或者 CPU only: cargo ndk ... build --release --features cpu
    
    if ($LASTEXITCODE -eq 0) {
        Write-Host "Android build successful!" -ForegroundColor Green
        
        # UPX 压缩（如果 UPX 可用）
        $upxAvailable = Get-Command upx -ErrorAction SilentlyContinue
        if ($upxAvailable) {
            Write-Host "`nCompressing .so files with UPX..." -ForegroundColor Cyan
            
            $soFiles = @(
                "target/aarch64-linux-android/release/libgpuf_c.so",
                "target/armv7-linux-androideabi/release/libgpuf_c.so",
                "target/x86_64-linux-android/release/libgpuf_c.so"
            )
            
            foreach ($soFile in $soFiles) {
                if (Test-Path $soFile) {
                    # Strip debug symbols first (using llvm-strip in newer NDK versions)
                    $stripTool = "llvm-strip"
                    
                    Write-Host "  Stripping $soFile..." -ForegroundColor Yellow
                    try {
                        & $stripTool $soFile
                        Write-Host "  ✓ Stripped with $stripTool" -ForegroundColor Green
                    } catch {
                        Write-Host "  ⚠ Strip failed (continuing anyway)" -ForegroundColor Yellow
                    }
                    
                    # Then compress with UPX
                    $originalSize = (Get-Item $soFile).Length
                    upx --best --lzma $soFile
                    $compressedSize = (Get-Item $soFile).Length
                    $ratio = [math]::Round((1 - $compressedSize / $originalSize) * 100, 1)
                    Write-Host "  ✓ $(Split-Path $soFile -Leaf): $([math]::Round($compressedSize / 1MB, 2)) MB (saved $ratio%)" -ForegroundColor Green
                }
            }
        } else {
            Write-Host "`nUPX not found. Please restart your terminal/IDE after adding to PATH." -ForegroundColor Yellow
        }
        
        Write-Host "`nOutput files:"
        Write-Host "  - target/aarch64-linux-android/release/libgpuf_c.so"
        Write-Host "  - target/armv7-linux-androideabi/release/libgpuf_c.so"
        Write-Host "  - target/x86_64-linux-android/release/libgpuf_c.so"
    } else {
        Write-Host "Android build failed!" -ForegroundColor Red
        exit 1
    }
}

# Build iOS (requires macOS)
if ($Platform -eq "ios" -or $Platform -eq "all") {
    Write-Host "`n=== Building for iOS ===" -ForegroundColor Cyan
    
    if ($IsMacOS) {
        # iOS targets - 使用 Metal 加速
        cargo build --target aarch64-apple-ios --release --features metal
        cargo build --target x86_64-apple-ios --release --features metal
        cargo build --target aarch64-apple-ios-sim --release --features metal
        
        if ($LASTEXITCODE -eq 0) {
            Write-Host "iOS build successful!" -ForegroundColor Green
            Write-Host "Output files:"
            Write-Host "  - target/aarch64-apple-ios/release/libgpuf_c.a"
            Write-Host "  - target/x86_64-apple-ios/release/libgpuf_c.a"
            Write-Host "  - target/aarch64-apple-ios-sim/release/libgpuf_c.a"
            Write-Host "  - gpuf_c.h (C header file)"
        } else {
            Write-Host "iOS build failed!" -ForegroundColor Red
            exit 1
        }
    } else {
        Write-Host "iOS builds require macOS" -ForegroundColor Yellow
    }
}

Write-Host "`n=== Build Complete ===" -ForegroundColor Green
Write-Host "Header file: gpuf_c.h"
