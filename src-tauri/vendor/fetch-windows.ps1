# Baja mpv + yt-dlp + uosc a vendor/ (Windows x86_64).
# Equivalente de fetch.sh para Windows; se corre en la CI antes de tauri build.
# Los binarios NO se versionan en git (ver vendor/.gitignore).
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

function Get-File($url, $out) {
  Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing
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

# --- yt-dlp (trailers de YouTube: el iframe da error 153 en el webview) ---
if (-not (Test-Path "yt-dlp.exe")) {
  Write-Host ">> yt-dlp"
  Get-File "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" "yt-dlp.exe"
}

# --- uosc (UI de mpv) ---
# En Windows el reproductor es mpv.exe como proceso aparte (player.rs vive tras
# #[cfg(not(target_os = "linux"))]) y toda su interfaz la pone uosc. mpv.conf
# apaga el OSC y la osd-bar nativos justamente porque uosc los reemplaza, así
# que sin este paso el build sale SIN NINGÚN control: ni iconos ni barra de
# tiempo. Faltó desde siempre; el equivalente en fetch.sh ya lo hacía.
# El zip trae scripts/uosc y fonts/ (uosc dibuja sus iconos con su propia
# fuente), los dos dentro de mpv-config/ porque ambos están en .gitignore.
if (-not (Test-Path "mpv-config/scripts/uosc")) {
  Write-Host ">> uosc"
  $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/tomasklaen/uosc/releases/latest" -Headers @{ "User-Agent" = "kutral" }
  $asset = $rel.assets | Where-Object { $_.name -eq "uosc.zip" } | Select-Object -First 1
  if (-not $asset) { throw "no encontré uosc.zip en el release de uosc" }
  Write-Host "  bajando: $($asset.name)"
  New-Item -ItemType Directory -Force -Path mpv-config | Out-Null
  Get-File $asset.browser_download_url "uosc.zip"
  Expand-Archive -Force "uosc.zip" -DestinationPath mpv-config
  Remove-Item "uosc.zip"
} else {
  Write-Host ">> uosc: ya está"
}

# Cortar acá y no en la máquina del usuario. Que uosc faltara en silencio es
# exactamente lo que dejó a Windows sin controles de reproductor.
foreach ($req in @("mpv-config/scripts/uosc", "mpv-config/fonts/uosc_icons.otf")) {
  if (-not (Test-Path $req)) { throw "falta ${req}: el build saldría sin UI de reproductor" }
}

Write-Host ">> listo"
Get-ChildItem mpv.exe, yt-dlp.exe
