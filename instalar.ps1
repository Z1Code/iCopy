$ErrorActionPreference = "Stop"
$raiz = $PSScriptRoot
& "$raiz\puente\build.ps1"
cargo build --release --manifest-path "$raiz\icopia\Cargo.toml"
if ($LASTEXITCODE -ne 0) { throw "cargo build falló" }
$dest = "$env:LOCALAPPDATA\Programs\icopia"
New-Item -ItemType Directory -Force $dest | Out-Null
Get-Process icopia-bandeja -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500
Copy-Item "$raiz\icopia\target\release\icopia.exe", "$raiz\icopia\target\release\icopia-bandeja.exe", "$raiz\puente\dist\puente-icloud.exe" $dest -Force
Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "icopia" -Value "`"$dest\icopia-bandeja.exe`""
Start-Process "$dest\icopia-bandeja.exe"
Write-Host "Instalado en $dest (arranca con Windows)."
