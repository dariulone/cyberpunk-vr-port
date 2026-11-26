param([int]$TimeoutSeconds = 60)
# Close Cyberpunk 2077 the way a player does (WM_CLOSE to its windows), wait for it to exit, report the exit code.
$ErrorActionPreference = 'Stop'
Add-Type -Namespace W -Name U -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool PostMessageW(System.IntPtr h, uint m, System.IntPtr w, System.IntPtr l);
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, System.IntPtr l);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr h, out uint pid);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr h);
public delegate bool EnumProc(System.IntPtr h, System.IntPtr l);
'@
$procs = @(Get-Process Cyberpunk2077 -ErrorAction SilentlyContinue)
if (-not $procs) { 'not running'; return }
foreach ($p in $procs) {
    $ids = [System.Collections.Generic.List[System.IntPtr]]::new()
    [W.U]::EnumWindows({ param($h, $l) $pid2 = 0; [void][W.U]::GetWindowThreadProcessId($h, [ref]$pid2); if ($pid2 -eq $p.Id -and [W.U]::IsWindowVisible($h)) { $ids.Add($h) }; $true }, [System.IntPtr]::Zero) | Out-Null
    foreach ($h in $ids) { [void][W.U]::PostMessageW($h, 0x0010, [System.IntPtr]::Zero, [System.IntPtr]::Zero) }
    if ($p.WaitForExit($TimeoutSeconds * 1000)) {
        "exited pid=$($p.Id) code=$($p.ExitCode) windows=$($ids.Count)"
    } else {
        $p.Kill(); $p.WaitForExit(10000) | Out-Null
        "KILLED after ${TimeoutSeconds}s pid=$($p.Id) windows=$($ids.Count)"
    }
}
