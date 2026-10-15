# Baja libmpv + mpv + yt-dlp a vendor/ (Windows x86_64).
# Equivalente de fetch.sh para Windows; se corre en la CI antes de tauri build.
# Los binarios NO se versionan en git (ver vendor/.gitignore).
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

function Get-File($url, $out) {
  Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing
}

# --- mpv.exe: solo para leer las pistas antes de transmitir a la TV (probe.rs) ---
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

# --- libmpv (el reproductor, mpv_embed) ---
# build.rs arma mpv.lib desde las exportaciones del DLL, que se carga en
# diferido desde vendor/mpv-dev/ (mpv_embed/win32.rs).
if (-not (Test-Path "mpv-dev/libmpv-2.dll")) {
  Write-Host ">> libmpv"
  $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/zhongfly/mpv-winbuild/releases/latest" -Headers @{ "User-Agent" = "kutral" }
  $asset = $rel.assets |
    Where-Object { $_.name -match "^mpv-dev-x86_64-\d" -and $_.name -match "\.7z$" } |
    Select-Object -First 1
  if (-not $asset) { throw "no encontré el build de libmpv para Windows" }
  Write-Host "  bajando: $($asset.name)"
  Get-File $asset.browser_download_url "mpv-dev.7z"
  New-Item -ItemType Directory -Force -Path mpv-dev | Out-Null
  7z e -y "mpv-dev.7z" "libmpv-2.dll" -o"mpv-dev" | Out-Null
  Remove-Item "mpv-dev.7z"
}

# --- yt-dlp (trailers de YouTube: el iframe da error 153 en el webview) ---
if (-not (Test-Path "yt-dlp.exe")) {
  Write-Host ">> yt-dlp"
  Get-File "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" "yt-dlp.exe"
}

# Cortar acá y no en la máquina del usuario: sin libmpv no hay reproductor.
if (-not (Test-Path "mpv-dev/libmpv-2.dll")) { throw "falta mpv-dev/libmpv-2.dll" }

Write-Host ">> listo"
Get-ChildItem mpv.exe, yt-dlp.exe, mpv-dev/libmpv-2.dll
