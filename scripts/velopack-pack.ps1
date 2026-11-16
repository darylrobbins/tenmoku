<#
.SYNOPSIS
Packs a built Windows release as a Velopack installer and feed (T1909):
Tenmoku-<version>-windows-<arch>-Setup.exe with its .sha256, releases.windows-<arch>-stable.json
and the full .nupkg it lists.

.DESCRIPTION
It does not run cargo. PackDir is the unpacked Tenmoku-<version>-windows-<arch> folder that
scripts/package-windows.ps1 zips: its tenmoku.exe (built with TENMOKU_UPDATE_FEED, and
TENMOKU_UPDATE_PUBLIC_KEY when set, baked in) and Licenses\ are packed. vpk packs them on the
channel windows-<arch>-stable with the pack id Tenmoku. Setup.exe installs per user into
%LocalAppData%\Tenmoku with no administrator prompt, adds a Start menu shortcut and an
Add/Remove Programs entry that uninstalls it. vpk's portable zip is not made: the release
already has package-windows.ps1's zip.

The feed is signed when TENMOKU_UPDATE_PRIVATE_KEY and TENMOKU_UPDATE_PUBLIC_KEY are both set:
-Signer (sign_update_feed.exe) gets the key on stdin and writes the feed's .sig, and its
public half must be TENMOKU_UPDATE_PUBLIC_KEY. Without a private key the feed is left unsigned,
which the app refuses; a private key without the public one is refused. Setup.exe is not
Authenticode signed. Prints each file it leaves in Out and stops on any failure.

.PARAMETER Arch
x64 or arm64.

.PARAMETER Version
The release's version, dotted numbers.

.PARAMETER PackDir
The unpacked portable folder.

.PARAMETER Out
Where the release files go.

.PARAMETER Signer
sign_update_feed.exe, needed only to sign.

