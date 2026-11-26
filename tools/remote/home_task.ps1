# Runs a command line in lone33's interactive desktop session on the second PC (a one-shot scheduled task with an
# interactive logon: a process started from ssh has no desktop, the game needs one). Output goes to $Log.
#   home_task.ps1 -Name CVR_X -Command 'py -3.13 -B X:\...\script.py args' -Log X:\cvr-tmp\x.log [-Wait]
param([string]$Name, [string]$Command, [string]$Log, [switch]$Wait)
$cmd = "/c $Command > `"$Log`" 2>&1"
$action = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument $cmd -WorkingDirectory 'X:\CyberpunkVRPort'
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 4)
Register-ScheduledTask -TaskName $Name -Action $action -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName $Name
"started $Name"
if ($Wait) {
  do { Start-Sleep -Seconds 2; $s = (Get-ScheduledTask -TaskName $Name).State } while ($s -eq 'Running')
  "finished: " + (Get-ScheduledTaskInfo -TaskName $Name).LastTaskResult
}
