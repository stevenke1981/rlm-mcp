#!/usr/bin/env bash
# Install rlm-mcp from GitHub Release (macOS Apple Silicon).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/stevenke1981/rlm-mcp/main/packaging/macos/install.sh | bash
#   RLM_VERSION=v0.1.6 ./packaging/macos/install.sh

set -euo pipefail

REPO="${RLM_REPO:-stevenke1981/rlm-mcp}"
VERSION="${RLM_VERSION:-latest}"
INSTALL_DIR="${RLM_INSTALL_DIR:-$HOME/.local/bin}"
CONFIG_DIR="${RLM_CONFIG_DIR:-$HOME/.config/rlm-mcp/bin}"

arch="$(uname -m)"
case "$arch" in
  arm64) TARGET="aarch64-apple-darwin" ;;
  *)
    echo "Unsupported macOS architecture in current release workflow: $arch" >&2
    exit 1
    ;;
esac

if [ "$VERSION" = "latest" ]; then
  API="https://api.github.com/repos/${REPO}/releases/latest"
  token="${GITHUB_TOKEN:-${GH_TOKEN:-}}"
  if [ -n "$token" ]; then
    VERSION="$(curl -fsSL -H "User-Agent: rlm-mcp-installer" -H "Authorization: Bearer ${token}" "$API" | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": "([^"]+)".*/\1/' || true)"
  else
    VERSION="$(curl -fsSL -H "User-Agent: rlm-mcp-installer" "$API" | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": "([^"]+)".*/\1/' || true)"
  fi
  if [ -z "$VERSION" ]; then
    latest_url="$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest" || true)"
    VERSION="$(printf '%s\n' "$latest_url" | sed -E 's#^.*/releases/tag/([^/?#]+).*$#\1#')"
    if [ -z "$VERSION" ] || [ "$VERSION" = "$latest_url" ]; then
      echo "failed to resolve the latest GitHub Release for ${REPO}" >&2
      exit 1
    fi
  fi
fi

VERSION_NO_V="${VERSION#v}"
ARCHIVE="rlm-mcp-${VERSION_NO_V}-${TARGET}.tar.gz"
BASE="https://github.com/${REPO}/releases/download/${VERSION}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Downloading ${BASE}/${ARCHIVE} ..."
curl -fsSL "${BASE}/${ARCHIVE}" -o "$TMP/${ARCHIVE}"

echo "Verifying checksum ..."
curl -fsSL "${BASE}/SHA256SUMS.txt" -o "$TMP/SHA256SUMS.txt"
expected="$(grep " ${ARCHIVE}$" "$TMP/SHA256SUMS.txt" | awk '{print $1}')"
if [ -z "$expected" ]; then
  echo "checksum for ${ARCHIVE} not found in SHA256SUMS.txt" >&2
  exit 1
fi
actual="$(shasum -a 256 "$TMP/${ARCHIVE}" | awk '{print $1}')"
if [ "$actual" != "$expected" ]; then
  echo "checksum mismatch for ${ARCHIVE}" >&2
  exit 1
fi

tar -xzf "$TMP/${ARCHIVE}" -C "$TMP"

mkdir -p "$INSTALL_DIR" "$CONFIG_DIR"
found="$(find "$TMP" -type f -name rlm-mcp | head -n 1)"
install -m 755 "$found" "$CONFIG_DIR/rlm-mcp"
ln -sf "$CONFIG_DIR/rlm-mcp" "$INSTALL_DIR/rlm-mcp"
"$CONFIG_DIR/rlm-mcp" install --json >/dev/null

skill="$(find "$TMP" -type f -name SKILL.md | head -n 1 || true)"
if [ -n "$skill" ]; then
  for target in \
    "$HOME/.codex/skills/rlm" \
    "$HOME/.claude/skills/rlm" \
    "$HOME/.agents/skills/rlm" \
    "$HOME/.config/opencode/skills/rlm"; do
    mkdir -p "$target"
    cp "$skill" "$target/SKILL.md"
  done
fi

echo ""
echo "Installed rlm-mcp ${VERSION} -> ${CONFIG_DIR}/rlm-mcp"
echo "OpenCode MCP configured: [\"${CONFIG_DIR}/rlm-mcp\"]"
if [ -n "${skill:-}" ]; then
  echo "Installed rlm skill for Codex, Claude Code, OpenCode, and agents."
fi
