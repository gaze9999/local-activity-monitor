# Use an existing runtime; the shared launcher never installs by default.
& (Join-Path $PSScriptRoot 'tools\launch-windows.ps1') --console @args
exit $LASTEXITCODE
