$ErrorActionPreference = "Stop"

Write-Host "Installing the Windows packaging tool..."
python -m pip install -r requirements-windows.txt

if (Test-Path dist) { Remove-Item dist -Recurse -Force }
if (Test-Path build) { Remove-Item build -Recurse -Force }
New-Item -ItemType Directory -Force -Path dist, dist\models, dist\games, dist\data | Out-Null

$common = @("--clean", "--noconfirm", "--onefile")
pyinstaller @common --name Engine --console Engine.py
pyinstaller @common --windowed --name Selfplay Selfplay.py
pyinstaller @common --windowed --name Arena Arena.py

Copy-Item models\*.nnue dist\models -ErrorAction SilentlyContinue
Write-Host "Created dist\Engine.exe, dist\Selfplay.exe, and dist\Arena.exe"