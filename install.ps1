<#
.SYNOPSIS
  Installs Private Music on Windows from the latest GitHub release.

.DESCRIPTION
  .\install.ps1              install or update (the normal installer window opens)
  .\install.ps1 -Silent      install without any window
  .\install.ps1 -Uninstall   remove it

  Public releases need no token. For private releases, set $env:GITHUB_TOKEN,
  or sign in with the GitHub CLI (gh auth login).
  Optional: $env:PM_REPO, $env:PM_VERSION (for example v0.2.0), $env:PM_API (for tests).

  A file downloaded by this script is not marked as "from the internet", so
  Windows SmartScreen does not stop the (unsigned) installer.
#>
[CmdletBinding()]
param(
  [switch]$Silent,
  [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is much faster without its progress bar
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13

$Repo = if ($env:PM_REPO) { $env:PM_REPO } else { 'decoder-dev/PrivateMusicDesktop' }
$Api = if ($env:PM_API) { $env:PM_API } else { 'https://api.github.com' }
$Version = if ($env:PM_VERSION) { $env:PM_VERSION } else { 'latest' }
$AppName = 'Private Music'

function Say($text) { Write-Host "==> $text" -ForegroundColor Cyan }
function Fail($text) { Write-Host "error: $text" -ForegroundColor Red; exit 1 }

# ---------------------------------------------------------------- uninstall
if ($Uninstall) {
  $roots = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall', 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
  $entry = foreach ($root in $roots) {
    if (Test-Path $root) {
      Get-ChildItem $root | ForEach-Object { Get-ItemProperty $_.PSPath } | Where-Object { $_.DisplayName -eq $AppName }
    }
  } | Select-Object -First 1
  if (-not $entry -or -not $entry.UninstallString) { Fail "$AppName is not installed." }
  Say "removing $AppName"
  $command = $entry.UninstallString
  if ($Silent) { $command = "$command /S" }
  Start-Process -FilePath 'cmd.exe' -ArgumentList '/c', "`"$command`"" -Wait
  Say 'removed. The saved sign-in stays in Windows Credential Manager until you sign out in the app.'
  exit 0
}

# --------------------------------------------------------------------- token
$Token = if ($env:GITHUB_TOKEN) { $env:GITHUB_TOKEN } elseif ($env:GH_TOKEN) { $env:GH_TOKEN } else { $null }
if (-not $Token -and (Get-Command gh -ErrorAction SilentlyContinue)) {
  try { $Token = (& gh auth token 2>$null) } catch { $Token = $null }
}
$Headers = @{ 'User-Agent' = 'PrivateMusicInstaller'; 'Accept' = 'application/vnd.github+json' }
if ($Token) { $Headers['Authorization'] = "Bearer $Token" }

# ------------------------------------------------------------------- release
Say "looking up the release ($Version)"
$url = if ($Version -eq 'latest') { "$Api/repos/$Repo/releases/latest" } else { "$Api/repos/$Repo/releases/tags/$Version" }
try {
  $release = Invoke-RestMethod -Uri $url -Headers $Headers
} catch {
  Fail "could not read the release. For a private repository set `$env:GITHUB_TOKEN (or run 'gh auth login'); a draft release must be published first. ($($_.Exception.Message))"
}

function Get-Asset($pattern) { $release.assets | Where-Object { $_.name -match $pattern } | Select-Object -First 1 }

function Save-Asset($asset, $path) {
  $download = @{ 'User-Agent' = 'PrivateMusicInstaller'; 'Accept' = 'application/octet-stream' }
  if ($Token) { $download['Authorization'] = "Bearer $Token" }
  Invoke-WebRequest -Uri "$Api/repos/$Repo/releases/assets/$($asset.id)" -Headers $download -OutFile $path
}

$installer = Get-Asset '-setup\.exe$'
if (-not $installer) { Fail 'the release has no Windows installer.' }

$temp = Join-Path ([IO.Path]::GetTempPath()) ("private-music-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp | Out-Null
try {
  $file = Join-Path $temp $installer.name
  Say "downloading $($installer.name)"
  Save-Asset $installer $file

  $sums = Get-Asset '^SHA256SUMS\.txt$'
  if ($sums) {
    $sumsFile = Join-Path $temp 'SHA256SUMS.txt'
    Save-Asset $sums $sumsFile
    $line = Get-Content $sumsFile | Where-Object { ($_ -split '\s+')[1] -replace '^\*', '' -eq $installer.name } | Select-Object -First 1
    if (-not $line) { Fail "$($installer.name) is not listed in SHA256SUMS.txt." }
    $want = ($line -split '\s+')[0].ToLowerInvariant()
    $got = (Get-FileHash -Algorithm SHA256 -Path $file).Hash.ToLowerInvariant()
    if ($want -ne $got) { Fail "checksum mismatch for $($installer.name); the download is damaged or was tampered with." }
    Say 'checksum ok'
  } else {
    Fail 'the release has no SHA256SUMS.txt; refusing to install an unverified file.'
  }

  Unblock-File -Path $file
  Say "installing $AppName $($release.tag_name)"
  $arguments = if ($Silent) { @('/S') } else { @() }
  $process = Start-Process -FilePath $file -ArgumentList $arguments -Wait -PassThru
  if ($process.ExitCode -ne 0) { Fail "the installer exited with code $($process.ExitCode)." }
  Say 'done. Start it from the Start menu.'
} finally {
  Remove-Item -Recurse -Force $temp -ErrorAction SilentlyContinue
}
