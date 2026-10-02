#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$GameRoot = 'D:\SteamLibrary\steamapps\common\Cyberpunk 2077',
    [string]$BridgeRoot = (Join-Path $env:USERPROFILE 'Downloads\PSVR2Toolkit-DSX-Bridge'),
    [ValidateRange(1,65535)][int]$Port = 6969,
    [ValidateRange(0,3)][double]$AudioHapticsGain = 1.35,
    [switch]$CheckOnly
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
try {
    $exe = Join-Path $BridgeRoot 'psvr2_toolkit_dsx_bridge_017.exe'
    $mod = Join-Path $GameRoot 'bin\x64\plugins\cyber_engine_tweaks\mods\DualSense Support\config'
    $config = Join-Path $mod 'DualSenseXConfig.txt'
    $settings = Join-Path $mod 'settings.json'
    foreach ($file in @($exe,$config,$settings,(Join-Path $GameRoot 'bin\x64\Cyberpunk2077.exe'))) {
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Required file missing: $file" }
    }
    $json = Get-Content -LiteralPath $settings -Raw | ConvertFrom-Json
    if ($json.PSObject.Properties.Name -notcontains 'UDPautostart' -or $json.UDPautostart -ne $false) {
        throw 'Disable Enhanced DualSense Support UDP autostart in its settings first. This launcher will not rewrite third-party settings.'
    }
    $native = Join-Path $GameRoot 'red4ext\plugins\DualSenseSupport\DualSense Support.dll'
    if (Test-Path -LiteralPath $native) { throw 'Disable the unnecessary Enhanced DualSense Support native launcher DLL first; keep its CET mod. Do not patch the DLL runtime version.' }
    if (Get-Process DSX,UDPClient,psvr2_toolkit_dsx_bridge,psvr2_toolkit_dsx_bridge_017 -ErrorAction SilentlyContinue) {
        throw 'Another DSX/UDP client or bridge is running. Close it first; only one Sense actuator owner is allowed.'
    }
    Write-Host "Bridge: $exe"
    Write-Host "Gameplay profiles: $config"
    Write-Host 'Mode: original fork weapon/audio engine + melee pulses at gain 1.0.'
    Write-Host 'Motion uses isolated CyberpunkVR_PSVR2_Haptics_017_v1 IPC; upstream input slots are untouched.'
    if ($CheckOnly) { Write-Host 'Preflight passed; bridge not started, no settings changed.'; exit 0 }
    if (-not (Get-Process vrserver -ErrorAction SilentlyContinue)) { throw 'Start SteamVR before the bridge.' }
    $listeners = @(Get-NetUDPEndpoint -LocalPort $Port -ErrorAction SilentlyContinue)
    if ($listeners.Count) { throw "UDP port $Port is already in use; stop the other controller-effect client first." }
    $gain = $AudioHapticsGain.ToString([Globalization.CultureInfo]::InvariantCulture)
    & $exe --port $Port --cyberpunk-config $config --audio-haptics-gain $gain --vr-motion-gain 1.0
    exit $LASTEXITCODE
} catch { Write-Host "STOPPED: $($_.Exception.Message)" -ForegroundColor Red; exit 1 }
