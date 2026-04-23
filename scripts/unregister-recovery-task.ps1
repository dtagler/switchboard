#Requires -RunAsAdministrator

<#
.SYNOPSIS
    Removes the switchboard boot recovery scheduled task.

.DESCRIPTION
    Unregisters the "switchboard-boot-recover" scheduled task.
    Safe to call even if task doesn't exist (idempotent).

.EXAMPLE
    .\unregister-recovery-task.ps1
#>

$taskName = "switchboard-boot-recover"

$existing = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue

if ($existing) {
    Write-Host "Removing task: $taskName"
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
    Write-Host "✓ Task removed successfully" -ForegroundColor Green
} else {
    Write-Host "Task not found: $taskName (already removed or never registered)" -ForegroundColor Yellow
}
