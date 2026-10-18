param(
    [string]$CacheDirectory = (Join-Path ([IO.Path]::GetTempPath()) 'private-music-ffmpeg')
)

$ErrorActionPreference = 'Stop'
$version = '9.0.2'
$expectedHash = '60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba'
$package = "ffmpeg-$version-essentials_build"
$root = Join-Path $CacheDirectory $package
$binary = Join-Path $root 'bin/ffmpeg.exe'

# Закреплённая версия и контрольная сумма защищают тесты от подмены загрузки.
# Сборки Gyan указаны на официальной странице загрузок FFmpeg для Windows.
if (-not (Test-Path -LiteralPath $binary)) {
    New-Item -ItemType Directory -Path $CacheDirectory -Force | Out-Null
    $archive = Join-Path $CacheDirectory "$package.zip"
    Invoke-WebRequest -Uri "https://github.com/GyanD/codexffmpeg/releases/download/$version/$package.zip" `
        -OutFile $archive -MaximumRetryCount 3 -RetryIntervalSec 3 -TimeoutSec 300
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expectedHash) {
        throw 'Контрольная сумма ffmpeg не совпадает с закреплённой версией.'
    }
    Expand-Archive -LiteralPath $archive -DestinationPath $CacheDirectory -Force
}

& $binary -version
if ($LASTEXITCODE -ne 0) { throw 'Не удалось запустить ffmpeg.' }
$binDirectory = Split-Path -Parent $binary
$env:PATH = "$binDirectory$([IO.Path]::PathSeparator)$env:PATH"
if ($env:GITHUB_PATH) { $binDirectory | Out-File -FilePath $env:GITHUB_PATH -Encoding utf8 -Append }
