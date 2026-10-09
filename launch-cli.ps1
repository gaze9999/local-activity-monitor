# Use an existing runtime; the shared launcher never installs by default.
& (Join-Path $PSScriptRoot 'tools\launch-logged.ps1') --console @args
exit $LASTEXITCODE
