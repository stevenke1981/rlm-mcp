# Install rlm-mcp from GitHub Release (Windows x64).
#
# Usage:
#   irm https://raw.githubusercontent.com/stevenke1981/rlm-mcp/main/packaging/windows/install.ps1 | iex
#   $env:RLM_VERSION = "v0.1.6"; .\packaging\windows\install.ps1

param(
    [string]$Version = $(if ($env:RLM_VERSION) { $env:RLM_VERSION } else { "latest" }),
    [string]$Repo = $(if ($env:RLM_REPO) { $env:RLM_REPO } else { "stevenke1981/rlm-mcp" })
)

$ErrorActionPreference = "Stop"

$InstallDir = if ($env:RLM_INSTALL_DIR) { $env:RLM_INSTALL_DIR } else { "$env:USERPROFILE\.config\rlm-mcp\bin" }
$Target = "x86_64-pc-windows-msvc"

function Resolve-LatestVersion {
    param(
        [Parameter(Mandatory=$true)][string]$Repo
    )

    $headers = @{ "User-Agent" = "rlm-mcp-installer" }
    if ($env:GITHUB_TOKEN) {
        $headers["Authorization"] = "Bearer $env:GITHUB_TOKEN"
    } elseif ($env:GH_TOKEN) {
        $headers["Authorization"] = "Bearer $env:GH_TOKEN"
    }

    try {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers $headers
        if ($release.tag_name) {
            return [string]$release.tag_name
        }
    } catch {
        Write-Warning "GitHub API latest lookup failed; falling back to the public release redirect: $($_.Exception.Message)"
    }

    $latestUrl = "https://github.com/$Repo/releases/latest"
    $location = $null
    try {
        $response = Invoke-WebRequest -Uri $latestUrl -Headers @{ "User-Agent" = "rlm-mcp-installer" } -UseBasicParsing -MaximumRedirection 0 -ErrorAction Stop
        if ($response.Headers.Location) {
            $location = [string]$response.Headers.Location
        } elseif ($response.BaseResponse.ResponseUri) {
            $location = [string]$response.BaseResponse.ResponseUri.AbsoluteUri
        }
    } catch {
        $errorResponse = $_.Exception.Response
        if ($errorResponse) {
            try {
                $location = [string]$errorResponse.Headers.Location
            } catch {
                $location = $null
            }
            if (-not $location) {
                try {
                    $location = [string]$errorResponse.Headers["Location"]
                } catch {
                    $location = $null
                }
            }
        }
    }

    if ($location -and $location -notmatch '^https?://') {
        $location = [Uri]::new([Uri]$latestUrl, $location).AbsoluteUri
    }
    if ($location -match '/releases/tag/([^/?#]+)') {
        return $Matches[1]
    }

    throw "failed to resolve the latest GitHub Release for $Repo"
}

if ($Version -eq "latest") {
    $Version = Resolve-LatestVersion -Repo $Repo
}

$VersionNoV = $Version -replace '^v',''
$Archive = "rlm-mcp-$VersionNoV-$Target.zip"
$Base = "https://github.com/$Repo/releases/download/$Version"
$Url = "$Base/$Archive"
$Tmp = Join-Path $env:TEMP "rlm-mcp-install"
if (Test-Path -LiteralPath $Tmp) {
    Remove-Item -LiteralPath $Tmp -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $Tmp, $InstallDir | Out-Null

$ArchivePath = Join-Path $Tmp $Archive
Write-Host "Downloading $Url ..."
Invoke-WebRequest -Uri $Url -OutFile $ArchivePath

Write-Host "Verifying checksum ..."
$sums = Invoke-WebRequest -Uri "$Base/SHA256SUMS.txt" -UseBasicParsing
$checksumText = if ($sums.Content -is [byte[]]) {
    [Text.Encoding]::UTF8.GetString($sums.Content)
} else {
    [string]$sums.Content
}
$expected = ($checksumText -split "`r?`n" | Where-Object { $_ -match "\s+$([regex]::Escape($Archive))`$" } | ForEach-Object { ($_ -split '\s+')[0] } | Select-Object -First 1)
if (-not $expected) { throw "checksum for $Archive not found in SHA256SUMS.txt" }
$actual = (Get-FileHash -Path $ArchivePath -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $expected.ToLower()) {
    throw "checksum mismatch for $Archive (expected $expected, got $actual)"
}

Expand-Archive -Path $ArchivePath -DestinationPath $Tmp -Force
$Extracted = Get-ChildItem -Path $Tmp -Filter "rlm-mcp.exe" -Recurse | Select-Object -First 1
if (-not $Extracted) { throw "rlm-mcp.exe not found in archive" }

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

$InstalledBinary = Install-BinaryWithLockedFallback -Source $Extracted.FullName -InstallDir $InstallDir -VersionNoV $VersionNoV

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
if ($LASTEXITCODE -ne 0) { throw "rlm-mcp agent configuration failed" }

Write-Host ""
Write-Host "Installed rlm-mcp $Version -> $InstalledBinary" -ForegroundColor Green
Write-Host ('OpenCode and Codex MCP configured: ["{0}"]' -f $InstalledBinary)
if ($Skill) { Write-Host "Installed rlm skill for Codex, Claude Code, OpenCode, and agents." }