.PARAMETER DryRun
Print the vpk command and run nothing.
#>
param(
    [Parameter(Mandatory)][ValidateSet('x64', 'arm64')][string]$Arch,
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$PackDir,
    [Parameter(Mandatory)][string]$Out,
    [string]$Signer = '',
    [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = Split-Path -Parent $PSScriptRoot
function Die([string]$message) { throw "velopack-pack: $message" }

if ($Version -notmatch '^[0-9]+(\.[0-9]+)*$') { Die "version must be dotted numbers, got '$Version'" }
if (-not (Test-Path -PathType Leaf (Join-Path $PackDir 'tenmoku.exe'))) { Die "no tenmoku.exe in $PackDir" }
if (-not (Test-Path -PathType Container (Join-Path $PackDir 'Licenses'))) { Die "no Licenses\ in $PackDir" }

$channel = "windows-$Arch-stable"
$feedFile = "releases.$channel.json"
$private = [string]$env:TENMOKU_UPDATE_PRIVATE_KEY
$public = [string]$env:TENMOKU_UPDATE_PUBLIC_KEY
# The key reaches the signer only: out of the environment before vpk runs.
Remove-Item Env:TENMOKU_UPDATE_PRIVATE_KEY -ErrorAction SilentlyContinue
if ($public -and $public -notmatch '^[A-Za-z0-9+/=]+$') { Die 'TENMOKU_UPDATE_PUBLIC_KEY is not base64' }
if ($private -and -not $public) { Die 'a private key but no TENMOKU_UPDATE_PUBLIC_KEY: the app would have no key to check the feed with' }
if ($private -and -not $Signer) { Die '-Signer is required to sign the feed' }

$pin = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^velopack = \{ version = "=([^"]+)"').Matches |
    Select-Object -First 1 | ForEach-Object { $_.Groups[1].Value }
if (-not $pin) { Die "no pinned velopack version in Cargo.toml" }
$vpk = if ($env:VPK) { $env:VPK } else { (Get-Command vpk -ErrorAction SilentlyContinue).Source }
if (-not $vpk) { Die "no vpk in PATH; install it with: dotnet tool install vpk --version $pin --tool-path <dir>" }

$work = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
$vpkArgs = @('pack', '--packId', 'Tenmoku', '--packVersion', $Version, '--packDir', (Join-Path $work 'app'),
    '--mainExe', 'tenmoku.exe', '--channel', $channel, '--runtime', "win-$Arch", '--packTitle', 'Tenmoku',
    '--packAuthors', 'Tenmoku', '--icon', (Join-Path $root 'crates\fastedit\assets\windows\tenmoku.ico'),
    '--shortcuts', 'StartMenuRoot', '--noPortable', '--delta', 'None', '--outputDir', (Join-Path $work 'vpk'))
if ($DryRun) {
    Write-Output "$vpk $($vpkArgs -join ' ')"
    exit 0
}

# Not $version: PowerShell names are case-insensitive, and that is $Version.
$vpkVersion = (& $vpk -h 2>&1 | Out-String) -replace '(?s).*Velopack CLI (\S+).*', '$1'
if ($vpkVersion -ne $pin) { Die "vpk is '$vpkVersion', not $pin as the velopack crate is pinned" }

New-Item -ItemType Directory -Force (Join-Path $work 'app'), $Out | Out-Null
try {
    Copy-Item (Join-Path $PackDir 'tenmoku.exe') (Join-Path $work 'app\tenmoku.exe')
    Copy-Item -Recurse (Join-Path $PackDir 'Licenses') (Join-Path $work 'app\Licenses')
    & $vpk @vpkArgs
    if ($LASTEXITCODE -ne 0) { Die "vpk pack exited $LASTEXITCODE" }

    $setups = @(Get-ChildItem (Join-Path $work 'vpk') -Filter '*-Setup.exe')
    if ($setups.Count -ne 1) { Die "vpk did not write one Setup.exe: $($setups.Name -join ', ')" }
    $nupkg = "Tenmoku-$Version-$channel-full.nupkg"
    foreach ($f in $feedFile, $nupkg) {
        if (-not (Test-Path -PathType Leaf (Join-Path $work "vpk\$f"))) { Die "vpk wrote no $f" }
    }
    $name = "Tenmoku-$Version-windows-$Arch-Setup.exe"
    $setup = Join-Path (Resolve-Path $Out) $name
    Move-Item -Force $setups[0].FullName $setup
    $hash = (Get-FileHash -Algorithm SHA256 $setup).Hash.ToLowerInvariant()
    # LF and no BOM, as package-windows.ps1 writes its .sha256.
    [IO.File]::WriteAllText("$setup.sha256", "$hash  $name`n")
    Copy-Item (Join-Path $work "vpk\$feedFile"), (Join-Path $work "vpk\$nupkg") $Out
    $feed = Join-Path (Resolve-Path $Out) $feedFile
    Remove-Item -Force -ErrorAction SilentlyContinue "$feed.sig"
    if ($private) {
        if (-not (Test-Path -PathType Leaf $Signer)) { Die "no signer at $Signer" }
        # On stdin only: an argument would show in the process list.
        $signed = ($private | & $Signer sign $feed | Out-String).Trim()
        if ($LASTEXITCODE -ne 0) { Die "signing $feedFile failed" }
        if ($signed -ne $public) {
            Remove-Item -Force -ErrorAction SilentlyContinue "$feed.sig"
            Die "the private key's public half is not TENMOKU_UPDATE_PUBLIC_KEY; no signature written"
        }
    }
    elseif ($public) {
        Write-Warning "no private key: $feedFile is unsigned, and this build's key refuses it"
    }
    else {
        Write-Output "no private key or public key: $feedFile is unsigned, and the app has no key to take it with"
    }
    Write-Output $setup "$setup.sha256" $feed (Join-Path (Resolve-Path $Out) $nupkg)
    if (Test-Path "$feed.sig") { Write-Output "$feed.sig" }
}
finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $work
}
