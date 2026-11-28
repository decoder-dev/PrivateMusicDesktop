[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$PreviousMsi,
  [Parameter(Mandatory)][string]$CurrentMsi
)

$ErrorActionPreference = 'Stop'
$registryLocations = @(
  'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
  'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'
)

function Get-Products {
  foreach ($location in $registryLocations) {
    if (Test-Path -LiteralPath $location) {
      Get-ChildItem -LiteralPath $location | Get-ItemProperty |
        Where-Object { $_.DisplayName -eq 'PrivateMusic' -and $_.WindowsInstaller -eq 1 }
    }
  }
}

function Read-Package([string]$path) {
  $file = Get-Item -LiteralPath $path
  if ($file.Name -notmatch '^PrivateMusic_(\d+\.\d+\.\d+)_windows-x64\.msi$') {
    throw "Unexpected installer name: $($file.Name)"
  }
  [pscustomobject]@{ Path = $file.FullName; Version = [version]$Matches[1] }
}

function Invoke-Msi([string]$operation, [string]$target, [string]$step) {
  $log = Join-Path $env:TEMP "private-music-$step.log"
  $arguments = '{0} "{1}" /quiet /norestart /l*v "{2}"' -f $operation, $target, $log
  $process = Start-Process -FilePath "$env:SystemRoot\System32\msiexec.exe" -ArgumentList $arguments -WindowStyle Hidden -Wait -PassThru
  if ($process.ExitCode -notin 0, 3010) {
    Get-Content -LiteralPath $log -Tail 60 -ErrorAction SilentlyContinue | Write-Host
    throw "$step failed with Windows Installer exit code $($process.ExitCode)."
  }
}

function Assert-Installation([version]$expected) {
  $products = @(Get-Products)
  if ($products.Count -ne 1 -or [version]$products[0].DisplayVersion -ne $expected) {
    $installedVersions = $products.DisplayVersion -join ', '
    throw "Expected exactly one installed PrivateMusic $expected. Found: $installedVersions"
  }
  $location = $products[0].InstallLocation
  if (-not $location -or -not (Test-Path -LiteralPath (Join-Path $location 'PrivateMusic.exe') -PathType Leaf)) {
    throw 'The registered application launcher is missing.'
  }
  return $products[0]
}

function Assert-LauncherStarts($product) {
  $launcher = Join-Path $product.InstallLocation 'PrivateMusic.exe'
  $process = Start-Process -FilePath $launcher -WindowStyle Hidden -PassThru
  try {
    if ($process.WaitForExit(5000)) {
      throw "The packaged launcher stopped during startup with exit code $($process.ExitCode)."
    }
  } finally {
    if (-not $process.HasExited -and (-not $process.CloseMainWindow() -or -not $process.WaitForExit(5000))) {
      Stop-Process -InputObject $process
      $process.WaitForExit()
    }
  }
}

$previous = Read-Package $PreviousMsi
$current = Read-Package $CurrentMsi
if ($previous.Version -ge $current.Version) { throw 'The current version must be newer than the published baseline.' }
if (Get-Products) { throw 'Use a disposable Windows runner without an existing PrivateMusic installation.' }

$appData = Join-Path $env:APPDATA 'PrivateMusicDesktop'
$marker = Join-Path $appData ("installer-test-" + [guid]::NewGuid() + '.txt')
try {
  Invoke-Msi '/i' $current.Path 'clean-install'
  $installed = Assert-Installation $current.Version
  Assert-LauncherStarts $installed
  Invoke-Msi '/x' $installed.PSChildName 'clean-uninstall'
  if (Get-Products) { throw 'The clean test install was not removed.' }
  Write-Host "Clean install and launch passed for $($current.Version)."

  Invoke-Msi '/i' $previous.Path 'baseline-install'
  $installed = Assert-Installation $previous.Version
  New-Item -ItemType Directory -Path $appData -Force | Out-Null
  Set-Content -LiteralPath $marker -Value 'keep app data'
  Invoke-Msi '/i' $current.Path 'upgrade'
  $upgraded = Assert-Installation $current.Version
  if ($upgraded.InstallLocation -ne $installed.InstallLocation -or
      -not (Test-Path -LiteralPath $marker) -or (Get-Content -Raw -LiteralPath $marker).Trim() -ne 'keep app data') {
    throw 'The upgrade changed the install location or removed app data.'
  }
  Assert-LauncherStarts $upgraded
  Write-Host "Upgrade from $($previous.Version) to $($current.Version) preserved app data and launched."
} finally {
  Remove-Item -LiteralPath $marker -Force -ErrorAction SilentlyContinue
}
