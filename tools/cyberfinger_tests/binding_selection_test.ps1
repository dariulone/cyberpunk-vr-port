#Requires -Version 5.1
# Exercise the selector with an in-process SteamVR API mock; never contacts SteamVR.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$selector = Join-Path $repo 'scripts\select_cyberfinger_binding.ps1'
$fixture = Join-Path $repo ('build\cyberfinger-binding-tests\' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($fixture)
$path = Join-Path $fixture 'binding with spaces.json'
$expectedUri = ([uri]$path).AbsoluteUri
$names = @('cf_weapon', 'cf_inventory', 'cf_scanner', 'cf_heal', 'cf_settings') | ForEach-Object { "/actions/gameplay/in/$_" }
$binding = @{
    app_key = 'system.generated.openxr.cyberpunkvrport.cyberpunk2077.exe'
    controller_type = 'cyberfinger'; category = 'openxr'
    bindings = @{ '/actions/gameplay' = @{ sources = @($names | ForEach-Object {
        @{ path = '/user/hand/left/input/b'; mode = 'button'; inputs = @{ click = @{ output = $_ } } }
    }) } }
} | ConvertTo-Json -Depth 15
[IO.File]::WriteAllText($path, $binding)
$cfBindingTest = [pscustomobject]@{
    posts = 0; reads = 0; category = 'openxr'; current = 'file:///previous.json'
    available = @($names); hasController = $true
    closesOnRecheck = $false; applySelection = $true
}

function Assert([bool]$Condition, [string]$Message) { if (!$Condition) { throw $Message } }
function Invoke-RestMethod {
    param($Uri, $Headers, $TimeoutSec, $Method, $ContentType, $Body)
    Assert ($Uri.StartsWith('http://127.0.0.1:27062/input/')) 'Unexpected API destination'
    Assert ($Headers.Origin -eq 'http://127.0.0.1:27062') 'Missing local UI origin'
    if ($Method -eq 'Post') {
        Assert ($Uri.EndsWith('/selectconfig.action')) 'Unexpected mutation'
        $request = $Body | ConvertFrom-Json
        Assert ($request.app_key -eq 'system.generated.openxr.cyberpunkvrport.cyberpunk2077.exe') 'Wrong app'
        Assert ($request.controller_type -eq 'cyberfinger') 'Wrong controller'
        Assert ($request.url -eq $expectedUri) 'Wrong file URI'
        $cfBindingTest.posts++
        if ($cfBindingTest.applySelection) { $cfBindingTest.current = $request.url }
        return [pscustomobject]@{ success = $true }
    }
    $cfBindingTest.reads++
    $selection = [pscustomobject]@{}
    if ($cfBindingTest.hasController) { $selection | Add-Member NoteProperty cyberfinger $cfBindingTest.current }
    $category = $cfBindingTest.category
    if ($cfBindingTest.closesOnRecheck -and $cfBindingTest.reads -gt 1) { $category = 'legacy' }
    return [pscustomobject]@{
        category = $category; current_binding_url = $selection
        action_sets = @([pscustomobject]@{ actions = @($cfBindingTest.available | ForEach-Object { [pscustomobject]@{ name = $_ } }) })
    }
}
function Expect-Failure([string]$Message) {
    $failed = $false
    try { & $selector -BindingPath $path | Out-Null }
    catch { $failed = $true; Assert ($_.ToString().Contains($Message)) "Unexpected error: $_" }
    Assert $failed "Expected failure: $Message"
}

$cfBindingTest.category = 'legacy'
Expect-Failure 'has not registered its OpenXR actions'
Assert ($cfBindingTest.posts -eq 0) 'Selected a legacy binding'
$cfBindingTest.category = 'openxr'; $cfBindingTest.hasController = $false
Expect-Failure 'has not registered CyberFinger controllers'
Assert ($cfBindingTest.posts -eq 0) 'Selected without controllers'
$cfBindingTest.hasController = $true; $cfBindingTest.available = @($names[0])
Expect-Failure 'running game is missing binding actions'
Assert ($cfBindingTest.posts -eq 0) 'Selected actions absent from the game'
$cfBindingTest.available = @($names)
& $selector -BindingPath $path -WhatIf | Out-Null
Assert ($cfBindingTest.posts -eq 0) 'WhatIf changed the binding'
$cfBindingTest.reads = 0; $cfBindingTest.closesOnRecheck = $true
Expect-Failure 'has not registered its OpenXR actions'
Assert ($cfBindingTest.posts -eq 0) 'Selected after the game closed'
$cfBindingTest.closesOnRecheck = $false
& $selector -BindingPath $path | Out-Null
Assert ($cfBindingTest.posts -eq 1 -and $cfBindingTest.current -eq $expectedUri) 'Failed to select and verify'
& $selector -BindingPath $path | Out-Null
Assert ($cfBindingTest.posts -eq 1) 'Reselected an already active binding'
$cfBindingTest.current = 'file:///previous.json'; $cfBindingTest.applySelection = $false
Expect-Failure 'did not verify the OpenXR selection'
# The documented command omits BindingPath. Resolve its default inside the
# script, where PSScriptRoot is initialized even with powershell -File.
$defaultBinding = Get-Content (Join-Path $repo 'resources\cyberfinger\cyberpunk_openxr_v2.json') -Raw | ConvertFrom-Json
$defaultActions = $defaultBinding.bindings.'/actions/gameplay'
$cfBindingTest.available = @($defaultActions.sources | ForEach-Object {
    $_.inputs.PSObject.Properties | ForEach-Object { $_.Value.output }
}) + @($defaultActions.poses.output) + @($defaultActions.haptics.output)
$postsBefore = $cfBindingTest.posts
& $selector -WhatIf | Out-Null
Assert ($cfBindingTest.posts -eq $postsBefore) 'Default-path preview changed the binding'
Write-Output 'PASS: 9 binding-selection scenarios (legacy guard, controllers, actions, preview, game close, select, idempotence, unverified response, default path).'
