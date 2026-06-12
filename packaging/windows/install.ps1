# Install rlm-mcp from GitHub Release (Windows x64).
#
# Usage:
#   irm https://raw.githubusercontent.com/stevenke1981/rlm-mcp/main/packaging/windows/install.ps1 | iex
#   $env:RLM_VERSION = "v0.1.2"; .\packaging\windows\install.ps1

param(
    [string]$Version = $(if ($env:RLM_VERSION) { $env:RLM_VERSION } else { "latest" }),
    [string]$Repo = $(if ($env:RLM_REPO) { $env:RLM_REPO } else { "stevenke1981/rlm-mcp" })
)

$ErrorActionPreference = "Stop"

$InstallDir = if ($env:RLM_INSTALL_DIR) { $env:RLM_INSTALL_DIR } else { "$env:USERPROFILE\.config\rlm-mcp\bin" }
$Target = "x86_64-pc-windows-msvc"

if ($Version -eq "latest") {
    $rel = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
    $Version = $rel.tag_name
}

$Archive = "rlm-mcp-$($Version -replace '^v','')-$Target.zip"
$Base = "https://github.com/$Repo/releases/download/$Version"
$Url = "$Base/$Archive"
$Tmp = Join-Path $env:TEMP "rlm-mcp-install"
New-Item -ItemType Directory -Force -Path $Tmp, $InstallDir | Out-Null

$ArchivePath = Join-Path $Tmp $Archive
Write-Host "Downloading $Url ..."
Invoke-WebRequest -Uri $Url -OutFile $ArchivePath

Write-Host "Verifying checksum ..."
$sums = Invoke-WebRequest -Uri "$Base/SHA256SUMS.txt" -UseBasicParsing
$expected = ($sums.Content -split "`n" | Where-Object { $_ -match "\s+$([regex]::Escape($Archive))`$" } | ForEach-Object { ($_ -split '\s+')[0] } | Select-Object -First 1)
if (-not $expected) { throw "checksum for $Archive not found in SHA256SUMS.txt" }
$actual = (Get-FileHash -Path $ArchivePath -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $expected.ToLower()) {
    throw "checksum mismatch for $Archive (expected $expected, got $actual)"
}

Expand-Archive -Path $ArchivePath -DestinationPath $Tmp -Force
$Extracted = Get-ChildItem -Path $Tmp -Filter "rlm-mcp.exe" -Recurse | Select-Object -First 1
if (-not $Extracted) { throw "rlm-mcp.exe not found in archive" }
Copy-Item $Extracted.FullName (Join-Path $InstallDir "rlm-mcp.exe") -Force
$InstalledBinary = Join-Path $InstallDir "rlm-mcp.exe"

$Skill = Get-ChildItem -Path $Tmp -Filter "SKILL.md" -Recurse | Select-Object -First 1
if ($Skill) {
    $skillTargets = @(
        "$env:USERPROFILE\.codex\skills\rlm",
        "$env:USERPROFILE\.claude\skills\rlm",
        "$env:USERPROFILE\.agents\skills\rlm",
        "$env:USERPROFILE\.config\opencode\skills\rlm"
    )
    foreach ($target in $skillTargets) {
        New-Item -ItemType Directory -Force -Path $target | Out-Null
        Copy-Item $Skill.FullName (Join-Path $target "SKILL.md") -Force
    }
}

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$InstallDir;$userPath", "User")
    $env:Path = "$InstallDir;$env:Path"
    Write-Host "Added $InstallDir to user PATH"
}

& $InstalledBinary install --json
if ($LASTEXITCODE -ne 0) { throw "rlm-mcp OpenCode configuration failed" }

Write-Host ""
Write-Host "Installed rlm-mcp $Version -> $InstalledBinary" -ForegroundColor Green
Write-Host "OpenCode MCP configured: [\"$InstalledBinary\"]"
if ($Skill) { Write-Host "Installed rlm skill for Codex, Claude Code, OpenCode, and agents." }
