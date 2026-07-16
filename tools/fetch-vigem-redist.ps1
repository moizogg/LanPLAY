# Download official ViGEmBus driver installer into Tauri resources.
# ViGEmClient is compiled into lanplay.exe (static) — no DLL fetch needed.
#
# Run:  pwsh -File tools/fetch-vigem-redist.ps1

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
$OutDir = Join-Path $Root "apps\desktop\src-tauri\resources\vigem"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# Remove obsolete DLL from older package layouts
$oldDll = Join-Path $OutDir "ViGEmClient.dll"
if (Test-Path $oldDll) {
    Remove-Item -Force $oldDll
    Write-Host "Removed obsolete ViGEmClient.dll (now statically linked)."
}

$VigemBusVersion = "v1.22.0"

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

$notice = @"
LANPlay packages:

- ViGEmClient: compiled statically into lanplay.exe (Nefarius ViGEmClient, MIT)
  Source: third-party/ViGEmClient (vendored from https://github.com/nefarius/ViGEmClient)

- ViGEmBus driver installer: $($release.tag_name) / $($setupAsset.name)
  https://github.com/nefarius/ViGEmBus (BSD-3-Clause)

Kernel driver still requires one-time Windows install (UAC) via Host UI.
"@
Set-Content -Path (Join-Path $OutDir "THIRD_PARTY_VIGEM.txt") -Value $notice -Encoding UTF8

Write-Host "==> Done (driver setup only)."
Get-ChildItem $OutDir | Format-Table Name, Length
