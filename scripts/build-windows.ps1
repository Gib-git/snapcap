# Builds SnapCap.exe and a zip in dist\.
#
# Requirements (build machine only): Rust (MSVC toolchain), Visual Studio Build Tools
# with the C++ workload, and NASM on PATH (`winget install nasm.nasm`) so the H.264
# encoder gets its SIMD code. The C runtime is linked statically (.cargo/config.toml),
# so SnapCap.exe runs on a clean Windows 10/11 without any redistributables.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

if (-not (Get-Command nasm -ErrorAction SilentlyContinue)) {
    Write-Warning "nasm not found: the H.264 encoder will be built without SIMD and run much slower."
}

cargo build --release --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x64" }
New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item target\release\snapcap.exe dist\SnapCap.exe -Force
Compress-Archive -Force -Path dist\SnapCap.exe, LICENSE, LICENSE-EXCEPTION -DestinationPath "dist\SnapCap-$version-windows-$arch.zip"
Get-ChildItem dist | Format-Table Name, Length
