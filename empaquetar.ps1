# Compila el puente y los binarios en release y genera dist\iCopy-Setup-<versión>.exe (sin firmar).
$ErrorActionPreference = "Stop"
$raiz = $PSScriptRoot
$version = (Select-String -Path "$raiz\icopia\Cargo.toml" -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
& "$raiz\puente\build.ps1"
cargo build --release --manifest-path "$raiz\icopia\Cargo.toml"
if ($LASTEXITCODE -ne 0) { throw "cargo build falló" }
$iscc = (Get-Command iscc.exe -ErrorAction SilentlyContinue).Source
foreach ($c in "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe") {
    if (-not $iscc -and (Test-Path $c)) { $iscc = $c }
}
if (-not $iscc) { throw "No encuentro Inno Setup 6 (winget install JRSoftware.InnoSetup)" }
& $iscc "/DVersion=$version" "$raiz\instalador\icopy.iss"
if ($LASTEXITCODE -ne 0) { throw "Inno Setup falló" }
Write-Host "Listo: dist\iCopy-Setup-$version.exe"
