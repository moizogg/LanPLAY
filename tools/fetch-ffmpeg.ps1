# Download FFmpeg essentials (includes h264_qsv / nvenc / amf / libx264) for LANPlay.
# Sunshine-class encode path uses this binary.
#
# Run:  pwsh -File tools/fetch-ffmpeg.ps1

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
$OutDir = Join-Path $Root "apps\desktop\src-tauri\resources\ffmpeg"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$destExe = Join-Path $OutDir "ffmpeg.exe"
if (Test-Path $destExe) {
    $len = (Get-Item $destExe).Length
    if ($len -gt 1MB) {
        Write-Host "==> ffmpeg.exe already present ($([math]::Round($len/1MB,1)) MB) — skip download"
        & $destExe -hide_banner -encoders 2>&1 | Select-String -Pattern "h264_qsv|h264_nvenc|h264_amf|libx264" | ForEach-Object { Write-Host "   $_" }
        exit 0
    }
}

# Gyan.dev release essentials build (Windows x64) — widely used, QSV-enabled.
$zipUrl = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
$tmpZip = Join-Path $env:TEMP "lanplay-ffmpeg-essentials.zip"
$tmpDir = Join-Path $env:TEMP "lanplay-ffmpeg-extract"

Write-Host "==> Downloading FFmpeg essentials…"
Write-Host "    $zipUrl"
Invoke-WebRequest -Uri $zipUrl -OutFile $tmpZip -UseBasicParsing

if (Test-Path $tmpDir) { Remove-Item -Recurse -Force $tmpDir }
New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null

Write-Host "==> Extracting…"
Expand-Archive -Path $tmpZip -DestinationPath $tmpDir -Force

$found = Get-ChildItem -Path $tmpDir -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1
if (-not $found) {
    throw "ffmpeg.exe not found inside essentials zip"
}

Copy-Item -Force $found.FullName $destExe
# Optional: ffprobe not required
Write-Host "==> Installed: $destExe ($([math]::Round((Get-Item $destExe).Length/1MB,1)) MB)"

Write-Host "==> Encoder probe:"
& $destExe -hide_banner -encoders 2>&1 | Select-String -Pattern "h264_qsv|h264_nvenc|h264_amf|libx264" | ForEach-Object { Write-Host "   $_" }

# cleanup
Remove-Item -Force $tmpZip -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue

# gitignore note: large binary — CI fetches; optional local cache
Write-Host "==> Done. Bundle path: apps/desktop/src-tauri/resources/ffmpeg/ffmpeg.exe"
