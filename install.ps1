# fufu installer for Windows (PowerShell 5+):
#   irm https://raw.githubusercontent.com/tyler-johnson/fufu/main/install.ps1 | iex
#
# Downloads the latest release binary, verifies its sha256 against the
# release's checksums.txt, installs to %LOCALAPPDATA%\Programs\ff, and
# adds that directory to your user PATH. Pin a version by setting
# $env:FF_VERSION (e.g. v0.1.0) first.
$ErrorActionPreference = 'Stop'

$repo = 'tyler-johnson/fufu'
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\ff'

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'amd64' }
    'ARM64' { 'arm64' }
    default { throw "fufu installer: unsupported architecture $env:PROCESSOR_ARCHITECTURE" }
}

$version = $env:FF_VERSION
if (-not $version) {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest" -Headers @{ 'User-Agent' = 'fufu-installer' }
    $version = $release.tag_name
}
if (-not $version) { throw 'fufu installer: could not determine the latest release' }

$archive = "ff_$($version.TrimStart('v'))_windows_$arch.zip"
$base = "https://github.com/$repo/releases/download/$version"

$tmp = Join-Path $env:TEMP "fufu-install-$PID"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
    Write-Host "downloading fufu $version (windows/$arch)..."
    Invoke-WebRequest -Uri "$base/$archive" -OutFile (Join-Path $tmp $archive)
    Invoke-WebRequest -Uri "$base/checksums.txt" -OutFile (Join-Path $tmp 'checksums.txt')

    $line = Get-Content (Join-Path $tmp 'checksums.txt') | Where-Object { $_ -match [regex]::Escape($archive) }
    if (-not $line) { throw "fufu installer: checksums.txt has no entry for $archive" }
    $want = ($line -split '\s+')[0]
    $got = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $archive)).Hash
    if ($got -ne $want) { throw "fufu installer: checksum mismatch for $archive - refusing to install" }

    Expand-Archive -Path (Join-Path $tmp $archive) -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    Copy-Item (Join-Path $tmp 'ff.exe') (Join-Path $installDir 'ff.exe') -Force
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

Write-Host "installed fufu $version to $installDir\ff.exe"

# Put the install directory on the user PATH so new terminals find ff.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $installDir) {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$installDir", 'User')
    Write-Host "added $installDir to your user PATH - open a new terminal to pick it up"
}
if (($env:Path -split ';') -notcontains $installDir) {
    $env:Path = "$env:Path;$installDir"
}

Write-Host ''
Write-Host 'next steps:'
Write-Host "  ff hook shell install          # alias git='ff git' in your shell rc"
Write-Host "  ff hook agent install claude   # capture around Claude Code's tool actions"
