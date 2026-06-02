# Baja RetroArch + cores libretro + mpv a vendor/ (Windows x86_64).
# Equivalente de fetch.sh para Windows; se corre en la CI antes de tauri build.
# Los binarios NO se versionan en git (ver vendor/.gitignore).
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

function Get-File($url, $out) {
  Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing
}

New-Item -ItemType Directory -Force -Path cores | Out-Null

# --- cores (.dll) ---
$coresBase = "https://buildbot.libretro.com/nightly/windows/x86_64/latest"
foreach ($c in @("fceumm", "snes9x", "mgba", "gambatte", "melonds")) {
  if (Test-Path "cores/${c}_libretro.dll") { Write-Host "  ya está: $c"; continue }
  Write-Host "  bajando core: $c"
  Get-File "$coresBase/${c}_libretro.dll.zip" "cores/$c.zip"
  Expand-Archive -Force "cores/$c.zip" -DestinationPath cores
  Remove-Item "cores/$c.zip"
}

# --- RetroArch (.exe + dlls; sin assets para no inflar el bundle) ---
if (-not (Test-Path "retroarch.exe")) {
  Write-Host ">> RetroArch"
  Get-File "https://buildbot.libretro.com/nightly/windows/x86_64/RetroArch.7z" "ra.7z"
  7z x -y "ra.7z" -o"ra_tmp" | Out-Null
  $sub = Get-ChildItem -Directory "ra_tmp" | Select-Object -First 1
  if (-not $sub) { $sub = Get-Item "ra_tmp" }
  Copy-Item -Force "$($sub.FullName)\retroarch.exe" .
  Get-ChildItem "$($sub.FullName)\*.dll" | Copy-Item -Force -Destination .
  Remove-Item -Recurse -Force "ra_tmp", "ra.7z"
}

# --- mpv (.exe self-contained) ---
if (-not (Test-Path "mpv.exe")) {
  Write-Host ">> mpv"
  $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/zhongfly/mpv-winbuild/releases/latest" -Headers @{ "User-Agent" = "kutral" }
  $asset = $rel.assets |
    Where-Object { $_.name -match "^mpv-x86_64-\d" -and $_.name -match "\.7z$" -and $_.name -notmatch "dev|debug|v3" } |
    Select-Object -First 1
  if (-not $asset) { throw "no encontré el build de mpv para Windows" }
  Write-Host "  bajando: $($asset.name)"
  Get-File $asset.browser_download_url "mpv.7z"
  7z e -y "mpv.7z" "mpv.exe" -o"." | Out-Null
  Remove-Item "mpv.7z"
}

Write-Host ">> listo"
Get-ChildItem retroarch.exe, mpv.exe
Get-ChildItem cores
