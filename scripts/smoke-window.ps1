param([Parameter(Mandatory=$true)][string]$Binary)
$ErrorActionPreference = 'Stop'
$report = Join-Path $env:RUNNER_TEMP 'window-modes.txt'
Remove-Item -LiteralPath $report -ErrorAction SilentlyContinue
Remove-Item Env:PM_SMOKE_LOGIN -ErrorAction SilentlyContinue
$env:PM_SMOKE_WINDOW = $report
$app = Start-Process -FilePath $Binary -WindowStyle Hidden -PassThru
if (-not $app.WaitForExit(30000)) { $app.Kill(); throw 'Window-mode smoke test timed out' }
if (Test-Path -LiteralPath $report) { Get-Content -LiteralPath $report }
if ($app.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $report) -or -not (Select-String -LiteralPath $report -Pattern '^passed ' -Quiet)) { throw 'Window-mode smoke test failed' }
