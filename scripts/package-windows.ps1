# Builds an unsigned Windows zip (app + CLI) with SHA-256 checksums into dist\.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"').Matches[0].Groups[1].Value
cargo build --release --locked -p vault-desktop -p vault-cli
$name = "nilavar-$version-windows-x86_64"
$out = "dist\$name"
Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item target\release\nilavar.exe, target\release\nilavar-cli.exe, LICENSE, README.md $out
Copy-Item -Recurse docs "$out\docs"
Compress-Archive -Force -Path $out -DestinationPath "dist\$name-UNSIGNED.zip"
Get-FileHash "dist\$name-UNSIGNED.zip" -Algorithm SHA256 | ForEach-Object { "$($_.Hash.ToLower())  $name-UNSIGNED.zip" } | Out-File -Encoding ascii dist\SHA256SUMS
Get-Content dist\SHA256SUMS
