<#
.SYNOPSIS
Устанавливает Private Music в Windows из последней версии GitHub.

.DESCRIPTION
.\install.ps1 устанавливает или обновляет приложение с открытием окна установщика.
.\install.ps1 -Silent выполняет тихую установку. .\install.ps1 -Uninstall удаляет приложение.

Публичным релизам не нужен токен. Для частных выпусков установите $env:GITHUB_TOKEN или войдите в
систему с помощью GitHub CLI (вход с аутентификацией gh). Необязательно: $env:PM_REPO,
$env:PM_VERSION (например, v0.2.0), $env:PM_API (для тестов).

Файл, загруженный этим сценарием, не помечен как "from the internet", поэтому Windows SmartScreen не
останавливает (неподписанный) установщик.
#>
[CmdletBinding()]
param(
  [switch]$Silent,
  [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest работает намного быстрее без индикатора выполнения.
if ($PSVersionTable.PSEdition -eq 'Desktop') {
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
}

$Repo = if ($env:PM_REPO) { $env:PM_REPO } else { 'decoder-dev/PrivateMusicDesktop' }
$Api = if ($env:PM_API) { $env:PM_API } else { 'https://api.github.com' }
$Version = if ($env:PM_VERSION) { $env:PM_VERSION } else { 'latest' }
$AppName = 'Private Music'

function Say($text) { Write-Host "==> $text" -ForegroundColor Cyan }
function Fail($text) { Write-Host "error: $text" -ForegroundColor Red; exit 1 }

function Invoke-WindowsInstaller([string]$arguments) {
  $process = Start-Process -FilePath "$env:SystemRoot\System32\msiexec.exe" -ArgumentList $arguments -Wait -PassThru
  switch ($process.ExitCode) {
    0 { return }
    { $_ -in 1641, 3010 } { Say 'restart Windows to finish the operation'; return }
    default { Fail "Windows Installer exited with code $($process.ExitCode)." }
  }
}

# ------------------------------------------------------------------ удалить
if ($Uninstall) {
  $roots = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
           'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
           'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
  $entries = foreach ($root in $roots) {
    if (Test-Path $root) {
      Get-ChildItem $root | Get-ItemProperty | Where-Object { $_.DisplayName -in @($AppName, 'PrivateMusic') }
    }
  }
  $msiEntries = $entries | Where-Object {
    $_.WindowsInstaller -eq 1 -and $_.PSChildName -match '^\{[0-9a-f-]{36}\}$'
  }
  if (-not $entries) { Fail "$AppName is not installed." }
  Say "removing $AppName"
  if ($msiEntries) {
    foreach ($entry in $msiEntries) {
      $arguments = "/x $($entry.PSChildName) /norestart"
      if ($Silent) { $arguments += ' /quiet' }
      Invoke-WindowsInstaller $arguments
    }
  } else {
    $entry = $entries | Where-Object { $_.UninstallString } | Select-Object -First 1
    if (-not $entry) { Fail "could not find the uninstaller for $AppName." }
    $uninstaller = $entry.UninstallString.Trim('"')
    $parameters = @{ FilePath = $uninstaller; Wait = $true; PassThru = $true }
    if ($Silent) { $parameters.ArgumentList = '/S' }
    $process = Start-Process @parameters
    if ($process.ExitCode -ne 0) { Fail "the uninstaller exited with code $($process.ExitCode)." }
  }
  Say 'removed. The saved sign-in stays in Windows Credential Manager until you sign out in the app.'
  exit 0
}

# --------------------------------------- токен
$Token = if ($env:GITHUB_TOKEN) { $env:GITHUB_TOKEN } elseif ($env:GH_TOKEN) { $env:GH_TOKEN } else { $null }
if (-not $Token -and (Get-Command gh -ErrorAction SilentlyContinue)) {
  try { $Token = (& gh auth token 2>$null) } catch { $Token = $null }
}
$Headers = @{ 'User-Agent' = 'PrivateMusicInstaller'; 'Accept' = 'application/vnd.github+json' }
if ($Token) { $Headers['Authorization'] = "Bearer $Token" }

# ----------------------------------------------------------------------------
Say "looking up the release ($Version)"
$url = if ($Version -eq 'latest') { "$Api/repos/$Repo/releases/latest" } else { "$Api/repos/$Repo/releases/tags/$Version" }
try {
  $release = Invoke-RestMethod -Uri $url -Headers $Headers
} catch {
  Fail "could not read the release. For a private repository set `$env:GITHUB_TOKEN (or run 'gh auth login'); a draft release must be published first. ($($_.Exception.Message))"
}

function Get-Asset($pattern) { $release.assets | Where-Object { $_.name -match $pattern } | Select-Object -First 1 }

function Save-Asset($asset, $path) {
  if (-not $Token -and $Api -eq 'https://api.github.com') {
    $tag = [uri]::EscapeDataString($release.tag_name)
    $name = [uri]::EscapeDataString($asset.name)
    Invoke-WebRequest -Uri "https://github.com/$Repo/releases/download/$tag/$name" -UseBasicParsing -OutFile $path
  } else {
    $download = @{ 'User-Agent' = 'PrivateMusicInstaller'; 'Accept' = 'application/octet-stream' }
    if ($Token) { $download['Authorization'] = "Bearer $Token" }
    Invoke-WebRequest -Uri "$Api/repos/$Repo/releases/assets/$($asset.id)" -Headers $download -UseBasicParsing -OutFile $path
  }
}

$installer = Get-Asset '^PrivateMusic_[0-9]+\.[0-9]+\.[0-9]+_windows-x64\.msi$'
if (-not $installer) { $installer = Get-Asset '^[a-z0-9_.-]+-setup\.exe$' }
if (-not $installer) { Fail 'the release has no Windows installer.' }

$temp = Join-Path ([IO.Path]::GetTempPath()) ("private-music-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp | Out-Null
$file = Join-Path $temp $installer.name
$sumsFile = Join-Path $temp 'SHA256SUMS.txt'
try {
  Say "downloading $($installer.name)"
  Save-Asset $installer $file

  $sums = Get-Asset '^SHA256SUMS\.txt$'
  if ($sums) {
    Save-Asset $sums $sumsFile
    $pattern = '^[0-9a-f]{64}\s+\*?' + [regex]::Escape($installer.name) + '$'
    $line = Get-Content -LiteralPath $sumsFile | Where-Object { $_ -match $pattern } | Select-Object -First 1
    if (-not $line) { Fail "$($installer.name) is not listed in SHA256SUMS.txt." }
    $want = ($line -split '\s+')[0].ToLowerInvariant()
    $got = (Get-FileHash -Algorithm SHA256 -LiteralPath $file).Hash.ToLowerInvariant()
    if ($want -ne $got) { Fail "checksum mismatch for $($installer.name); the download is damaged or was tampered with." }
    Say 'checksum ok'
  } else {
    Fail 'the release has no SHA256SUMS.txt; refusing to install an unverified file.'
  }

  Unblock-File -LiteralPath $file
  Say "installing $AppName $($release.tag_name)"
  if ($installer.name -like '*.msi') {
    $arguments = "/i `"$file`" /norestart"
    if ($Silent) { $arguments += ' /quiet' }
    Invoke-WindowsInstaller $arguments
  } else {
    $parameters = @{ FilePath = $file; Wait = $true; PassThru = $true }
    if ($Silent) { $parameters.ArgumentList = '/S' }
    $process = Start-Process @parameters
    if ($process.ExitCode -ne 0) { Fail "the installer exited with code $($process.ExitCode)." }
  }
  Say 'done. Start it from the Start menu.'
} finally {
  Remove-Item -LiteralPath $file, $sumsFile -Force -ErrorAction SilentlyContinue
  if (-not (Get-ChildItem -LiteralPath $temp -Force)) { [IO.Directory]::Delete($temp) }
}
