# Install rlm-mcp MCP server + rlm skill from GitHub Release by default (Windows).
# Idempotent: re-run safely; use -FromSource only when developing this checkout.

param(
    [string]$Version = $(if ($env:RLM_VERSION) { $env:RLM_VERSION } else { "latest" }),
    [switch]$FromSource,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

if (-not $FromSource) {
    $Script = Join-Path $ScriptDir "packaging\windows\install.ps1"
    & $Script -Version $Version
    exit $LASTEXITCODE
}

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

function Install-BinaryWithLockedFallback {
    param(
        [Parameter(Mandatory=$true)][string]$Source,
        [Parameter(Mandatory=$true)][string]$InstallDir,
        [Parameter(Mandatory=$true)][string]$VersionNoV
    )
    $stable = Join-Path $InstallDir "rlm-mcp.exe"
    $versioned = Join-Path $InstallDir "rlm-mcp-$VersionNoV.exe"
    try {
        Copy-Item -LiteralPath $Source -Destination $stable -Force -ErrorAction Stop
        return $stable
    } catch {
        $message = $_.Exception.Message
        if ($message -notmatch "being used by another process|cannot access the file|access.*denied") {
            throw
        }
        Copy-Item -LiteralPath $Source -Destination $versioned -Force -ErrorAction Stop
        Write-Warning "Stable binary is locked; installed side-by-side: $versioned"
        return $versioned
    }
}

New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
$CargoToml = Get-Content -Raw (Join-Path $ScriptDir "Cargo.toml")
$CargoVersion = if ($CargoToml -match '(?m)^version\s*=\s*"([^"]+)"') { $Matches[1] } else { "dev" }
$InstalledBinary = Install-BinaryWithLockedFallback -Source $Built -InstallDir $BinDir -VersionNoV $CargoVersion
Write-Host "  ✓ Binary → $InstalledBinary" -ForegroundColor Green
& $InstalledBinary install --json
if ($LASTEXITCODE -ne 0) { throw "rlm-mcp agent configuration failed" }

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
Write-Host "Binary installed: $InstalledBinary" -ForegroundColor Green
Write-Host ""
Write-Host "OpenCode and Codex MCP configured automatically." -ForegroundColor DarkGray
Write-Host ('  command: ["{0}"]' -f $InstalledBinary) -ForegroundColor DarkGray
Write-Host "  server name: rlm-mcp" -ForegroundColor DarkGray
Write-Host "Standalone RLM — no CBM dependency. Optional dual setup: cbm-mcp/packaging/mcp/dual-servers.example.json" -ForegroundColor DarkGray
Write-Host ""
