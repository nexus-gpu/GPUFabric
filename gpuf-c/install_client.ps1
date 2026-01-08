param(
    [string]$BaseUrl = "https://oss.gpunexus.com/client",
    [string]$InstallDir = "$env:USERPROFILE\AppData\Local\Programs\gpuf-c",
    [string]$PackageName = "v1.0.0-windows-gpuf-c.tar.gz"
)

# check if running as administrator
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "error: please run this script as administrator" -ForegroundColor Red
    exit 1
}

$ErrorActionPreference = 'Stop'

function Parse-Version([string]$v) {
    try { return [version]$v } catch { return $null }
}

function Get-CudaVersion {
    # Prefer nvcc if available
    $nvcc = Get-Command nvcc -ErrorAction SilentlyContinue
    if ($nvcc) {
        $out = & nvcc --version 2>$null
        $m = [regex]::Match(($out | Out-String), "release\s+([0-9]+\.[0-9]+)")
        if ($m.Success) { return $m.Groups[1].Value }
    }

    # Prefer nvidia-smi (works even without CUDA Toolkit)
    $smi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
    if ($smi) {
        $out = & nvidia-smi 2>$null
        $m = [regex]::Match(($out | Out-String), "CUDA Version:\s*([0-9]+\.[0-9]+)")
        if ($m.Success) { return $m.Groups[1].Value }
    }

    return $null
}

function Has-Vulkan {
    $dll1 = Join-Path $env:WINDIR "System32\vulkan-1.dll"
    $dll2 = Join-Path $env:WINDIR "SysWOW64\vulkan-1.dll"
    return (Test-Path $dll1) -or (Test-Path $dll2)
}

function Get-Md5PrefixFromFileName([string]$Path) {
    $name = [System.IO.Path]::GetFileName($Path)
    $m = [regex]::Match($name, "^([0-9a-fA-F]{6})-")
    if ($m.Success) { return $m.Groups[1].Value.ToLower() }
    return $null
}

function Verify-Md5PrefixIfPossible([string]$Path) {
    $prefix = Get-Md5PrefixFromFileName $Path
    if (-not $prefix) {
        Write-Host "warning: md5 prefix not found in filename (skip md5 prefix check): $([System.IO.Path]::GetFileName($Path))" -ForegroundColor Yellow
        return
    }

    $md5 = (Get-FileHash -Algorithm MD5 -Path $Path).Hash.ToLower()
    if ($md5.Substring(0, 6) -ne $prefix) {
        Write-Host "error: md5 prefix mismatch for $Path" -ForegroundColor Red
        Write-Host "expected prefix: $prefix" -ForegroundColor Yellow
        Write-Host "actual md5:      $md5" -ForegroundColor Yellow
        exit 1
    }

    Write-Host "md5 prefix match ok: $md5" -ForegroundColor Green
}

$hasVulkan = Has-Vulkan
$cudaVersionStr = Get-CudaVersion
$cudaVersion = $null
if ($cudaVersionStr) { $cudaVersion = Parse-Version $cudaVersionStr }

$cudaOk = $false
if ($cudaVersion) {
    $cudaOk = $cudaVersion -ge (Parse-Version "13.0")
}

if (-not $hasVulkan -and -not $cudaOk) {
    Write-Host "error: Windows requires Vulkan runtime OR CUDA version >= 13.0" -ForegroundColor Red
    if ($hasVulkan) {
        Write-Host "Vulkan detected" -ForegroundColor Green
    } else {
        Write-Host "Vulkan not detected (vulkan-1.dll not found)" -ForegroundColor Yellow
    }
    if ($cudaVersionStr) {
        Write-Host "CUDA detected: $cudaVersionStr (require >= 13.0)" -ForegroundColor Yellow
    } else {
        Write-Host "CUDA not detected (nvidia-smi/nvcc/registry not found)" -ForegroundColor Yellow
    }
    exit 1
}

$pkgUrl = "$BaseUrl/$PackageName"
$archivePath = Join-Path $env:TEMP $PackageName

try {
    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    Write-Host "Downloading: $pkgUrl" -ForegroundColor Yellow
    (New-Object System.Net.WebClient).DownloadFile($pkgUrl, $archivePath)

    # Extract (.tar.gz) using tar.exe (available on most Windows 10/11)
    $tar = Get-Command tar -ErrorAction SilentlyContinue
    if (-not $tar) {
        Write-Host "error: tar command not found. Please install tar/bsdtar or use a zip-based package." -ForegroundColor Red
        exit 1
    }

    Write-Host "Extracting to: $InstallDir" -ForegroundColor Yellow
    & tar -xzf $archivePath -C $InstallDir

    # Expect gpuf-c.exe inside root of the archive.
    # But releases may contain a top-level folder, so search recursively.
    $exe = Join-Path $InstallDir "gpuf-c.exe"
    if (-not (Test-Path $exe)) {
        $candidate = Get-ChildItem -Path $InstallDir -Recurse -Filter "gpuf-c.exe" -File -ErrorAction SilentlyContinue | Select-Object -First 1
        if (-not $candidate) {
            $candidate = Get-ChildItem -Path $InstallDir -Recurse -Filter "*gpuf-c*.exe" -File -ErrorAction SilentlyContinue | Select-Object -First 1
        }
        if (-not $candidate) {
            $candidate = Get-ChildItem -Path $InstallDir -Recurse -Filter "*.exe" -File -ErrorAction SilentlyContinue | Select-Object -First 1
        }
        if (-not $candidate) {
            Write-Host "error: no .exe found after extraction in $InstallDir" -ForegroundColor Red
            Write-Host "hint: archive may contain unexpected layout or is not a Windows package" -ForegroundColor Yellow
            Write-Host "extracted files (top 50):" -ForegroundColor Yellow
            Get-ChildItem -Path $InstallDir -Recurse -Force -ErrorAction SilentlyContinue | Select-Object -First 50 FullName
            exit 1
        }

        Verify-Md5PrefixIfPossible $candidate.FullName

        $srcDir = $candidate.DirectoryName
        Copy-Item -Path $candidate.FullName -Destination $exe -Force

        # Copy adjacent runtime DLLs next to gpuf-c.exe (required for CUDA builds on Windows)
        $dlls = Get-ChildItem -Path $srcDir -Filter "*.dll" -File -ErrorAction SilentlyContinue
        foreach ($d in $dlls) {
            Copy-Item -Path $d.FullName -Destination (Join-Path $InstallDir $d.Name) -Force
        }

        # Copy common ancillary files if present
        $extras = @("ca-cert.pem", "read.txt")
        foreach ($e in $extras) {
            $p = Join-Path $srcDir $e
            if (Test-Path $p) {
                Copy-Item -Path $p -Destination (Join-Path $InstallDir $e) -Force
            }
        }
    }

    # add to PATH
    $currentPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($currentPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable('Path', "$currentPath;$InstallDir", 'User')
        $env:Path += ";$InstallDir"
    }

    Remove-Item -Path $archivePath -Force -ErrorAction SilentlyContinue

    Write-Host "gpuf-c (llama.cpp) installed successfully!" -ForegroundColor Green
    Write-Host "InstallDir: $InstallDir" -ForegroundColor Yellow
    Write-Host "Please restart terminal to make PATH changes take effect." -ForegroundColor Yellow

} catch {
    Write-Host "installation failed: $_" -ForegroundColor Red
    exit 1
}
