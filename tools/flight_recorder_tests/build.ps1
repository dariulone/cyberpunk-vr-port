param(
 [Parameter(Mandatory=$true)][string]$JsonInclude,
 [Parameter(Mandatory=$true)][string]$OpenXRInclude,
 [Parameter(Mandatory=$true)][string]$FidelityFxSdk,
 [string]$BuildDirectory,
 [string]$CMake='cmake',
 [string]$CTest='ctest'
)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if(!$BuildDirectory){$BuildDirectory=Join-Path $root 'build-flight-tests'}
& $CMake -S $PSScriptRoot -B $BuildDirectory -G 'Visual Studio 17 2022' -A x64 "-DFLIGHT_JSON_INCLUDE=$JsonInclude" "-DFLIGHT_OPENXR_INCLUDE=$OpenXRInclude" "-DFLIGHT_FIDELITYFX_SDK=$FidelityFxSdk"
if($LASTEXITCODE){exit $LASTEXITCODE}
& $CMake --build $BuildDirectory --config Release --parallel 4
if($LASTEXITCODE){exit $LASTEXITCODE}
& $CTest --test-dir $BuildDirectory -C Release --output-on-failure
exit $LASTEXITCODE
