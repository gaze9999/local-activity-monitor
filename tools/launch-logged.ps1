# Keep an independent wrapper around the existing bootstrap, including its errors.
$monitorEncoding = New-Object Text.UTF8Encoding($false)
[Console]::InputEncoding = $monitorEncoding
[Console]::OutputEncoding = $monitorEncoding
$OutputEncoding = $monitorEncoding
. (Join-Path $PSScriptRoot 'launch-log.ps1')
$root = Split-Path $PSScriptRoot -Parent
$monitorExit = $null
try {
    try {
        $logPath = Initialize-MonitorLog $root
        Write-Host ('啟動 Log: ' + $logPath)
        Write-MonitorLog ('wrapper_start pid={0} powershell={1}' -f $PID, $PSVersionTable.PSVersion)
    } catch {
        Write-Host ('無法建立啟動 Log: ' + $_.Exception.GetType().Name)
    }
    $powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    Write-MonitorLog 'bootstrap_start'
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'launch-windows.ps1') @args 2>&1 | Write-MonitorOutput
        $monitorExit = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousPreference
    }
    Write-MonitorLog ('bootstrap_exit code=' + $monitorExit)
} catch {
    Write-MonitorLog ([string]$_) 'exception'
    Write-MonitorLog ([string]$_.ScriptStackTrace) 'stack'
    Write-Host ('啟動失敗: ' + (Protect-MonitorLog $_.Exception.Message))
    $monitorExit = 1
} finally {
    $exitValue = if ($null -eq $monitorExit) { 'unknown (exit code not captured)' } else { [string]$monitorExit }
    Write-MonitorLog ('wrapper_exit code=' + $exitValue)
    Close-MonitorLog
}
if ($null -eq $monitorExit) { exit 130 }
exit $monitorExit
