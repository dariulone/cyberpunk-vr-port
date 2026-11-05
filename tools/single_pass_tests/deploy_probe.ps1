param(
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-z0-9-]+$')][string]$Label,
    [Parameter(Mandatory=$true)][ValidatePattern('^[A-Fa-f0-9]{64}$')][string]$Previous,
    [string]$GameRoot='C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077'
)
$ErrorActionPreference='Stop'
if(Get-Process -Name Cyberpunk2077,qrenderdoc -ErrorAction SilentlyContinue){throw 'Close game and RenderDoc first'}
$repo=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$built=Join-Path $repo 'build\bin\red4ext\plugins\cyberpunkvrport\Release'
$installed=Join-Path $GameRoot 'red4ext\plugins\cyberpunkvrport'
$archive=Join-Path $repo ('build\single-pass-20260927\deploy-'+$Label)
if(Test-Path -LiteralPath $archive){throw 'Deployment archive already exists'}
if((Get-FileHash -LiteralPath (Join-Path $installed 'cyberpunkvrport.dll')).Hash -ne $Previous){throw 'Previous DLL hash differs'}
$settings=@{
    'UserSettings.json'=(Join-Path $installed 'UserSettings.json')
    'vrik_calibration.ini'=(Join-Path $installed 'vrik_calibration.ini')
    'vrport.ini'=(Join-Path $GameRoot 'red4ext\plugins\cyberpunkvrport\config\vrport.ini')
    'vrport-launcher.ini'=(Join-Path $GameRoot 'red4ext\plugins\cyberpunkvrport\config\vrport-launcher.ini')
    'GameUserSettings.json'=(Join-Path $env:LOCALAPPDATA 'CD Projekt Red\Cyberpunk 2077\UserSettings.json')
}
function SettingsHash($Path) {
    if (Test-Path -LiteralPath $Path -PathType Leaf) { return (Get-FileHash -LiteralPath $Path).Hash }
    return $null
}
$before=@{}
foreach($name in @('cyberpunkvrport.dll','cyberpunkvrport.pdb')){$before[$name]=(Get-FileHash -LiteralPath (Join-Path $installed $name)).Hash}
foreach($name in $settings.Keys){$before[$name]=SettingsHash $settings[$name]}
New-Item -ItemType Directory -Path (Join-Path $archive 'previous') | Out-Null
$before | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $archive 'before.json')
foreach($name in @('cyberpunkvrport.dll','cyberpunkvrport.pdb')){
    Copy-Item -LiteralPath (Join-Path $installed $name) -Destination (Join-Path $archive ('previous\'+$name))
    Copy-Item -LiteralPath (Join-Path $built $name) -Destination (Join-Path $archive $name)
    Copy-Item -LiteralPath (Join-Path $built $name) -Destination (Join-Path $installed $name) -Force
}
$after=@{}
foreach($name in @('cyberpunkvrport.dll','cyberpunkvrport.pdb')){$after[$name]=(Get-FileHash -LiteralPath (Join-Path $installed $name)).Hash}
foreach($name in $settings.Keys){$after[$name]=SettingsHash $settings[$name]}
foreach($name in $settings.Keys){if($before[$name] -ne $after[$name]){throw ('Settings changed: '+$name)}}
$after | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $archive 'manifest.json')
$after | ConvertTo-Json
