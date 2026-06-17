<#
.SYNOPSIS
    maludb installer for Windows — downloads a prebuilt release and installs it.

.DESCRIPTION
    Run with:

      irm https://raw.githubusercontent.com/maludb/maludb-terminal/main/install.ps1 | iex

    or, to pass options, download and run directly:

      ./install.ps1 -Version v0.2.0 -BinDir "C:\tools\maludb"

.PARAMETER Version
    Release tag to install (e.g. v0.2.0). Defaults to the latest release.

.PARAMETER BinDir
    Install location. Defaults to %LOCALAPPDATA%\Programs\maludb\bin.

.PARAMETER Target
    Rust target triple. Defaults to x86_64-pc-windows-msvc.
#>
[CmdletBinding()]
param(
    [string]$Version,
    [string]$BinDir = (Join-Path $env:LOCALAPPDATA "Programs\maludb\bin"),
    [string]$Target = "x86_64-pc-windows-msvc"
)

$ErrorActionPreference = "Stop"
$repo = "maludb/maludb-terminal"
$headers = @{ "User-Agent" = "maludb-install" }

if (-not $Version) {
    Write-Host "Resolving latest release..."
    $release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers $headers
    $Version = $release.tag_name
}

$tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
$vnum = $tag.TrimStart("v")

$archive = "maludb-$vnum-$Target.zip"
$url = "https://github.com/$repo/releases/download/$tag/$archive"

$tmp = New-Item -ItemType Directory -Path (Join-Path $env:TEMP ("maludb-" + [System.IO.Path]::GetRandomFileName()))
try {
    $zip = Join-Path $tmp $archive
    Write-Host "Downloading $archive ..."
    Invoke-WebRequest $url -OutFile $zip -UseBasicParsing
    Invoke-WebRequest "$url.sha256" -OutFile "$zip.sha256" -UseBasicParsing

    $expected = ((Get-Content "$zip.sha256") -split '\s+')[0].ToLower()
    $actual = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
    if ($expected -ne $actual) {
        throw "checksum verification failed (expected $expected, got $actual)"
    }

    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    Copy-Item (Join-Path $tmp "maludb-$vnum-$Target\bin\maludb.exe") (Join-Path $BinDir "maludb.exe") -Force
    Write-Host "Installed maludb $vnum to $BinDir\maludb.exe"

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (($userPath -split ';') -notcontains $BinDir) {
        Write-Host "Adding $BinDir to your user PATH (restart your shell to pick it up)."
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$BinDir", "User")
    }
}
finally {
    Remove-Item -Recurse -Force $tmp
}
