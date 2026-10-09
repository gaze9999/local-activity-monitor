# Source checkout bootstrap. Native release bundles already contain Python.
# Keep discovery offline even when Python Install Manager has no runtimes yet.
$env:PYTHON_MANAGER_AUTOMATIC_INSTALL = 'false'
$monitorEncoding = New-Object System.Text.UTF8Encoding($false)
[Console]::InputEncoding = $monitorEncoding
[Console]::OutputEncoding = $monitorEncoding
$OutputEncoding = $monitorEncoding

function Find-MonitorPython([string]$Root) {
    $localPython = Join-Path $Root '.venv\Scripts\python.exe'
    if (Test-Path -LiteralPath $localPython -PathType Leaf) { return $localPython }
    $probe = 'import sys, json; assert sys.version_info >= (3, 10); import ssl, sqlite3; print(json.dumps(sys.executable))'
    foreach ($name in @('py', 'python', 'python3')) {
        $prefix = @()
        if ($name -eq 'py') { $prefix = @('-3') }
        foreach ($command in @(Get-Command $name -CommandType Application -ErrorAction SilentlyContinue)) {
            try { $result = & $command.Source @prefix -I -B -c $probe 2>$null } catch { continue }
            if ($LASTEXITCODE -eq 0 -and $result) {
                # ASCII JSON survives Windows PowerShell's legacy console encoding.
                try { $python = ConvertFrom-Json -InputObject ([string]@($result)[-1]) -ErrorAction Stop } catch { continue }
                if (Test-Path -LiteralPath $python -PathType Leaf) { return $python }
            }
        }
    }
    return $null
}

function Start-Monitor([string]$Root, [string[]]$MonitorArguments, [bool]$InstallPython = $false) {
    if ((Test-Path -LiteralPath (Join-Path $Root '.venv')) -and -not (Test-Path -LiteralPath (Join-Path $Root '.venv\Scripts\python.exe') -PathType Leaf)) {
        Write-Host 'Existing .venv is incomplete. Inspect it before retrying; nothing was overwritten.'
        return 1
    }
    $python = Find-MonitorPython $Root
    if (-not $python) {
        Write-Host 'Python 3.10+ with SSL and SQLite support is required.'
        if (-not $InstallPython) {
            Write-Host 'No dependencies were installed. Install Python manually or retry with --install-python.'
            return 1
        }
        $manager = Get-Command pymanager -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        $installArguments = @('install', '3.14')
        if (-not $manager) {
            $manager = Get-Command winget -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
            $installArguments = @('install', '--id', 'Python.Python.3.14', '--exact', '--source', 'winget', '--scope', 'user', '--accept-source-agreements', '--accept-package-agreements')
        }
        if (-not $manager) {
            Write-Host 'No supported installer found. Install Python from https://www.python.org/downloads/windows/ and run launch-cli.cmd again.'
            return 1
        }
        Write-Host 'This will download and install Python 3.14 for your user account using the existing installer.'
        Write-Host ('Command: "{0}" {1}' -f $manager.Source, ($installArguments -join ' '))
        try { $answer = [string](Read-Host 'Install Python now? [y/N]') } catch { $answer = '' }
        if ($answer.Trim().ToLowerInvariant() -notin @('y', 'yes')) {
            Write-Host 'Setup cancelled. No installation was performed.'
            return 0
        }
        & $manager.Source @installArguments | Out-Host
        if ($LASTEXITCODE -ne 0) {
            Write-Host 'Python installation failed. Inspect the installer error above, then retry launch-cli.cmd.'
            return 1
        }
        # Installers may add Python/py to PATH. Refresh this process only.
        $env:PATH = [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + $env:PATH
        $python = Find-MonitorPython $Root
        if (-not $python) {
            Write-Host 'Python is still unavailable or incomplete. Reopen the terminal and retry launch-cli.cmd, or inspect the installation.'
            return 1
        }
    }
    & $python -X utf8 -I -B (Join-Path $Root 'tools\launch-cli.py') @MonitorArguments | Out-Host
    return $LASTEXITCODE
}

# Dot sourcing exposes the bootstrap functions for isolated launcher tests.
if ($MyInvocation.InvocationName -ne '.') {
    try {
        $root = Split-Path $PSScriptRoot -Parent
        $install = '--install-python' -in $args
        $forwarded = @($args | Where-Object { $_ -notin @('--console', '--install-python') })
        if (Test-Path -LiteralPath (Join-Path $root 'launch-cli.exe') -PathType Leaf) {
            & (Join-Path $root 'launch-cli.exe') @forwarded | Out-Host
            exit $LASTEXITCODE
        }
        $monitorExit = Start-Monitor $root $forwarded $install
        if ($monitorExit -ne 0) {
            Write-Host '監測入口未正常完成, 請查看上方錯誤與 README.md 的啟動排查方式'
        }
        Write-Host ('監測入口已結束, 退出碼 ' + $monitorExit)
        Write-Host '若上方顯示 already_running, 請使用原本的監測視窗與網頁'
        if (Test-Path -LiteralPath (Join-Path $root '.local\startup-status.json') -PathType Leaf) {
            Write-Host '啟動階段與退出結果記錄在 .local\startup-status.json'
        }
        exit $monitorExit
    } catch {
        Write-Host ('Setup or startup failed: ' + $_.Exception.Message)
        exit 1
    }
}
