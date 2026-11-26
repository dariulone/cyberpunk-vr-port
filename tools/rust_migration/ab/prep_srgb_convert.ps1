# ab.ps1 -Prep: the test marker cvr_swapchain_convert.txt beside the game's exe for each launch (both builds read it:
# the runtime's sRGB companion swapchain format is passed over, so each eye goes through the sRGB decode pass --
# srgb_pass's pipelines in game); -Restore takes it out again.
param([switch]$Restore)
$ErrorActionPreference = 'Stop'
$marker = Join-Path 'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64' 'cvr_swapchain_convert.txt'
if ($Restore) {
    if (Test-Path -LiteralPath $marker) { Remove-Item -LiteralPath $marker -Force }
} else {
    [System.IO.File]::WriteAllText($marker, 'ab.ps1 -Prep (tools/rust_migration/ab/prep_srgb_convert.ps1)', [System.Text.UTF8Encoding]::new($false))
}
