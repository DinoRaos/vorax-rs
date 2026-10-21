# Registers a Windows scheduled task that runs scripts/sync.ps1 every day (default 10:30, after upstream's
# nightly smarter-player job at 08:10 UTC). Remove it with:  Unregister-ScheduledTask -TaskName VoraxRustSync
param([string]$At = '10:30')

$script = Join-Path $PSScriptRoot 'sync.ps1'
$pwsh = (Get-Command pwsh -ErrorAction SilentlyContinue)?.Source ?? (Get-Command powershell).Source
$action = New-ScheduledTaskAction -Execute $pwsh -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$script`"" -WorkingDirectory (Split-Path -Parent $PSScriptRoot)
$trigger = New-ScheduledTaskTrigger -Daily -At $At
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 1)
Register-ScheduledTask -TaskName 'VoraxRustSync' -Action $action -Trigger $trigger -Settings $settings -Description 'Pull tli-vorax-simulator, re-export its data, re-check the Rust port against it' -Force | Out-Null
Write-Host "Registered VoraxRustSync: daily at $At. Results go to sync.log."
