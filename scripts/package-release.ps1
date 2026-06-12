# Package local release binary + SHA256 checksum (mirrors CI layout).
param(
    [string]$Version = "",
    [string]$Target = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

if (-not $Version) {
    $Version = (Select-String -Path (Join-Path $Root "Cargo.toml") -Pattern '^version' | Select-Object -First 1).Line -replace '.*"(.*)".*', '$1'
}
if (-not $Target) {
    $Target = (rustc -vV | Select-String '^host:' | ForEach-Object { $_ -replace '^host: ', '' })
}

$BinName = if ($Target -match 'windows') { "rlm-mcp.exe" } else { "rlm-mcp" }
$Candidates = @(
    (Join-Path $Root "target\$Target\release\$BinName"),
    (Join-Path $Root "target\release\$BinName")
)
$Built = $Candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $Built) {
    throw "Binary not found. Run: cargo build --release [--target $Target]"
}

$Dist = Join-Path $Root "dist"
$StagingName = "rlm-mcp-$Version-$Target"
$Staging = Join-Path $Dist $StagingName
if (Test-Path $Staging) { Remove-Item -Recurse -Force $Staging }
New-Item -ItemType Directory -Force -Path $Staging | Out-Null

Copy-Item $Built $Staging
Copy-Item (Join-Path $Root "README.md") $Staging
Copy-Item (Join-Path $Root "packaging\release\LICENSE-MIT") (Join-Path $Staging "LICENSE")
Copy-Item (Join-Path $Root "packaging\mcp") (Join-Path $Staging "mcp-templates") -Recurse
Copy-Item (Join-Path $Root "SKILL.md") $Staging
Set-Content -Path (Join-Path $Staging "RELEASE.txt") -Value "rlm-mcp $Version ($Target)"

$Archive = Join-Path $Dist "$StagingName.zip"
if (Test-Path $Archive) { Remove-Item -Force $Archive }
Compress-Archive -Path $Staging -DestinationPath $Archive

$Hash = (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLower()
"$Hash  $(Split-Path -Leaf $Archive)" | Set-Content "$Archive.sha256"
Write-Host "Packaged: $Archive"
Get-Content "$Archive.sha256"