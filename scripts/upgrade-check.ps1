<#
.SYNOPSIS
  Checks that installing a newer Private Music over an older one keeps the
  signed-in session and the app's data, the way a user's update does.

.DESCRIPTION
  scripts\upgrade-check.ps1 -Old <old setup.exe> -New <new setup.exe>

  Installs OLD silently, plants markers where the app keeps things (the secret
  the sign-in lives in, and the app data folders), installs NEW over it
  silently and fails if a marker is gone, the version did not change to NEW's,
  or the program is missing. Meant for CI (it changes the current user's
  installed programs and Credential Manager).
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$Old,
  [Parameter(Mandatory)][string]$New
)

$ErrorActionPreference = 'Stop'
$AppName = 'Private Music'
$AppId = 'com.dec.privatemusic2.desktop'
$Target = "vk-session.$AppId"   # where the keyring crate keeps the sign-in
$Marker = 'upgrade-marker.txt'

function Say($text) { Write-Host "==> $text" -ForegroundColor Cyan }
function Fail($text) { Write-Host "error: $text" -ForegroundColor Red; exit 1 }

function Get-Installed {
  $roots = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall', 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
  foreach ($root in $roots) {
    if (Test-Path $root) {
      $found = Get-ChildItem $root | ForEach-Object { Get-ItemProperty $_.PSPath } | Where-Object { $_.DisplayName -eq $AppName } | Select-Object -First 1
      if ($found) { return $found }
    }
  }
  return $null
}

function Install($setup) {
  Say "installing $(Split-Path $setup -Leaf)"
  $process = Start-Process -FilePath $setup -ArgumentList '/S' -PassThru -Wait
  if ($process.ExitCode -ne 0) { Fail "the installer exited with $($process.ExitCode)" }
  # NSIS may hand over to a copy of itself: wait until the registry shows it.
  for ($i = 0; $i -lt 60 -and -not (Get-Installed); $i++) { Start-Sleep -Seconds 1 }
}

function Version-Of($setup) {
  if ((Split-Path $setup -Leaf) -match '(\d+\.\d+\.\d+)') { return $Matches[1] }
  return $null
}

function Credential-Exists { [bool](& cmdkey /list:$Target 2>$null | Select-String -SimpleMatch $Target) }

$folders = @(
  (Join-Path $env:APPDATA $AppId),
  (Join-Path $env:LOCALAPPDATA $AppId),
  (Join-Path $env:LOCALAPPDATA "$AppId\EBWebView")
)

if (Get-Installed) { Fail "$AppName is already installed here; run this on a clean machine." }

Install $Old
$before = Get-Installed
if (-not $before) { Fail 'the old version did not install.' }
Say "installed version $($before.DisplayVersion) at $($before.InstallLocation)"

Say 'planting markers: the saved sign-in and the app data folders'
& cmdkey /generic:$Target /user:vk-session /pass:upgrade-marker | Out-Null
if (-not (Credential-Exists)) { Fail 'could not write the test credential.' }
foreach ($folder in $folders) {
  New-Item -ItemType Directory -Force -Path $folder | Out-Null
  Set-Content -Path (Join-Path $folder $Marker) -Value 'kept'
}

Install $New
$after = Get-Installed
if (-not $after) { Fail 'nothing is installed after the upgrade.' }
Say "installed version $($after.DisplayVersion) at $($after.InstallLocation)"

$problems = @()
$expected = Version-Of $New
if ($expected -and $after.DisplayVersion -ne $expected) { $problems += "version is $($after.DisplayVersion), expected $expected" }
if ($after.InstallLocation -ne $before.InstallLocation) { $problems += "install folder moved from $($before.InstallLocation) to $($after.InstallLocation)" }
if (-not (Test-Path (Join-Path $after.InstallLocation 'private-music-desktop.exe'))) { $problems += 'the program file is missing' }
if (-not (Credential-Exists)) { $problems += 'the saved sign-in was removed from Credential Manager' }
foreach ($folder in $folders) {
  if (-not (Test-Path (Join-Path $folder $Marker))) { $problems += "app data was removed: $folder" }
}

& cmdkey /delete:$Target | Out-Null
foreach ($folder in $folders) { Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $folder $Marker) }

if ($problems) { $problems | ForEach-Object { Write-Host "FAILED: $_" -ForegroundColor Red }; exit 1 }
Say 'the upgrade kept the sign-in and the app data'
