<#
.SYNOPSIS
Проверяет, что при установке более новой версии Private Music поверх старой сохраняется сеанс входа в
систему и данные приложения так же, как это происходит при обновлении пользователя.

.DESCRIPTION
scripts\upgrade-check.ps1 -Old <старый setup.exe> -New <новый setup.exe>

Устанавливает старую версию без диалогов, создаёт маркеры в хранилище учётных данных и папках
приложения, затем устанавливает новую версию поверх старой. Завершается с ошибкой, если маркер
пропал, версия не обновилась или программа отсутствует. Предназначен для CI: меняет установленные
у текущего пользователя программы и записи диспетчера учётных данных.
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$Old,
  [Parameter(Mandatory)][string]$New
)

$ErrorActionPreference = 'Stop'
$AppName = 'Private Music'
$AppId = 'com.dec.privatemusic2.desktop'
$Target = "vk-session.$AppId"   # Имя записи сессии в диспетчере учётных данных Windows.
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

# Установщик записывает папку в кавычки.
function Folder-Of($entry) { return ([string]$entry.InstallLocation).Trim('"') }

function Install($setup) {
  Say "installing $(Split-Path $setup -Leaf)"
  $process = Start-Process -FilePath $setup -ArgumentList '/S' -PassThru -Wait
  if ($process.ExitCode -ne 0) { Fail "the installer exited with $($process.ExitCode)" }
  # NSIS может передать установку дочернему процессу: ждём появления записи в реестре.
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
Say "installed version $($before.DisplayVersion) at $(Folder-Of $before)"

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
Say "installed version $($after.DisplayVersion) at $(Folder-Of $after)"

$problems = @()
$expected = Version-Of $New
if ($expected -and $after.DisplayVersion -ne $expected) { $problems += "version is $($after.DisplayVersion), expected $expected" }
if ((Folder-Of $after) -ne (Folder-Of $before)) { $problems += "install folder moved from $(Folder-Of $before) to $(Folder-Of $after)" }
if (-not (Test-Path (Join-Path (Folder-Of $after) 'private-music-desktop.exe'))) { $problems += 'the program file is missing' }
if (-not (Credential-Exists)) { $problems += 'the saved sign-in was removed from Credential Manager' }
foreach ($folder in $folders) {
  if (-not (Test-Path (Join-Path $folder $Marker))) { $problems += "app data was removed: $folder" }
}

& cmdkey /delete:$Target | Out-Null
foreach ($folder in $folders) { Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $folder $Marker) }

if ($problems) { $problems | ForEach-Object { Write-Host "FAILED: $_" -ForegroundColor Red }; exit 1 }
Say 'the upgrade kept the sign-in and the app data'
