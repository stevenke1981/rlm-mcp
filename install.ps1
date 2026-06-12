# Install rlm-mcp MCP server + rlm skill (Windows).
# Idempotent: re-run safely; use -SkipBuild to copy existing release binary only.

param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$SkillName = "rlm"
$userHome = $env:USERPROFILE
$BinDir = Join-Path $userHome ".config\rlm-mcp\bin"

Write-Host ""
if ($SkipBuild) {
    Write-Host "Skipping build (-SkipBuild)..." -ForegroundColor DarkGray
} else {
    Write-Host "Building Rust release binary..." -ForegroundColor DarkGray
    Push-Location $ScriptDir
    cargo build --release
    Pop-Location
}

$Built = Join-Path $ScriptDir "target\release\rlm-mcp.exe"
if (-not (Test-Path $Built)) {
    throw "Build failed: $Built not found"
}

New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
Copy-Item $Built (Join-Path $BinDir "rlm-mcp.exe") -Force
Write-Host "  ✓ Binary → $BinDir\rlm-mcp.exe" -ForegroundColor Green

function Install-Skill {
    param([string]$TargetDir, [string]$Label)
    New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null
    Copy-Item (Join-Path $ScriptDir "SKILL.md") (Join-Path $TargetDir "SKILL.md") -Force
    Write-Host "  ✓ $Label" -ForegroundColor Green
    Write-Host "    → $TargetDir\SKILL.md" -ForegroundColor DarkGray
}

Write-Host "Installing rlm skill..." -ForegroundColor DarkGray
Install-Skill (Join-Path $userHome ".codex\skills\$SkillName") "Codex"
Install-Skill (Join-Path $userHome ".claude\skills\$SkillName") "Claude Code"
Install-Skill (Join-Path $userHome ".agents\skills\$SkillName") "OpenCode / Codex"
Install-Skill (Join-Path $userHome ".config\opencode\skills\$SkillName") "OpenCode native"

Write-Host ""
Write-Host "Binary installed: $BinDir\rlm-mcp.exe" -ForegroundColor Green
Write-Host ""
Write-Host "Add to agent MCP config (or copy from packaging\mcp\):" -ForegroundColor DarkGray
Write-Host "  command: [\"$BinDir\rlm-mcp.exe\"]" -ForegroundColor DarkGray
Write-Host "  server name: rlm-mcp" -ForegroundColor DarkGray
Write-Host "Standalone RLM — no CBM dependency. Optional dual setup: cbm-mcp/packaging/mcp/dual-servers.example.json" -ForegroundColor DarkGray
Write-Host ""