<#
.SYNOPSIS
Packs an already-built Windows release binary into dist\Tenmoku-<version>-windows-<arch>.zip
with its .sha256.

.DESCRIPTION
It does not run cargo: build first with 'cargo build --release -p fastedit'. The zip holds one
folder, Tenmoku-<version>-windows-<arch>\, with tenmoku.exe (its icon is embedded by
crates/fastedit/build.rs) and Licenses\, the licence texts the macOS bundle ships, unmodified,
in the layout scripts/package-linux.sh uses. The .sha256 is "<hash>  <zip name>", as shasum
writes it. Prints the zip's path last and stops on any failure.

.PARAMETER Arch
The architecture in the names: x64 or arm64.

.PARAMETER Binary
The tenmoku.exe to pack (default: target\release\tenmoku.exe, under CARGO_TARGET_DIR when set).

.PARAMETER Version
The version in the names (default: Cargo.toml's [workspace.package] version).

.PARAMETER Out
Where the zip goes (default: dist).
#>
param(
    [Parameter(Mandatory)][ValidateSet('x64', 'arm64')][string]$Arch,
    [string]$Binary = '',
    [string]$Version = '',
    [string]$Out = 'dist'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = Split-Path -Parent $PSScriptRoot
function Die([string]$message) { throw "package-windows: $message" }

if (-not $Binary) {
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    $Binary = Join-Path $target 'release\tenmoku.exe'
}
if (-not $Version) {
    $inPackage = $false
    foreach ($line in Get-Content (Join-Path $root 'Cargo.toml')) {
        if ($line -match '^\[') { $inPackage = ($line -eq '[workspace.package]') }
        elseif ($inPackage -and $line -match '^version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
    }
    if (-not $Version) { Die 'no [workspace.package] version in Cargo.toml' }
}
if ($Version -notmatch '^[0-9]+(\.[0-9]+)*$') { Die "version must be dotted numbers, got '$Version'" }
if (-not (Test-Path -PathType Leaf $Binary)) { Die "no binary at $Binary (run cargo build --release -p fastedit)" }

$name = "Tenmoku-$Version-windows-$Arch"
$work = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
$dir = Join-Path $work $name
New-Item -ItemType Directory -Force $dir | Out-Null
try {
    Copy-Item $Binary (Join-Path $dir 'tenmoku.exe')

    function Ship([string]$source, [string]$as) {
        if (-not (Test-Path -PathType Leaf $source)) { Die "licence missing: $source" }
        $to = Join-Path $dir "Licenses\$as"
        New-Item -ItemType Directory -Force (Split-Path -Parent $to) | Out-Null
        Copy-Item $source $to
    }
    $fonts = @(Get-ChildItem (Join-Path $root 'crates\fe-fonts\fonts') -Directory |
        Where-Object { Test-Path (Join-Path $_.FullName 'OFL.txt') })
    if ($fonts.Count -eq 0) { Die "no OFL.txt under crates\fe-fonts\fonts: the fonts' licences must ship with them" }
    foreach ($family in $fonts) { Ship (Join-Path $family.FullName 'OFL.txt') "fonts\$($family.Name)\OFL.txt" }
    Ship (Join-Path $root 'LICENSE') 'LICENSE'
    Ship (Join-Path $root 'NOTICE') 'NOTICE'
    Ship (Join-Path $root 'LICENSES\Apache-2.0.txt') 'LICENSES\Apache-2.0.txt'
    Ship (Join-Path $root 'LICENSES\MIT.txt') 'LICENSES\MIT.txt'
    Ship (Join-Path $root 'THIRD_PARTY.md') 'THIRD_PARTY.md'
    Ship (Join-Path $root 'kiln\crates\kiln-ui\assets\icons\LICENSE') 'icons-LICENSE.txt'
    Ship (Join-Path $root 'kiln\crates\kiln-ui\assets\file-icons\LICENSE') 'file-icons-LICENSE.txt'
    Ship (Join-Path $root 'vendor\tree-sitter\LICENSE') 'tree-sitter-LICENSE.txt'
    foreach ($grammar in Get-ChildItem (Join-Path $root 'vendor') -Directory -Filter 'tree-sitter-*') {
        Ship (Join-Path $grammar.FullName 'LICENSE') "vendor-$($grammar.Name)-LICENSE.txt"
    }
    foreach ($grammar in Get-ChildItem (Join-Path $root 'extensions\grammars') -Directory) {
        Ship (Join-Path $grammar.FullName 'LICENSE') "grammar-$($grammar.Name)-LICENSE.txt"
        foreach ($extra in Get-ChildItem $grammar.FullName -File |
            Where-Object { $_.Name -like 'LICENSE-*' -or $_.Name -eq 'NOTICE' }) {
            Ship $extra.FullName "grammar-$($grammar.Name)-$($extra.Name).txt"
        }
    }

    New-Item -ItemType Directory -Force $Out | Out-Null
    $zip = Join-Path (Resolve-Path $Out) "$name.zip"
    Remove-Item -Force -ErrorAction SilentlyContinue $zip
    # .NET's zip writer, not Compress-Archive: some versions of that store paths with
    # backslashes, which unzip elsewhere takes as part of the file name.
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::CreateFromDirectory($dir, $zip, [IO.Compression.CompressionLevel]::Optimal, $true)
    $hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
    # LF and no BOM, so `shasum -c` and `sha256sum -c` read it on any system.
    [IO.File]::WriteAllText("$zip.sha256", "$hash  $name.zip`n")
    Write-Output $zip
}
finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $work
}
