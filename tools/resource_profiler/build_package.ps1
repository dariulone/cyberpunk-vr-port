param(
    [string]$BuildDir = (Join-Path $PSScriptRoot '..\..\build\resource-profiler'),
    [string]$PackageRoot = (Join-Path $PSScriptRoot '..\..\dist'),
    [string]$Generator = 'Visual Studio 18 2026'
)
$ErrorActionPreference = 'Stop'
$BuildDir = [IO.Path]::GetFullPath($BuildDir)
$PackageRoot = [IO.Path]::GetFullPath($PackageRoot)
& cmake -S $PSScriptRoot -B $BuildDir -G $Generator -A x64
if ($LASTEXITCODE -ne 0) { throw 'Profiler configure failed' }
& cmake --build $BuildDir --config Release --parallel 2
if ($LASTEXITCODE -ne 0) { throw 'Profiler build failed' }
& ctest --test-dir $BuildDir -C Release --output-on-failure
if ($LASTEXITCODE -ne 0) { throw 'Profiler unit checks failed' }

# Every packaging run has its own directory; no removal of existing releases/logs.
$packageName = 'CyberpunkVRProfiler-1.0.0-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
$stage = Join-Path $PackageRoot $packageName
$payload = Join-Path $stage 'CyberpunkVRProfiler'
[IO.Directory]::CreateDirectory($payload) | Out-Null
Copy-Item -LiteralPath (Join-Path $BuildDir 'Release\CyberpunkVRProfiler.exe') -Destination $payload
Copy-Item -LiteralPath (Join-Path $PSScriptRoot '..\..\LICENSE') -Destination (Join-Path $payload 'LICENSE.txt')
$utf8Bom = [Text.UTF8Encoding]::new($true)
$ascii = [Text.Encoding]::ASCII
foreach ($name in @('RUN_PROFILER.bat','RUN_FULL_THREADS.bat','RUN_5_MINUTES.bat','README_RU.txt','README_EN.txt')) {
    $contents = [IO.File]::ReadAllText((Join-Path $PSScriptRoot $name)).Replace("`r`n", "`n").Replace("`n", "`r`n")
    $encoding = if ($name.EndsWith('.bat')) { $ascii } else { $utf8Bom }
    [IO.File]::WriteAllText((Join-Path $payload $name), $contents, $encoding)
}
$hash = (Get-FileHash -LiteralPath (Join-Path $payload 'CyberpunkVRProfiler.exe') -Algorithm SHA256).Hash
[IO.File]::WriteAllText((Join-Path $payload 'SHA256.txt'), "$hash  CyberpunkVRProfiler.exe`r`n", $ascii)
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archivePath = Join-Path $PackageRoot ($packageName + '.zip')
[IO.Compression.ZipFile]::CreateFromDirectory($stage, $archivePath, [IO.Compression.CompressionLevel]::Optimal, $false)
Write-Output "Package: $archivePath"
Write-Output "Portable EXE: $(Join-Path $payload 'CyberpunkVRProfiler.exe')"
Write-Output "SHA256: $hash"
