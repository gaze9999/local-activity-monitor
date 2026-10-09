# Standalone logging works before Python discovery or application imports.
$script:MonitorLogWriter = $null
$script:MonitorLogLimit = 1MB
$script:MonitorLogBackups = 3
$script:MonitorLogPath = $null

function Protect-MonitorLog([string]$Text) {
    if ($Text.Length -gt 16384) { $Text = $Text.Substring(0, 16384) + ' [line truncated]' }
    $Text = [regex]::Replace($Text, '(?i)\bBearer\s+[A-Za-z0-9._~+/-]+=*|\bsk-[A-Za-z0-9_-]{12,}', '[已隱藏]')
    $Text = [regex]::Replace($Text, '(?i)\b(?:api[_-]?key|password|secret|credential|authorization|cookie|access[_-]?token|refresh[_-]?token|token)["'']?\s*(?:[:=]\s*|\s+)(?:"[^"\r\n]*"|''[^''\r\n]*''|[^\s,;]+)', '[已隱藏]')
    return [regex]::Replace($Text, '(?i)(https?://)[^/\s@]+@', '$1[已隱藏]@')
}

function Write-MonitorLog([string]$Text, [string]$Kind = 'launcher') {
    if (-not $script:MonitorLogWriter) { return }
    $entry = '[{0}] [{1}] {2}' -f [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ'), $Kind, (Protect-MonitorLog $Text)
    $size = [Text.Encoding]::UTF8.GetByteCount($entry + [Environment]::NewLine)
    try {
        if ($script:MonitorLogWriter.BaseStream.Length + $size -gt $script:MonitorLogLimit) {
            Rotate-MonitorLog
        }
        $script:MonitorLogWriter.WriteLine($entry)
    } catch {
        Write-Host ('無法繼續寫入啟動 Log: ' + $_.Exception.GetType().Name)
        Close-MonitorLog
    }
}

function Open-MonitorLog([IO.FileMode]$Mode) {
    $stream = [IO.File]::Open($script:MonitorLogPath, $Mode, [IO.FileAccess]::Write, [IO.FileShare]::Read)
    $encoding = New-Object Text.UTF8Encoding($false)
    $script:MonitorLogWriter = New-Object IO.StreamWriter($stream, $encoding)
    $script:MonitorLogWriter.AutoFlush = $true
}

function Rotate-MonitorLog {
    # All paths are literal siblings of the file created by this launch.
    $directory = [IO.Path]::GetDirectoryName($script:MonitorLogPath)
    $paths = @($script:MonitorLogPath) + @(1..$script:MonitorLogBackups | ForEach-Object { $script:MonitorLogPath + '.' + $_ })
    foreach ($path in $paths) {
        if ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($path)) -ne $directory) { throw 'Log path escaped the owned directory' }
        if (Test-Path -LiteralPath $path) {
            $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
            if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Log rotation path is redirected' }
        }
    }
    Close-MonitorLog
    for ($index = $script:MonitorLogBackups; $index -ge 1; $index--) {
        $target = $script:MonitorLogPath + '.' + $index
        $source = if ($index -eq 1) { $script:MonitorLogPath } else { $script:MonitorLogPath + '.' + ($index - 1) }
        if ($index -eq $script:MonitorLogBackups -and (Test-Path -LiteralPath $target)) { Remove-Item -LiteralPath $target -ErrorAction Stop }
        if (Test-Path -LiteralPath $source) { Move-Item -LiteralPath $source -Destination $target -ErrorAction Stop }
    }
    Open-MonitorLog ([IO.FileMode]::CreateNew)
}

function Write-MonitorOutput {
    param([Parameter(ValueFromPipeline = $true)][object]$Value)
    process {
        $kind = if ($Value -is [Management.Automation.ErrorRecord]) { 'stderr' } else { 'stdout' }
        $text = Protect-MonitorLog ([string]$Value)
        Write-MonitorLog $text $kind
        Write-Host $text
    }
}

function Initialize-MonitorLog([string]$Root) {
    $directory = Join-Path $Root '.local\startup-logs'
    foreach ($path in @((Join-Path $Root '.local'), $directory)) {
        if (Test-Path -LiteralPath $path) {
            $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
            if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw 'Startup log directory is not an owned plain directory'
            }
        } else {
            New-Item -ItemType Directory -Path $path -ErrorAction Stop | Out-Null
        }
    }
    $path = Join-Path $directory ('startup-{0}-{1}.log' -f [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff'), $PID)
    $script:MonitorLogPath = [IO.Path]::GetFullPath($path)
    Open-MonitorLog ([IO.FileMode]::CreateNew)
    # Only this logger's reserved filenames are eligible for retention cleanup.
    $older = @(Get-ChildItem -LiteralPath $directory -File -Force | Where-Object {
        $_.Name -match '^startup-\d{8}-\d{6}-\d{3}-\d+\.log$' -and -not ($_.Attributes -band [IO.FileAttributes]::ReparsePoint)
    } | Sort-Object Name -Descending | Select-Object -Skip 10)
    foreach ($file in $older) {
        try {
            Remove-Item -LiteralPath $file.FullName -ErrorAction Stop
            foreach ($index in 1..$script:MonitorLogBackups) {
                $backup = $file.FullName + '.' + $index
                if (Test-Path -LiteralPath $backup) {
                    $item = Get-Item -LiteralPath $backup -Force -ErrorAction Stop
                    if (-not $item.PSIsContainer -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { Remove-Item -LiteralPath $backup -ErrorAction Stop }
                }
            }
        } catch { Write-MonitorLog ('retention skipped: ' + $_.Exception.GetType().Name) }
    }
    return $path
}

function Close-MonitorLog {
    if ($script:MonitorLogWriter) {
        try { $script:MonitorLogWriter.Dispose() } catch { }
        $script:MonitorLogWriter = $null
    }
}
