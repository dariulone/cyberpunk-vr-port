# Runs the production deployer against small fake packages/game roots only.
# Leaves its fixture and production backup reports for inspection; no cleanup.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
if (Get-Process -Name Cyberpunk2077 -ErrorAction SilentlyContinue) { throw 'Game is running; isolated deployment test deferred' }
$fixture = Join-Path $repo ('scratch\deploy-regression-' + [Guid]::NewGuid().ToString('N').Substring(0,12))
$utf8 = [Text.UTF8Encoding]::new($false)
$prefix = 'red4ext\plugins\cyberpunkvrport\'
function Check([bool]$condition, [string]$why) { if (!$condition) { throw $why } }
function Put([string]$root, [string]$relative, [string]$content) {
    $path = [IO.Path]::GetFullPath((Join-Path $root $relative))
    Check ($path.StartsWith($fixture + '\', [StringComparison]::OrdinalIgnoreCase)) 'Fixture escaped scratch'
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path))
    [IO.File]::WriteAllText($path, $content, $utf8)
}
function Hash([string]$path) { return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Read([string]$path) { return [IO.File]::ReadAllText($path) }
function Run-Case([string]$name, [bool]$existing, [bool]$roomscale) {
    $case = Join-Path $fixture $name
    $game = Join-Path $case 'game'
    $package = Join-Path $case 'package'
    Put $game 'bin\x64\Cyberpunk2077.exe' 'fixture placeholder; not an executable'
    Put $game 'bin\x64\dxgi.dll' 'third-party-overlay-proxy-sentinel'
    Put $game 'config\vrport.ini' 'legacy-config-must-stay-unchanged'
    Put $package ($prefix+'cyberpunkvrport.dll') 'plugin-new-payload'
    Put $package ($prefix+'config\tweaks\vrport\fixture.yaml') 'Items.fixture: {}'
    Put $package ($prefix+'config\vrport.ini') "first_launch=1`r`nxr_custom_user_setting=0`r`n"
    Put $package ($prefix+'config\vrport-launcher.ini') "width=2000`r`nheight=2100`r`n"
    Put $package ($prefix+'config\vrik_calibration.ini') "custom_arm_length=0.650`r`n"
    Put $package ($prefix+'lua\CyberpunkVRPort_Stereo\vrcam.json') '{"component":"default_choice","virtualCamera":"default_camera","catalogue":["default_choice","user$2","new_entry"]}'
    Put $package ($prefix+'lua\Fixture\init.lua') 'return {version=2}'
    if ($existing) {
        Put $game ($prefix+'config\vrport.ini') "first_launch=1`r`nxr_custom_user_setting=73`r`n"
        Put $game ($prefix+'config\vrport-launcher.ini') "width=3210`r`nheight=2788`r`n"
        Put $game ($prefix+'config\vrik_calibration.ini') "custom_arm_length=0.743`r`n"
        Put $game ($prefix+'lua\CyberpunkVRPort_Stereo\vrcam.json') '{"component":"user$2","virtualCamera":"user_camera","catalogue":["old"]}'
    }
    $expectedLauncher = Hash (Join-Path $(if($existing){$game}else{$package}) ($prefix+'config\vrport-launcher.ini'))
    $expectedCalibration = Hash (Join-Path $(if($existing){$game}else{$package}) ($prefix+'config\vrik_calibration.ini'))
    $oldIniHash = if($existing){Hash (Join-Path $game ($prefix+'config\vrport.ini'))}else{$null}
    # The production backup names have second resolution; avoid a collision with
    # this test's preceding case or another already completed deployment.
    while(Test-Path -LiteralPath (Join-Path $repo ('build\roomscale-deploy-'+(Get-Date -Format 'yyyyMMdd-HHmmss')))) {
        Start-Sleep -Milliseconds 100
    }
    $args = @{GameRoot=$game; Package=$package}
    if($roomscale){$args.EnableRoomscale=$true}
    $result = (& (Join-Path $repo 'scripts\deploy_full.ps1') @args | Out-String) | ConvertFrom-Json
    $report = (Read (Join-Path $result.Backup 'deployment.json')) | ConvertFrom-Json
    Check (!$result.GameStarted -and !$report.GameStarted) 'Deployer claimed it started the game'
    Check ((Read (Join-Path $game 'bin\x64\dxgi.dll')) -eq 'third-party-overlay-proxy-sentinel') 'Third-party DXGI was removed/overwritten'
    Check ((Read (Join-Path $game 'config\vrport.ini')) -eq 'legacy-config-must-stay-unchanged') 'First-launch edit used obsolete config path'
    Check ((Hash (Join-Path $game ($prefix+'config\vrport-launcher.ini'))) -eq $expectedLauncher) 'Launcher selection was overwritten'
    Check ((Hash (Join-Path $game ($prefix+'config\vrik_calibration.ini'))) -eq $expectedCalibration) 'Calibration was overwritten'
    $ini = Read (Join-Path $game ($prefix+'config\vrport.ini'))
    Check ($ini -match '(?m)^first_launch=0\r?$') 'First-launch flag was not set in active plugin config'
    Check (!$ini.Contains("0`n")) 'Set-Ini introduced mixed LF/CRLF endings'
    Check ($ini.Contains($(if($existing){'xr_custom_user_setting=73'}else{'xr_custom_user_setting=0'}))) 'User runtime setting lost'
    if($roomscale){foreach($line in 'xr_roomscale_movement=1','xr_physical_body_rotation=1','xr_snap_turn_yaw_index=0'){Check ($ini.Contains($line)) "Missing roomscale setting $line"}}
    $stereo = (Read (Join-Path $game ($prefix+'lua\CyberpunkVRPort_Stereo\vrcam.json'))) | ConvertFrom-Json
    Check ($stereo.component -eq $(if($existing){'user$2'}else{'default_choice'})) 'Selected component merge failed'
    Check ($stereo.virtualCamera -eq $(if($existing){'user_camera'}else{'default_camera'})) 'Selected virtual camera merge failed'
    Check ($stereo.catalogue -contains 'new_entry' -and $stereo.catalogue -notcontains 'old') 'VRCAM catalogue did not refresh'
    foreach($entry in $report.Files){
        $path=Join-Path $game $entry.Path
        Check ((Hash $path) -eq $entry.SHA256) "Final payload hash mismatch: $($entry.Path)"
        Check ((Get-Item -LiteralPath $path).Length -eq $entry.Bytes) "Final payload size mismatch: $($entry.Path)"
        if($entry.Path -notin @(($prefix+'config\vrport.ini'),($prefix+'lua\CyberpunkVRPort_Stereo\vrcam.json'))){
            Check ((Hash $path) -eq (Hash (Join-Path $package $entry.Path))) "Unmodified payload differs: $($entry.Path)"
        }
    }
    foreach($entry in $report.Preserved){Check ((Hash (Join-Path $game $entry.Path)) -eq $entry.SHA256) "Preserved final hash stale: $($entry.Path)"}
    if($existing){
        $before=Join-Path $result.Backup ('before\'+$prefix+'config\vrport.ini')
        Check ((Hash $before) -eq $oldIniHash) 'Backup no longer contains pre-deployment preferences'
        Check ($report.PreservedCount -eq 3) 'Expected three preserved runtime files'
    }else{Check ($report.PreservedCount -eq 0) 'Fresh game falsely reported preserved files'}
    Write-Output "PASS ${name}: hashes, sizes, settings, VRCAM, third-party DXGI, active config and backup"
    return $result.Backup
}
$backups=@()
$backups += Run-Case 'existing-settings' $true $false
$backups += Run-Case 'fresh-roomscale' $false $true
[IO.File]::WriteAllText((Join-Path $fixture 'results.json'),(@{fixture=$fixture;outputs=$backups}|ConvertTo-Json -Depth 4),$utf8)
Write-Output "PASS deploy_full regression; fixture: $fixture"
