# Download / build official ViGEm redistributables into Tauri resources.
# Run from repo root:  pwsh -File tools/fetch-vigem-redist.ps1
#
# Users never visit GitHub — CI ships these inside LANPlay.
#
# - ViGEmBus setup: official signed installer (kernel driver, one-time UAC)
# - ViGEmClient.dll: built from nefarius/ViGEmClient (native C API)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
$OutDir = Join-Path $Root "apps\desktop\src-tauri\resources\vigem"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$VigemBusVersion = "v1.22.0"
$ViGEmClientRepo = "https://github.com/nefarius/ViGEmClient.git"
$ViGEmClientRef = "master" # pin a commit SHA later if you want stricter repro

Write-Host "==> Fetching ViGEmBus setup ($VigemBusVersion)…"
$releaseApi = "https://api.github.com/repos/nefarius/ViGEmBus/releases/tags/$VigemBusVersion"
try {
    $release = Invoke-RestMethod -Uri $releaseApi -Headers @{ "User-Agent" = "LANPlay-CI" }
} catch {
    Write-Host "    Tag lookup failed, using /releases/latest…"
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/nefarius/ViGEmBus/releases/latest" -Headers @{ "User-Agent" = "LANPlay-CI" }
}

$setupAsset = $release.assets | Where-Object {
    $_.name -match '(?i)(setup|install|ViGEmBus).*\.(exe|msi)$'
} | Select-Object -First 1

if (-not $setupAsset) {
    $setupAsset = $release.assets | Where-Object { $_.name -match '\.(exe|msi)$' } | Select-Object -First 1
}
if (-not $setupAsset) {
    throw "Could not find ViGEmBus installer asset in release $($release.tag_name)"
}

$setupPath = Join-Path $OutDir $setupAsset.name
Write-Host "    Downloading $($setupAsset.name)…"
Invoke-WebRequest -Uri $setupAsset.browser_download_url -OutFile $setupPath -UseBasicParsing
Write-Host "    Saved $setupPath"

$aliasExe = Join-Path $OutDir "ViGEmBus_Setup.exe"
$aliasMsi = Join-Path $OutDir "ViGEmBus_Setup.msi"
if ($setupAsset.name -match '\.msi$') {
    Copy-Item -Force $setupPath $aliasMsi
    if (Test-Path $aliasExe) { Remove-Item $aliasExe -Force }
} else {
    Copy-Item -Force $setupPath $aliasExe
    if (Test-Path $aliasMsi) { Remove-Item $aliasMsi -Force }
}

Write-Host "==> Building native ViGEmClient.dll from source…"
$work = Join-Path $env:TEMP "lanplay-vigemclient-build"
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Force -Path $work | Out-Null

$src = Join-Path $work "ViGEmClient"
Write-Host "    Cloning $ViGEmClientRepo ($ViGEmClientRef)…"
git clone --depth 1 --branch $ViGEmClientRef $ViGEmClientRepo $src 2>&1 | Out-Host

$build = Join-Path $src "build"
# SHARED DLL so we can ship next to lanplay.exe
$cmakeArgs = @(
    "-S", $src,
    "-B", $build,
    "-A", "x64",
    "-DViGEmClient_DLL=ON"
)
Write-Host "    cmake configure…"
& cmake @cmakeArgs
if ($LASTEXITCODE -ne 0) { throw "cmake configure failed" }

Write-Host "    cmake build (Release)…"
& cmake --build $build --config Release
if ($LASTEXITCODE -ne 0) { throw "cmake build failed" }

$dll = Get-ChildItem -Path $build -Recurse -Filter "ViGEmClient.dll" |
    Where-Object { $_.FullName -match 'Release|RelWithDebInfo' } |
    Select-Object -First 1
if (-not $dll) {
    $dll = Get-ChildItem -Path $build -Recurse -Filter "ViGEmClient.dll" | Select-Object -First 1
}
if (-not $dll) {
    throw "ViGEmClient.dll not produced by build (is MSVC/CMake available?)"
}

$dllOut = Join-Path $OutDir "ViGEmClient.dll"
Copy-Item -Force $dll.FullName $dllOut
Write-Host "    Saved $dllOut"

$notice = @"
LANPlay bundles ViGEm redistributables so end users do not download them manually.

- ViGEmBus driver installer: Nefarius Software Solutions (BSD-3-Clause)
  https://github.com/nefarius/ViGEmBus
  Release: $($release.tag_name)
  Asset: $($setupAsset.name)

- ViGEmClient.dll: built from https://github.com/nefarius/ViGEmClient ($ViGEmClientRef)

Kernel driver still requires a one-time Windows install (UAC).
LANPlay Host UI runs the bundled installer when needed.
"@
Set-Content -Path (Join-Path $OutDir "THIRD_PARTY_VIGEM.txt") -Value $notice -Encoding UTF8

Write-Host "==> Done."
Get-ChildItem $OutDir | Format-Table Name, Length
