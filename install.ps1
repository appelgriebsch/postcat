<#
.SYNOPSIS
    postcat installer for Windows.

.DESCRIPTION
    Downloads the latest (or a specific) postcat release from GitHub and
    installs the binary.

.PARAMETER Version
    Install a specific version, e.g. "0.3.0" (default: latest).

.PARAMETER InstallDir
    Where to put the binary (default: $Home\.local\bin).

.EXAMPLE
    irm https://raw.githubusercontent.com/egoist/postcat/main/install.ps1 | iex

.EXAMPLE
    $env:POSTCAT_VERSION = "0.3.0"
    irm https://raw.githubusercontent.com/egoist/postcat/main/install.ps1 | iex
#>

param(
    [string]$Version = $env:POSTCAT_VERSION,
    [string]$InstallDir = $env:POSTCAT_INSTALL_DIR
)

$ErrorActionPreference = "Stop"

$Repo = "egoist/postcat"
$BinName = "postcat.exe"

function Info($msg) {
    Write-Host "==> " -ForegroundColor Blue -NoNewline
    Write-Host $msg
}

function Fail($msg) {
    Write-Host "error: " -ForegroundColor Red -NoNewline
    Write-Host $msg
    exit 1
}

$arch = $env:PROCESSOR_ARCHITECTURE
switch ($arch) {
    "AMD64" { $target = "x86_64-pc-windows-msvc" }
    default { Fail "unsupported architecture: $arch" }
}
Info "Detected target: $target"

if ([string]::IsNullOrEmpty($Version)) {
    Info "Looking up latest release..."
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    $Version = $release.tag_name -replace '^v', ''
    if ([string]::IsNullOrEmpty($Version)) {
        Fail "could not determine latest version"
    }
}
Info "Installing postcat $Version"

$Asset = "postcat-$Version-$target.zip"
$Url = "https://github.com/$Repo/releases/download/v$Version/$Asset"

$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $TmpDir | Out-Null

try {
    $ArchivePath = Join-Path $TmpDir $Asset
    Info "Downloading $Url"
    try {
        Invoke-WebRequest -Uri $Url -OutFile $ArchivePath -UseBasicParsing
    } catch {
        Fail "download failed - does version $Version exist for $target?"
    }

    Info "Extracting..."
    Expand-Archive -Path $ArchivePath -DestinationPath $TmpDir -Force

    if ([string]::IsNullOrEmpty($InstallDir)) {
        $InstallDir = Join-Path $Home ".local\bin"
    }
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

    $Source = Join-Path $TmpDir $BinName
    $Destination = Join-Path $InstallDir $BinName
    Copy-Item -Path $Source -Destination $Destination -Force

    Info "Installed postcat to $Destination"

    $pathEntries = $env:Path -split ";"
    if ($pathEntries -notcontains $InstallDir) {
        Write-Host ""
        Write-Host "warning: " -ForegroundColor Yellow -NoNewline
        Write-Host "$InstallDir is not on your PATH."
        Write-Host ""
        Write-Host "Add it for future sessions with:"
        Write-Host ""
        Write-Host "  [Environment]::SetEnvironmentVariable('Path', `"`$env:Path;$InstallDir`", 'User')"
        Write-Host ""
        Write-Host "Or just for this session with:"
        Write-Host ""
        Write-Host "  `$env:Path += `";$InstallDir`""
        Write-Host ""
    }

    Info "Run 'postcat' to get started."
} finally {
    Remove-Item -Path $TmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
