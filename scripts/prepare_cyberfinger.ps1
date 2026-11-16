#Requires -Version 5.1
<#
.SYNOPSIS
Prepare and build this checkout's CyberFinger mod without touching the game.
.DESCRIPTION
Downloads the checksum-pinned upstream 0.2.45 release for its packed assets and
matching redscript compiler, initializes pinned submodules, configures CMake,
and builds the patched native plugin. All downloaded files stay in this repo.
Requires Git, CMake 3.24+ and Visual Studio 2022 C++ tools with a Windows SDK.
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param([string]$BuildDir = 'build\cyberfinger-release')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..')).TrimEnd('\')

function Local-Path([string]$Relative) {
    $path = if ([IO.Path]::IsPathRooted($Relative)) { [IO.Path]::GetFullPath($Relative) }
            else { [IO.Path]::GetFullPath((Join-Path $repo $Relative)) }
    if (!$path.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) { throw "Build/download path must be inside this checkout: $path" }
    $parent = $path
    while ($parent) {
        if ((Test-Path -LiteralPath $parent) -and
            ((Get-Item -LiteralPath $parent -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Linked build/download paths are not supported: $parent"
        }
        $parent = Split-Path -Parent $parent
    }
    return $path
}
function Hash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
foreach ($tool in @('cmake', 'git')) {
    if (!(Get-Command $tool -ErrorAction SilentlyContinue)) { throw "Install $tool and put it on PATH before preparing the mod." }
}
$BuildDir = Local-Path $BuildDir
$cache = Local-Path 'build\cyberfinger-bootstrap'
$releaseZip = Local-Path 'build\cyberfinger-bootstrap\CyberpunkVRPort-0.2.45.zip'
$releaseRoot = Local-Path 'build\cyberfinger-bootstrap\release-0.2.45'
$sdkRoot = Local-Path 'build\cyberfinger-bootstrap\RED4ext.SDK-game25'
# The repository submodule still targets 2.31. Its own patch-day tooling uses
# wopss/RED4ext.SDK for regenerated game layouts; pin the verified 2.5 revision.
$sdkCommit = 'a34f0ad0a9e4f37cbe1fe001e4ff68c2949d0665'
$sourceManifest = Local-Path 'mods\archive\cyberpunkvrport.archive.xl'
if (!(Test-Path -LiteralPath $sourceManifest -PathType Leaf)) { throw 'The source archive manifest is missing; repair the checkout first.' }
if (!$PSCmdlet.ShouldProcess($repo, "Fetch verified 0.2.45 assets, initialize dependencies, configure and build $BuildDir")) { return }

[void][IO.Directory]::CreateDirectory($cache)
$expectedZip = '64b23fb8f211cb5c3df896676a7d9c8ebf6e70d3affaa4f280ffd39d9cfab6cf'
if (!(Test-Path -LiteralPath $releaseZip)) {
    Write-Host 'Downloading matching upstream 0.2.45 assets (about 147 MiB)...'
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $download = Local-Path 'build\cyberfinger-bootstrap\CyberpunkVRPort-0.2.45.zip.download'
    $client = New-Object Net.WebClient
    try { $client.DownloadFile('https://github.com/dariulone/cyberpunk-vr-port/releases/download/0.2.45/CyberpunkVRPort-0.2.45.zip', $download) }
    finally { $client.Dispose() }
    if ((Hash $download) -ne $expectedZip) { throw "Release checksum mismatch: $download" }
    Move-Item -LiteralPath $download -Destination $releaseZip
}
if ((Hash $releaseZip) -ne $expectedZip) { throw "Cached release checksum mismatch: $releaseZip" }

# Never use the release's unpatched cyberpunkvrport.dll. Extract just data and
# the matching standalone script compiler; the native mod is built below.
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($releaseZip)
$plugin = 'red4ext/plugins/cyberpunkvrport/'
try {
    foreach ($rel in @('archive/cyberpunkvrport.archive', 'config/archive.yaml', 'redscript/scc_lib.dll', 'redscript/LICENSE.txt')) {
        $entry = $zip.GetEntry($plugin + $rel)
        if (!$entry) { throw "Pinned release is missing $rel" }
        $target = Local-Path (Join-Path $releaseRoot ($plugin + $rel))
        [void][IO.Directory]::CreateDirectory((Split-Path -Parent $target))
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
    }
} finally { $zip.Dispose() }
$assetRoot = Local-Path (Join-Path $releaseRoot $plugin)
if ((Hash (Join-Path $assetRoot 'config\archive.yaml')) -ne (Hash $sourceManifest)) {
    throw 'The 0.2.45 archive manifest does not match this checkout. Obtain assets for this source version.'
}
$archive = Local-Path 'mods\archive\cyberpunkvrport.archive'
$releaseArchive = Join-Path $assetRoot 'archive\cyberpunkvrport.archive'
if (!(Test-Path -LiteralPath $archive) -or (Hash $archive) -ne (Hash $releaseArchive)) {
    if (Test-Path -LiteralPath $archive) {
        # A checkout upgraded from 0.2.3 still has its ignored packed archive.
        # Preserve it before replacing it with the verified matching release.
        $archiveBackup = Local-Path ('build\cyberfinger-bootstrap\archive-' + (Hash $archive) + '.archive')
        if (!(Test-Path -LiteralPath $archiveBackup)) { Copy-Item -LiteralPath $archive -Destination $archiveBackup }
    }
    Copy-Item -LiteralPath $releaseArchive -Destination $archive -Force
}

Write-Host 'Initializing pinned source dependencies...'
& git -C $repo submodule update --init --recursive -- externals/RED4ext.SDK externals/minhook externals/wrpuppets-fbt-ik
if ($LASTEXITCODE -ne 0) { throw 'Git submodule initialization failed.' }
if (!(Test-Path -LiteralPath $sdkRoot)) {
    & git clone --filter=blob:none --no-checkout https://github.com/wopss/RED4ext.SDK.git $sdkRoot
    if ($LASTEXITCODE -ne 0) { throw 'Game-2.5 SDK download failed.' }
    & git -C $sdkRoot checkout --detach $sdkCommit
    if ($LASTEXITCODE -ne 0) { throw 'Could not select the pinned game-2.5 SDK.' }
}
$sdkHead = & git -C $sdkRoot rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $sdkHead -ne $sdkCommit) { throw "Unexpected SDK revision in $sdkRoot; expected $sdkCommit." }
$configure = @('-S', $repo, '-B', $BuildDir, '-G', 'Visual Studio 17 2022', '-A', 'x64', '-Wno-dev',
    ('-DCVR_RED4EXT_SDK_DIR=' + $sdkRoot),
    ('-DCVR_REDSCRIPT_COMPILER_DIR=' + (Join-Path $assetRoot 'redscript')))
Write-Host 'Configuring native build (the first run downloads pinned C++ dependencies)...'
& cmake @configure
if ($LASTEXITCODE -ne 0) { throw 'CMake configuration failed. See the first error above; Visual Studio 2022 C++ tools and a Windows SDK are required.' }
Write-Host 'Building the CyberFinger-patched game plugin...'
& cmake --build $BuildDir --config Release --target cyberpunkvrport_stereo --parallel 2
if ($LASTEXITCODE -ne 0) { throw 'Native build failed. No game files have been changed.' }
Write-Host "Prepared release build: $BuildDir"
