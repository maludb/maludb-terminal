<#
.SYNOPSIS
    Build and package a Windows maludb release zip.

.DESCRIPTION
    Windows counterpart to scripts/package-release.sh. Produces
    dist/maludb-<version>-<target>.zip plus a matching .sha256 file. The zip
    contains bin/maludb.exe, README.md, an optional LICENSE, and a local
    install.ps1 that copies the binary onto PATH.

.PARAMETER Target
    Rust target triple. Defaults to the local rustc host.

.PARAMETER Version
    Release version. Defaults to the Cargo.toml package version.

.PARAMETER DistDir
    Output directory. Defaults to dist.

.PARAMETER Binary
    Binary path to package. Defaults to target/<target>/release/maludb.exe.

.PARAMETER SkipBuild
    Skip cargo build and package the existing -Binary instead.
#>
[CmdletBinding()]
param(
    [string]$Target,
    [string]$Version,
    [string]$DistDir = "dist",
    [string]$Binary,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

function Get-HostTarget {
    $line = (& rustc -vV) | Where-Object { $_ -like "host:*" }
    return ($line -replace "^host:\s*", "").Trim()
}

function Get-CargoVersion {
    foreach ($line in Get-Content "Cargo.toml") {
        if ($line -match '^version\s*=\s*"(.*)"') {
            return $Matches[1]
        }
    }
    throw "could not determine package version from Cargo.toml"
}

if (-not $Target) { $Target = Get-HostTarget }
if (-not $Version) { $Version = Get-CargoVersion }

if (-not $Target) { throw "could not determine Rust target" }
if (-not $Version) { throw "could not determine package version" }

if (-not $SkipBuild) {
    & cargo build --release --target $Target
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

if (-not $Binary) { $Binary = "target/$Target/release/maludb.exe" }
if (-not (Test-Path $Binary)) {
    throw "release binary not found: $Binary"
}

$package = "maludb-$Version-$Target"
$workDir = Join-Path ([System.IO.Path]::GetTempPath()) ("maludb-pkg-" + [System.IO.Path]::GetRandomFileName())
$bundleDir = Join-Path $workDir $package
$binDir = Join-Path $bundleDir "bin"

New-Item -ItemType Directory -Force -Path $binDir | Out-Null
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

Copy-Item $Binary (Join-Path $binDir "maludb.exe")
Copy-Item "README.md" (Join-Path $bundleDir "README.md")
if (Test-Path "LICENSE") {
    Copy-Item "LICENSE" (Join-Path $bundleDir "LICENSE")
}

$installScript = @'
#Requires -Version 5
# Local installer: copy maludb.exe onto PATH.
$ErrorActionPreference = "Stop"
$dest = if ($env:MALUDB_BIN_DIR) { $env:MALUDB_BIN_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\maludb\bin" }
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Copy-Item (Join-Path $PSScriptRoot "bin\maludb.exe") (Join-Path $dest "maludb.exe") -Force
Write-Host "Installed maludb to $dest\maludb.exe"
Write-Host "Add $dest to your PATH if it is not already present."
'@
Set-Content -Path (Join-Path $bundleDir "install.ps1") -Value $installScript -Encoding UTF8

$archive = Join-Path $DistDir "$package.zip"
if (Test-Path $archive) { Remove-Item $archive }
Compress-Archive -Path $bundleDir -DestinationPath $archive

$hash = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLower()
$checksumLine = "$hash  $package.zip"
Set-Content -Path "$archive.sha256" -Value $checksumLine -Encoding ASCII -NoNewline

Remove-Item -Recurse -Force $workDir

Write-Host "Wrote $archive"
Write-Host "Wrote $archive.sha256"
