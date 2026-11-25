#Requires -Version 5.1
<#
.SYNOPSIS
Select the installed CyberFinger layout for the running GOG/OpenXR Cyberpunk app.
.DESCRIPTION
Connect the headset and start Cyberpunk first. Uses SteamVR's local binding UI
API, verifies the live action manifest and selection, and saves the previous URL
under build/cyberfinger-install-backups. Does not install a driver or start apps.
The API is an internal SteamVR interface; unsupported responses fail explicitly.
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string]$BindingPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ([string]::IsNullOrWhiteSpace($BindingPath)) {
    $BindingPath = Join-Path $PSScriptRoot '..\resources\cyberfinger\cyberpunk_openxr_v2.json'
}
$appKey = 'system.generated.openxr.cyberpunkvrport.cyberpunk2077.exe'
$controller = 'cyberfinger'
$baseUri = 'http://127.0.0.1:27062'
$headers = @{ Origin = $baseUri; Referer = "$baseUri/dashboard/controllerbinding.html" }
$actionsUri = "$baseUri/input/getactions.json?app_key=$appKey"
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$utf8 = New-Object Text.UTF8Encoding($false)

function Field($Object, [string]$Name) {
    if ($null -eq $Object) { return $null }
    $p = $Object.PSObject.Properties[$Name]
    if ($null -ne $p) { return $p.Value }
    return $null
}
function Read-Actions {
    try { return Invoke-RestMethod -Uri $actionsUri -Headers $headers -TimeoutSec 5 }
    catch { throw "Cannot read SteamVR's binding interface. Connect Steam Link, start SteamVR and Cyberpunk, then retry. $($_.Exception.Message)" }
}
function Assert-OpenXR($Actions) {
    if ((Field $Actions 'category') -ne 'openxr') {
        throw 'Cyberpunk has not registered its OpenXR actions. Connect the headset and leave Cyberpunk at its main menu, then retry. No binding was selected.'
    }
}
function Binding-Outputs($Node) {
    if ($null -eq $Node) { return }
    if ($Node -is [System.Collections.IEnumerable] -and $Node -isnot [string] -and $Node -isnot [pscustomobject]) {
        foreach ($item in $Node) { Binding-Outputs $item }
    } elseif ($Node -is [pscustomobject]) {
        foreach ($p in $Node.PSObject.Properties) {
            if ($p.Name -eq 'output') { [string]$p.Value }
            else { Binding-Outputs $p.Value }
        }
    }
}

$binding = Get-Content -LiteralPath $BindingPath -Raw | ConvertFrom-Json
if ((Field $binding 'app_key') -ne $appKey -or
    (Field $binding 'controller_type') -ne $controller -or
    (Field $binding 'category') -ne 'openxr') {
    throw 'Expected a CyberFinger OpenXR binding for CyberpunkVRPort; no binding was selected.'
}
$fullPath = (Resolve-Path -LiteralPath $BindingPath).ProviderPath
$bindingUri = ([uri]$fullPath).AbsoluteUri
$before = Read-Actions
Assert-OpenXR $before
$current = Field (Field $before 'current_binding_url') $controller
if ($null -eq $current) {
    throw 'SteamVR has not registered CyberFinger controllers. Connect both hands in CyberFinger Bridge and retry.'
}
$actionNames = @((Field $before 'action_sets') | ForEach-Object {
    (Field $_ 'actions') | ForEach-Object { Field $_ 'name' }
})
$outputs = @(Binding-Outputs (Field $binding 'bindings') | Sort-Object -Unique)
foreach ($name in @('cf_weapon', 'cf_inventory', 'cf_scanner', 'cf_heal', 'cf_settings')) {
    if ($outputs -notcontains "/actions/gameplay/in/$name") {
        throw "The binding is missing CyberFinger action $name; no binding was selected."
    }
}
$missing = @($outputs | Where-Object { $actionNames -notcontains $_ })
if ($missing.Count) {
    throw ('The running game is missing binding actions; verify the patched DLL and cyberfinger.ini enabled=1. Missing: ' + ($missing -join ', '))
}
if ($current -eq $bindingUri) {
    Write-Output "Already selected and verified: $bindingUri"
    return
}
if (!$PSCmdlet.ShouldProcess($appKey, "Select CyberFinger binding $bindingUri")) { return }

$backup = Join-Path $repo ('build\cyberfinger-install-backups\binding-selection-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($backup)
[IO.File]::WriteAllText((Join-Path $backup 'before.json'), ($before | ConvertTo-Json -Depth 30), $utf8)
$record = [ordered]@{
    Status = 'SelectionRequested'; AppKey = $appKey; Controller = $controller
    PreviousUrl = $current; SelectedUrl = $bindingUri
    BindingSha256 = (Get-FileHash -LiteralPath $fullPath -Algorithm SHA256).Hash
}
$recordPath = Join-Path $backup 'install.json'
[IO.File]::WriteAllText($recordPath, ($record | ConvertTo-Json), $utf8)
# Re-check immediately before the mutation. A closed game can otherwise make
# SteamVR silently store the selection under its unrelated legacy category.
Assert-OpenXR (Read-Actions)
$body = @{ app_key = $appKey; controller_type = $controller; url = $bindingUri } | ConvertTo-Json
$reply = Invoke-RestMethod -Uri "$baseUri/input/selectconfig.action" -Method Post -Headers $headers -ContentType 'application/json' -Body $body -TimeoutSec 5
if ((Field $reply 'success') -ne $true) { throw "SteamVR rejected the selection. Backup: $backup" }
for ($attempt = 0; $attempt -lt 12; $attempt++) {
    Start-Sleep -Milliseconds 250
    $after = Read-Actions
    if ((Field $after 'category') -eq 'openxr' -and
        (Field (Field $after 'current_binding_url') $controller) -eq $bindingUri) {
        $record.Status = 'Complete'
        [IO.File]::WriteAllText((Join-Path $backup 'after.json'), ($after | ConvertTo-Json -Depth 30), $utf8)
        [IO.File]::WriteAllText($recordPath, ($record | ConvertTo-Json), $utf8)
        Write-Output "Selected and verified CyberFinger CyberpunkVRPort. Previous selection saved in $backup"
        return
    }
}
throw "SteamVR accepted the request but did not verify the OpenXR selection. Keep the game running and check Controller Bindings. Backup: $backup"
