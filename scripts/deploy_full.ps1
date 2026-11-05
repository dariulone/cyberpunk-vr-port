# Deploy a complete build_dist package, preserving player settings and verifying
# every deployed byte. Never starts the game. PowerShell 5.1 compatible.
param(
    [Parameter(Mandatory=$true)][string]$GameRoot,
    [Parameter(Mandatory=$true)][string]$Package,
    [string]$ConfigSeedZip,
    [string]$ConfigSeedManifest,
    [switch]$EnableRoomscale
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$RepoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$GameRoot = (Resolve-Path -LiteralPath $GameRoot).Path.TrimEnd('\')
$Package = (Resolve-Path -LiteralPath $Package).Path.TrimEnd('\')
if (!(Test-Path -LiteralPath (Join-Path $GameRoot 'bin\x64\Cyberpunk2077.exe'))) { throw 'Not a game root' }
if (Get-Process Cyberpunk2077 -ErrorAction SilentlyContinue) { throw 'Close Cyberpunk2077 before deployment' }
$BuildParent = Join-Path $RepoRoot 'build'
if (!(Test-Path -LiteralPath $BuildParent)) { throw 'Missing build parent' }
$Backup = Join-Path $BuildParent ('roomscale-deploy-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
if (Test-Path -LiteralPath $Backup) { throw "Backup exists: $Backup" }
New-Item -ItemType Directory -Path $Backup | Out-Null

function Under-Root([string]$root, [string]$relative) {
    $p = [IO.Path]::GetFullPath((Join-Path $root $relative))
    if (!$p.StartsWith($root.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw "Path outside root: $relative"
    }
    return $p
}
function Ensure-Parent([string]$path) {
    $parent = Split-Path -Parent $path
    if (!(Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
}
function Hash([string]$path) { return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Set-Ini([string]$path, [string]$key, [string]$value) {
    $text = if (Test-Path -LiteralPath $path) { [IO.File]::ReadAllText($path) } else { '' }
    $pattern = '(?m)^' + [regex]::Escape($key) + '\s*=.*$'
    if ([regex]::IsMatch($text, $pattern)) { $text = [regex]::Replace($text, $pattern, "$key=$value") }
    else { $text = $text.TrimEnd() + "`r`n$key=$value`r`n" }
    Ensure-Parent $path
    [IO.File]::WriteAllText($path, $text, (New-Object Text.UTF8Encoding($false)))
}

$files = @(Get-ChildItem -LiteralPath $Package -Recurse -File)
if (!$files.Count) { throw 'Empty package' }
$relativeFiles = @($files | ForEach-Object { $_.FullName.Substring($Package.Length + 1) })
$retiredScripts = @('r6\scripts\CyberpunkVRPort_LootUi\vrport_loot_ui.reds',
    'r6\scripts\CyberpunkVRPort_SettingsGuard\vrport_settings_guard.reds',
    'r6\scripts\CyberpunkVRPort_Cyberware\vrport_launcher_tweak.reds',
    'archive\pc\mod\cyberpunkvrport.archive', 'archive\pc\mod\cyberpunkvrport.archive.xl')
foreach ($rel in $retiredScripts) {
    if ($relativeFiles -contains $rel) { throw "Package contains retired script: $rel" }
}
# The port's scripts are the plugin's now (red4ext\plugins\cyberpunkvrport\scripts): every copy an earlier build put
# in r6\scripts goes, backed up like the rest -- compiled beside the plugin's they would define everything twice.
$oldScripts = Join-Path $GameRoot 'r6\scripts'
if (Test-Path -LiteralPath $oldScripts) {
    $retiredScripts += @(Get-ChildItem -LiteralPath $oldScripts -Directory -Filter 'CyberpunkVRPort_*' |
        ForEach-Object { Get-ChildItem -LiteralPath $_.FullName -Recurse -File } |
        ForEach-Object { $_.FullName.Substring($GameRoot.Length + 1) })
}
$dllRel = 'red4ext\plugins\cyberpunkvrport\cyberpunkvrport.dll'
if ($relativeFiles -notcontains $dllRel) { throw 'Package lacks plugin DLL' }
if (!(@($relativeFiles | Where-Object { $_ -like 'red4ext\plugins\cyberpunkvrport\config\tweaks\vrport\*' }).Count)) {
    throw 'Full package lacks vrport tweaks'
}

$runtime = @('red4ext\plugins\cyberpunkvrport\config\vrport.ini', 'red4ext\plugins\cyberpunkvrport\config\vrport-launcher.ini',
    'red4ext\plugins\cyberpunkvrport\config\vrik_calibration.ini')
$stereoConfig = 'bin\x64\plugins\cyber_engine_tweaks\mods\CyberpunkVRPort_Stereo\vrcam.json'
$backedUp = @()
# Preserve the just-tested run before the next launch replaces its log.
$diagnostics = @('red4ext\plugins\cyberpunkvrport\log\cyberpunkvrport.log')
foreach ($rel in @($relativeFiles + $runtime + $diagnostics + $retiredScripts | Sort-Object -Unique)) {
    $existing = Under-Root $GameRoot $rel
    if (Test-Path -LiteralPath $existing -PathType Leaf) {
        $dest = Under-Root $Backup ('before\' + $rel)
        Ensure-Parent $dest
        Copy-Item -LiteralPath $existing -Destination $dest
        if ((Hash $existing) -ne (Hash $dest)) { throw "Backup mismatch: $rel" }
        $backedUp += [pscustomobject]@{ Path=$rel; SHA256=(Hash $dest) }
    }
}

# Restore only missing runtime preferences from the pre-RE snapshot, never its
# old DLL/scripts. The manifest verifies the archive and each recovered entry.
$restored = @()
if ($ConfigSeedZip) {
    if (!$ConfigSeedManifest) { throw 'Config seed requires its manifest' }
    $seed = Get-Content -LiteralPath $ConfigSeedManifest -Raw | ConvertFrom-Json
    if ((Hash $ConfigSeedZip) -ne $seed.zip_sha256) { throw 'Config seed ZIP hash mismatch' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead((Resolve-Path -LiteralPath $ConfigSeedZip).Path)
    try {
        foreach ($rel in $runtime) {
            $target = Under-Root $GameRoot $rel
            if (Test-Path -LiteralPath $target) { continue }
            $name = $rel.Replace('\', '/')
            $entry = $archive.GetEntry($name)
            if (!$entry) { continue }
            $record = $seed.files.PSObject.Properties[$name]
            if (!$record) { throw "No manifest entry for $name" }
            $staged = Under-Root $Backup ('restored-config\' + $rel)
            Ensure-Parent $staged
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $staged)
            if ((Hash $staged) -ne $record.Value.sha256) { throw "Seed entry hash mismatch: $rel" }
            Ensure-Parent $target
            Copy-Item -LiteralPath $staged -Destination $target
            $restored += $rel
        }
    } finally { $archive.Dispose() }
}

# Preserve the installed resolution selection while refreshing the catalogue.
$pickComponent = $null; $pickCamera = $null
$oldStereo = Under-Root $GameRoot $stereoConfig
if (Test-Path -LiteralPath $oldStereo) {
    $old = Get-Content -LiteralPath $oldStereo -Raw
    if ($old -match '"component"\s*:\s*"([^"]+)"') { $pickComponent = $Matches[1] }
    if ($old -match '"virtualCamera"\s*:\s*"([^"]+)"') { $pickCamera = $Matches[1] }
}

# Remove duplicate VR loaders from the loader search path by parking them in
# the verified backup, rather than leaving a renamed .dll beside the new one.
$parked = @()
$strays = @()
$dxgi = Join-Path $GameRoot 'bin\x64\dxgi.dll'
if (Test-Path -LiteralPath $dxgi) { $strays += Get-Item -LiteralPath $dxgi }
foreach ($dir in @('CyberpunkVR_Hands', 'CyberpunkVR_Stereo')) {
    $path = Join-Path $GameRoot "red4ext\plugins\$dir"
    if (!(Test-Path -LiteralPath $path)) { continue }
    $strays += @(Get-ChildItem -LiteralPath $path -Recurse -File -Filter '*.dll' |
        Where-Object { $dir -eq 'CyberpunkVR_Hands' -or $_.FullName -ne (Join-Path $GameRoot $dllRel) })
}
foreach ($file in $strays) {
    $rel = $file.FullName.Substring($GameRoot.Length + 1)
    $dest = Under-Root $Backup ('disabled-loaders\' + $rel)
    Ensure-Parent $dest
    $sha = Hash $file.FullName
    Move-Item -LiteralPath $file.FullName -Destination $dest
    if ((Hash $dest) -ne $sha) { throw "Parked loader mismatch: $rel" }
    $parked += $rel
}

$deployed = @(); $preserved = @()
foreach ($rel in $relativeFiles) {
    $source = Under-Root $Package $rel
    $target = Under-Root $GameRoot $rel
    if ($rel -eq $hudLayout -and (Test-Path -LiteralPath $target)) {
        $preserved += [pscustomobject]@{ Path=$rel; SHA256=(Hash $target) }
        continue
    }
    Ensure-Parent $target
    Copy-Item -LiteralPath $source -Destination $target -Force
    if ((Hash $source) -ne (Hash $target)) { throw "Deployment mismatch: $rel" }
    $deployed += [pscustomobject]@{ Path=$rel; Bytes=(Get-Item -LiteralPath $target).Length; SHA256=(Hash $target) }
}

$removedScripts = @()
foreach ($rel in $retiredScripts) {
    $target = Under-Root $GameRoot $rel
    if (Test-Path -LiteralPath $target -PathType Leaf) {
        $saved = Under-Root $Backup ('before\' + $rel)
        if (!(Test-Path -LiteralPath $saved) -or (Hash $saved) -ne (Hash $target)) {
            throw "Retired script backup mismatch: $rel"
        }
        Remove-Item -LiteralPath $target
        $removedScripts += $rel
    }
}

if ($pickComponent) {
    $text = [IO.File]::ReadAllText($oldStereo)
    if ($text -match [regex]::Escape('"' + $pickComponent + '"')) {
        # Use a MatchEvaluator to avoid replacement-string interpretation of names.
        $text = [regex]::Replace($text, '("component"\s*:\s*")[^"]+(")',
            [Text.RegularExpressions.MatchEvaluator]{ param($m) $m.Groups[1].Value + $pickComponent + $m.Groups[2].Value })
        if ($pickCamera) {
            $text = [regex]::Replace($text, '("virtualCamera"\s*:\s*")[^"]+(")',
                [Text.RegularExpressions.MatchEvaluator]{ param($m) $m.Groups[1].Value + $pickCamera + $m.Groups[2].Value })
        }
        [IO.File]::WriteAllText($oldStereo, $text, (New-Object Text.UTF8Encoding($false)))
        ($deployed | Where-Object Path -eq $stereoConfig).SHA256 = Hash $oldStereo
    }
}

$ini = Under-Root $GameRoot 'bin\x64\vrport.ini'
Set-Ini $ini 'first_launch' '0'
if ($EnableRoomscale) {
    Set-Ini $ini 'xr_roomscale_movement' '1'
    Set-Ini $ini 'xr_physical_body_rotation' '1'
    Set-Ini $ini 'xr_snap_turn_yaw_index' '0'
}
$report = [pscustomobject]@{
    GameRoot=$GameRoot; Package=$Package; Backup=$Backup
    SourceCommit=(git -C $RepoRoot rev-parse HEAD)
    SourceDirty=[bool](git -C $RepoRoot status --porcelain -- src include scripts)
    DLL_SHA256=(Hash (Join-Path $GameRoot $dllRel))
    DeployedCount=$deployed.Count; PreservedCount=$preserved.Count
    BackedUp=$backedUp; RestoredRuntimeConfig=$restored; DisabledLoaders=$parked
    RemovedScripts=$removedScripts
    Files=$deployed; Preserved=$preserved; IniSHA256=(Hash $ini)
    GameStarted=$false
}
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $Backup 'deployment.json') -Encoding UTF8
# Final re-read, after all merges and settings updates.
foreach ($entry in @($deployed + $preserved)) {
    if ((Hash (Under-Root $GameRoot $entry.Path)) -ne $entry.SHA256) { throw "Final verification failed: $($entry.Path)" }
}
[pscustomobject]@{ Deployed=$deployed.Count; Preserved=$preserved.Count; DLL_SHA256=$report.DLL_SHA256;
    RestoredConfig=$restored; Backup=$Backup; GameStarted=$false } | ConvertTo-Json -Depth 5
