# Source checkout bootstrap. Native release bundles already contain Python.
# Keep discovery offline even when Python Install Manager has no runtimes yet.
$env:PYTHON_MANAGER_AUTOMATIC_INSTALL = 'false'

function Find-MonitorPython([string]$Root) {
    $localPython = Join-Path $Root '.venv\Scripts\python.exe'
    if (Test-Path -LiteralPath $localPython -PathType Leaf) { return $localPython }
    $probe = 'import sys, json; assert sys.version_info >= (3, 10); import venv, ensurepip, ssl, sqlite3; print(json.dumps(sys.executable))'
    foreach ($name in @('py', 'python', 'python3')) {
        $command = Get-Command $name -CommandType Application -ErrorAction SilentlyContinue
        if (-not $command) { continue }
        $prefix = @()
        if ($name -eq 'py') { $prefix = @('-3') }
        $result = & $command.Source @prefix -I -B -c $probe 2>$null
        if ($LASTEXITCODE -eq 0 -and $result) {
            # ASCII JSON survives Windows PowerShell's legacy console encoding.
            try { $python = ConvertFrom-Json -InputObject ([string]@($result)[-1]) -ErrorAction Stop } catch { continue }
            if (Test-Path -LiteralPath $python -PathType Leaf) { return $python }
        }
    }
    return $null
}

function Start-Monitor([string]$Root, [string[]]$MonitorArguments) {
    if ((Test-Path -LiteralPath (Join-Path $Root '.venv')) -and -not (Test-Path -LiteralPath (Join-Path $Root '.venv\Scripts\python.exe') -PathType Leaf)) {
        Write-Host 'Existing .venv is incomplete. Inspect it before retrying; nothing was overwritten.'
        return 1
    }
    $python = Find-MonitorPython $Root
    if (-not $python) {
        Write-Host 'Python 3.10+ with venv, pip, SSL and SQLite support is required.'
        $manager = Get-Command pymanager -CommandType Application -ErrorAction SilentlyContinue
        $installArguments = @('install', '3.14')
        if (-not $manager) {
            $manager = Get-Command winget -CommandType Application -ErrorAction SilentlyContinue
            $installArguments = @('install', '--id', 'Python.Python.3.14', '--exact', '--source', 'winget', '--scope', 'user', '--accept-source-agreements', '--accept-package-agreements')
        }
        if (-not $manager) {
            Write-Host 'No supported installer found. Install Python from https://www.python.org/downloads/windows/ and run Start.cmd again.'
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
            Write-Host 'Python installation failed. Inspect the installer error above, then retry Start.cmd.'
            return 1
        }
        # Installers may add Python/py to PATH. Refresh this process only.
        $env:PATH = [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + $env:PATH
        $python = Find-MonitorPython $Root
        if (-not $python) {
            Write-Host 'Python is still unavailable or incomplete. Reopen the terminal and retry Start.cmd, or inspect the installation.'
            return 1
        }
    }
    & $python -I -B (Join-Path $Root 'launch.py') @MonitorArguments | Out-Host
    return $LASTEXITCODE
}

# Dot sourcing exposes the bootstrap functions for isolated launcher tests.
if ($MyInvocation.InvocationName -ne '.') {
    try {
        exit (Start-Monitor (Split-Path $PSScriptRoot -Parent) $args)
    } catch {
        Write-Host ('Setup or startup failed: ' + $_.Exception.Message)
        exit 1
    }
}
