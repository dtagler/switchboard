#Requires -RunAsAdministrator

<#
.SYNOPSIS
    Registers the switchboard boot recovery scheduled task.

.DESCRIPTION
    Creates a Windows scheduled task named "switchboard-boot-recover" that runs
    "switchboard.exe --recover" at system startup as NT AUTHORITY\SYSTEM.
    
    This ensures the internal keyboard is always enabled at boot, before any
    user logon, providing a safety layer independent of switchboard's own startup.

.PARAMETER SwitchboardPath
    Full path to switchboard.exe. Defaults to the current directory's switchboard.exe.

.EXAMPLE
    .\register-recovery-task.ps1
    
.EXAMPLE
    .\register-recovery-task.ps1 -SwitchboardPath "C:\Program Files\switchboard\switchboard.exe"
#>

param(
    [string]$SwitchboardPath = (Join-Path $PSScriptRoot "..\dist\switchboard.exe")
)

# Resolve to absolute path
$SwitchboardPath = Resolve-Path $SwitchboardPath -ErrorAction Stop | Select-Object -ExpandProperty Path

if (-not (Test-Path $SwitchboardPath)) {
    Write-Error "switchboard.exe not found at: $SwitchboardPath"
    exit 1
}

Write-Host "Registering boot recovery task for: $SwitchboardPath"

$taskName = "switchboard-boot-recover"

# Unregister if exists (idempotent)
$existing = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "Removing existing task..."
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
}

# Create task
$action = New-ScheduledTaskAction -Execute $SwitchboardPath -Argument "--recover"

$trigger = New-ScheduledTaskTrigger -AtStartup

$principal = New-ScheduledTaskPrincipal -UserId "NT AUTHORITY\SYSTEM" -LogonType ServiceAccount -RunLevel Highest

$settings = New-ScheduledTaskSettingsSet `
    -StartWhenAvailable `
    -DontStopIfGoingOnBatteries `
    -AllowStartIfOnBatteries `
    -ExecutionTimeLimit (New-TimeSpan -Minutes 2)

Register-ScheduledTask `
    -TaskName $taskName `
    -Action $action `
    -Trigger $trigger `
    -Principal $principal `
    -Settings $settings `
    -Description "switchboard boot recovery: ensures internal keyboard is enabled at startup (runs before user logon)" `
    | Out-Null

Write-Host "✓ Task registered successfully" -ForegroundColor Green
Write-Host ""
Write-Host "Task name: $taskName"
Write-Host "Trigger:   At system startup"
Write-Host "Principal: NT AUTHORITY\SYSTEM (Highest)"
Write-Host "Action:    $SwitchboardPath --recover"
Write-Host ""
Write-Host "To remove: .\unregister-recovery-task.ps1"
