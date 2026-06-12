# Install codebase-memory-rlm-mcp MCP server + rlm skill (Windows).

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$SkillName = "rlm"
$userHome = $env:USERPROFILE

Write-Host ""
Write-Host "Installing Python package..." -ForegroundColor DarkGray
python -m pip install -e $ScriptDir --quiet

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
Write-Host "Package installed: python -m codebase_memory_rlm_mcp" -ForegroundColor Green
Write-Host ""
Write-Host "Add to agent MCP config:" -ForegroundColor DarkGray
Write-Host '  command: ["python", "-m", "codebase_memory_rlm_mcp"]' -ForegroundColor DarkGray
Write-Host '  env: { "CBM_PROJECT": "your-project" }' -ForegroundColor DarkGray
Write-Host ""
Write-Host "Requires codebase-memory-mcp running separately." -ForegroundColor Yellow
Write-Host ""