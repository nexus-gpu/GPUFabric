param(
    [string]$BaseUrl = "https://oss.gpunexus.com/client",
    [string]$InstallDir = "$env:USERPROFILE\AppData\Local\Programs\gpuf-c",
    [string]$PackageName = "v1.0.2-windows-gpuf-c.tar.gz",
    [string]$DownloadDir = "C:\gpuf",
    [string]$PackageSha256 = ""
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

function Get-ExpectedVersionFromPackageName([string]$Name) {
    try {
        $m = [regex]::Match($Name, "^v?([0-9]+\.[0-9]+\.[0-9]+)")
        if ($m.Success) {
            return $m.Groups[1].Value
        }
    } catch {
    }
    return $null
}

function Get-InstalledVersionMarker([string]$Dir) {
    try {
        $marker = Join-Path $Dir ".gpuf_version"
        if (Test-Path $marker) {
            $v = (Get-Content -Path $marker -ErrorAction SilentlyContinue | Select-Object -First 1)
            if ($v) {
                $m = [regex]::Match($v, "([0-9]+\.[0-9]+\.[0-9]+)")
                if ($m.Success) {
                    return $m.Groups[1].Value
                }
            }
        }
    } catch {
    }
    return $null
}

function Get-InstalledGpufVersion([string]$ExePath) {
    if (-not (Test-Path $ExePath)) {
        return $null
    }

    try {
        $out = & $ExePath --version 2>&1
        $s = ($out | Out-String)
        $m = [regex]::Match($s, "([0-9]+\.[0-9]+\.[0-9]+)")
        if ($m.Success) {
            return $m.Groups[1].Value
        }
    } catch {
    }

    return $null
}

function Ensure-InstallDirOnPath([string]$Dir) {
    $currentPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($currentPath -notlike "*$Dir*") {
        [Environment]::SetEnvironmentVariable('Path', "$currentPath;$Dir", 'User')
        $env:Path += ";$Dir"
    }
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

function Read-Sha256File([string]$Path, [string]$ArchiveName) {
    if (-not (Test-Path $Path)) { return $null }
    $lines = Get-Content -Path $Path | ForEach-Object { $_.Trim() }
    foreach ($line in $lines) {
        if ($line -match '^([0-9a-fA-F]{64})(\s+|$)') {
            if (($line -match [regex]::Escape($ArchiveName)) -or ($lines.Count -eq 1)) {
                return $Matches[1].ToLower()
            }
        }
    }
    return $null
}

function Verify-Sha256Required([string]$Path, [string]$Expected) {
    if (-not $Expected) {
        Write-Host "error: sha256 check failed: expected hash missing" -ForegroundColor Red
        exit 1
    }
    $expectedLower = $Expected.ToLower()
    if ($expectedLower -notmatch '^[0-9a-f]{64}$') {
        Write-Host "error: sha256 check failed: invalid expected hash format" -ForegroundColor Red
        exit 1
    }
    if (-not (Test-Path $Path)) {
        Write-Host "error: sha256 check failed: file not found: $Path" -ForegroundColor Red
        exit 1
    }
    $actual = (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLower()
    if ($actual -ne $expectedLower) {
        Write-Host "error: sha256 mismatch for $Path" -ForegroundColor Red
        Write-Host "expected: $expectedLower" -ForegroundColor Yellow
        Write-Host "actual:   $actual" -ForegroundColor Yellow
        exit 1
    }
    Write-Host "sha256 match ok: $actual" -ForegroundColor Green
}

function Get-PeMachine([string]$Path) {
    try {
        $fs = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
        try {
            $br = New-Object System.IO.BinaryReader($fs)
            $mz = $br.ReadUInt16()
            if ($mz -ne 0x5A4D) { return $null }
            $fs.Seek(0x3C, [System.IO.SeekOrigin]::Begin) | Out-Null
            $peOffset = $br.ReadInt32()
            if ($peOffset -lt 0) { return $null }
            $fs.Seek($peOffset, [System.IO.SeekOrigin]::Begin) | Out-Null
            $peSig = $br.ReadUInt32()
            if ($peSig -ne 0x00004550) { return $null }
            return $br.ReadUInt16()
        } finally {
            $fs.Close()
        }
    } catch {
        return $null
    }
}

function Assert-ExeCompatible([string]$Path) {
    $machine = Get-PeMachine $Path
    if (-not $machine) {
        Write-Host "error: extracted file is not a valid Windows executable: $Path" -ForegroundColor Red
        exit 1
    }

    $is64 = [Environment]::Is64BitOperatingSystem
    if (-not $is64 -and $machine -eq 0x8664) {
        Write-Host "error: this package contains an x64 executable but your Windows appears to be 32-bit" -ForegroundColor Red
        Write-Host "hint: install a 32-bit build or use a 64-bit Windows" -ForegroundColor Yellow
        exit 1
    }

    $arch = $env:PROCESSOR_ARCHITECTURE
    $arch2 = $env:PROCESSOR_ARCHITEW6432
    $isArm = ($arch -eq 'ARM64' -or $arch2 -eq 'ARM64')
    if (-not $isArm -and $machine -eq 0xAA64) {
        Write-Host "error: this package contains an ARM64 executable but your Windows is not ARM64" -ForegroundColor Red
        Write-Host "hint: install the x64 build" -ForegroundColor Yellow
        exit 1
    }
}

function Write-DownloadProgress([int64]$Done, [int64]$Total) {
    if (-not $script:__gpuf_lastProgressLen) {
        $script:__gpuf_lastProgressLen = 0
    }

    $width = 50
    try {
        $w = $Host.UI.RawUI.WindowSize.Width
        if ($w -gt 40) {
            $width = [math]::Max(10, [math]::Min(70, $w - 40))
        }
    } catch {
    }

    $pct = 0
    if ($Total -gt 0) {
        $pct = [math]::Min(100, [math]::Floor(($Done * 100.0) / $Total))
    }

    $filled = [math]::Floor(($pct * $width) / 100)
    $empty = $width - $filled

    $fillChar = [string][char]0x2588
    $emptyChar = [string][char]0x2591
    $bar = (($fillChar * $filled) + ($emptyChar * $empty))

    $doneMb = [math]::Round($Done / 1MB, 2)
    if ($Total -gt 0) {
        $totalMb = [math]::Round($Total / 1MB, 2)
        $line = "Downloading [$bar] $pct% ($doneMb/$totalMb MB)"
    } else {
        $line = "Downloading [$bar] $doneMb MB"
    }

    $pad = ""
    if ($script:__gpuf_lastProgressLen -gt $line.Length) {
        $pad = (' ' * ($script:__gpuf_lastProgressLen - $line.Length))
    }
    $script:__gpuf_lastProgressLen = $line.Length

    Write-Host -NoNewline ("`r" + $line + $pad)
}

function Complete-DownloadProgress {
    if (-not $script:__gpuf_lastProgressLen) {
        $script:__gpuf_lastProgressLen = 0
    }
    if ($script:__gpuf_lastProgressLen -gt 0) {
        Write-Host ""
    }
    $script:__gpuf_lastProgressLen = 0
}

function Get-RemoteContentLength([string]$Url) {
    try {
        $req = [System.Net.HttpWebRequest]::Create($Url)
        $req.Method = 'HEAD'
        $req.AllowAutoRedirect = $true
        $resp = $req.GetResponse()
        try {
            return [int64]$resp.ContentLength
        } finally {
            try { $resp.Close() } catch { }
        }
    } catch {
        return [int64]-1
    }
}

function Download-FileWithProgress([string]$Url, [string]$OutFile) {
    $req = [System.Net.HttpWebRequest]::Create($Url)
    $req.Method = 'GET'
    $req.AllowAutoRedirect = $true

    $resp = $req.GetResponse()
    try {
        $total = $resp.ContentLength
    } catch {
        $total = -1
    }

    $inStream = $resp.GetResponseStream()
    $outStream = [System.IO.File]::Open($OutFile, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::ReadWrite)

    try {
        $buffer = New-Object byte[] (1024 * 1024)
        $done = [int64]0
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $lastUpdateMs = [int64]0
        while (($read = $inStream.Read($buffer, 0, $buffer.Length)) -gt 0) {
            $outStream.Write($buffer, 0, $read)
            $done += $read

            if (($sw.ElapsedMilliseconds - $lastUpdateMs) -ge 500) {
                Write-DownloadProgress $done $total

                $lastUpdateMs = $sw.ElapsedMilliseconds
            }
        }
    } finally {
        try { $outStream.Close() } catch { }
        try { $inStream.Close() } catch { }
        try { $resp.Close() } catch { }
        Complete-DownloadProgress
    }
}

function Download-FilePreferCurl([string]$Url, [string]$OutFile) {
    $curl = Get-Command curl.exe -ErrorAction SilentlyContinue
    if ($curl) {
        try {
            $total = Get-RemoteContentLength $Url

            if ($total -gt 0 -and (Test-Path $OutFile)) {
                try {
                    $existingLen = (Get-Item $OutFile).Length
                    if ($existingLen -ge $total) {
                        if ($existingLen -gt $total) {
                            Remove-Item -Path $OutFile -Force -ErrorAction SilentlyContinue
                        } else {
                            Write-DownloadProgress $existingLen $total
                            Complete-DownloadProgress
                            return
                        }
                    }
                } catch {
                }
            }

            $args = @(
                '-L',
                '-C', '-',
                '--fail',
                '--retry', '5',
                '--retry-delay', '2',
                '--silent',
                '--show-error',
                '-o', $OutFile,
                $Url
            )

            $p = Start-Process -FilePath $curl.Source -ArgumentList $args -NoNewWindow -PassThru
            while (-not $p.HasExited) {
                $done = [int64]0
                try {
                    if (Test-Path $OutFile) {
                        $done = (Get-Item $OutFile).Length
                    }
                } catch {
                }

                Write-DownloadProgress $done $total
                Start-Sleep -Milliseconds 500
            }

            $done = [int64]0
            try {
                if (Test-Path $OutFile) {
                    $done = (Get-Item $OutFile).Length
                }
            } catch {
            }
            Write-DownloadProgress $done $total
            Complete-DownloadProgress

            if ($total -gt 0 -and $done -ge $total) {
                return
            }

            if ($p.ExitCode -ne 0) {
                throw "curl.exe exited with code $($p.ExitCode)"
            }

            return
        } catch {
            Write-Host "warning: curl.exe download failed, fallback to direct download: $_" -ForegroundColor Yellow
        }
    } else {
        Write-Host "warning: curl.exe not found, fallback to direct download" -ForegroundColor Yellow
    }

    Download-FileWithProgress $Url $OutFile
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
$archivePath = Join-Path $DownloadDir $PackageName

try {
    if (-not (Test-Path $DownloadDir)) {
        New-Item -ItemType Directory -Path $DownloadDir -Force | Out-Null
    }

    try {
        $testPath = Join-Path $DownloadDir (".gpuf_write_test_" + [Guid]::NewGuid().ToString("N"))
        Set-Content -Path $testPath -Value "1" -Encoding Ascii -Force
        Remove-Item -Path $testPath -Force -ErrorAction SilentlyContinue
    } catch {
        Write-Host "error: cannot write to DownloadDir: $DownloadDir" -ForegroundColor Red
        Write-Host "hint: use -DownloadDir C:\\gpuf or a directory you can write to" -ForegroundColor Yellow
        exit 1
    }

    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    $expectedVer = Get-ExpectedVersionFromPackageName $PackageName
    $installedExe = Join-Path $InstallDir "gpuf-c.exe"
    $installedVer = Get-InstalledVersionMarker $InstallDir
    if (-not $installedVer) {
        $installedVer = Get-InstalledGpufVersion $installedExe
    }
    if ($expectedVer -and $installedVer -and (Test-Path $installedExe) -and ((Parse-Version $installedVer) -eq (Parse-Version $expectedVer))) {
        Ensure-InstallDirOnPath $InstallDir
        Write-Host "gpuf-c is already installed and up to date (version $installedVer). Skip download." -ForegroundColor Green
        exit 0
    }

    if (-not $expectedVer) {
        Write-Host "warning: cannot parse expected version from PackageName: $PackageName (will reinstall)" -ForegroundColor Yellow
    } else {
        $markerPath = Join-Path $InstallDir ".gpuf_version"
        if (Test-Path $markerPath) {
            $markerVer = Get-InstalledVersionMarker $InstallDir
            if ((-not (Test-Path $installedExe)) -and $markerVer -and ((Parse-Version $markerVer) -eq (Parse-Version $expectedVer))) {
                Write-Host "warning: version marker indicates up-to-date ($markerVer) but gpuf-c.exe is missing; will reinstall" -ForegroundColor Yellow
            } elseif ($markerVer -and ((Parse-Version $markerVer) -ne (Parse-Version $expectedVer))) {
                Write-Host "warning: installed version marker ($markerVer) != expected ($expectedVer), will reinstall" -ForegroundColor Yellow
            }
        } elseif ($installedVer) {
            Write-Host "warning: detected installed version ($installedVer) but expected ($expectedVer), will reinstall" -ForegroundColor Yellow
        } elseif (Test-Path $installedExe) {
            Write-Host "warning: gpuf-c.exe exists but version cannot be determined (marker missing and --version parse failed), will reinstall" -ForegroundColor Yellow
        }
    }

    Write-Host "Downloading: $pkgUrl" -ForegroundColor Yellow
    Write-Host "DownloadPath: $archivePath" -ForegroundColor Yellow
    if (Test-Path $archivePath) {
        Remove-Item -Path $archivePath -Force -ErrorAction SilentlyContinue
    }
    $tmpArchivePath = "$archivePath.part"
    Download-FilePreferCurl $pkgUrl $tmpArchivePath
    if (Test-Path $archivePath) {
        Remove-Item -Path $archivePath -Force -ErrorAction SilentlyContinue
    }
    try {
        Move-Item -Path $tmpArchivePath -Destination $archivePath -Force
    } catch {
        Write-Host "error: failed to finalize archive move to $archivePath" -ForegroundColor Red
        Write-Host "hint: the destination file may be locked by another process; archive remains at: $tmpArchivePath" -ForegroundColor Yellow
        throw
    }

    $expectedSha = $PackageSha256
    if (-not $expectedSha) {
        $shaFilePath = Join-Path $DownloadDir ($PackageName + ".sha256")
        try {
            Download-FilePreferCurl "$pkgUrl.sha256" $shaFilePath
            $expectedSha = Read-Sha256File $shaFilePath $PackageName
        } catch {
            $sumsPath = Join-Path $DownloadDir "SHA256SUMS"
            try {
                Download-FilePreferCurl "$BaseUrl/SHA256SUMS" $sumsPath
                $expectedSha = Read-Sha256File $sumsPath $PackageName
            } catch {
            }
        }
    }
    Verify-Sha256Required $archivePath $expectedSha

    # Extract (.tar.gz) using tar.exe (available on most Windows 10/11)
    $tar = Get-Command tar -ErrorAction SilentlyContinue
    if (-not $tar) {
        Write-Host "error: tar command not found. Please install tar/bsdtar or use a zip-based package." -ForegroundColor Red
        exit 1
    }

    # Clean up old installation files before extracting new version
    Write-Host "Cleaning old installation files..." -ForegroundColor Yellow
    if (Test-Path $InstallDir) {
        try {
            # Remove all files in InstallDir but keep the directory
            Get-ChildItem -Path $InstallDir -Recurse | Remove-Item -Force -Recurse -ErrorAction SilentlyContinue
            Write-Host "Old files removed" -ForegroundColor Green
        } catch {
            Write-Host "warning: failed to clean some old files: $_" -ForegroundColor Yellow
        }
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

        Assert-ExeCompatible $candidate.FullName

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
    Ensure-InstallDirOnPath $InstallDir

    try {
        $expectedVer = Get-ExpectedVersionFromPackageName $PackageName
        if ($expectedVer) {
            Set-Content -Path (Join-Path $InstallDir ".gpuf_version") -Value $expectedVer -Encoding Ascii -Force
        }
    } catch {
    }

    Remove-Item -Path $archivePath -Force -ErrorAction SilentlyContinue

    Write-Host "gpuf-c (llama.cpp) installed successfully!" -ForegroundColor Green
    Write-Host "InstallDir: $InstallDir" -ForegroundColor Yellow
    Write-Host "Please restart terminal to make PATH changes take effect." -ForegroundColor Yellow

} catch {
    Write-Host "installation failed: $_" -ForegroundColor Red
    exit 1
}
