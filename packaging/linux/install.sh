#!/usr/bin/env bash
# Install rlm-mcp on Linux — all-in-one installer.
#
# Default mode: download pre-built binary from GitHub Release.
#   curl -fsSL https://raw.githubusercontent.com/stevenke1981/rlm-mcp/main/packaging/linux/install.sh | bash
#   RLM_VERSION=v0.1.6 ./packaging/linux/install.sh
#
# Source mode: build & install from local checkout (requires Rust toolchain).
#   ./packaging/linux/install.sh --from-source
#   ./packaging/linux/install.sh --from-source --skip-build
#
# Both modes are idempotent — re-run safely.

set -euo pipefail

# ── Arg parse ─────────────────────────────────────────────────────
MODE="release"
SKIP_BUILD=0
REST_ARGS=()

for arg in "$@"; do
  case "$arg" in
    --from-source) MODE="source" ;;
    --skip-build)  SKIP_BUILD=1 ;;
    *)             REST_ARGS+=("$arg") ;;
  esac
done

set -- "${REST_ARGS[@]}"

# ── Config ────────────────────────────────────────────────────────
INSTALL_DIR="${RLM_INSTALL_DIR:-$HOME/.local/bin}"
CONFIG_DIR="${RLM_CONFIG_DIR:-$HOME/.config/rlm-mcp/bin}"
SKILL_NAME="rlm"

GREEN='\033[0;32m'
GRAY='\033[0;90m'
NC='\033[0m'

# ── Helpers ───────────────────────────────────────────────────────
install_skill() {
  local skill_file="$1"
  local targets=(
    "$HOME/.codex/skills/$SKILL_NAME"
    "$HOME/.claude/skills/$SKILL_NAME"
    "$HOME/.agents/skills/$SKILL_NAME"
    "$HOME/.config/opencode/skills/$SKILL_NAME"
  )
  for target in "${targets[@]}"; do
    mkdir -p "$target"
    cp "$skill_file" "$target/SKILL.md"
  done
  echo -e "${GREEN}  ✓ Skills installed${NC}"
  for target in "${targets[@]}"; do
    echo -e "${GRAY}    → ${target}/SKILL.md${NC}"
  done
}

# ── Source mode ───────────────────────────────────────────────────
if [ "$MODE" = "source" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

  echo ""
  echo -e "${GRAY}Source install from ${PROJECT_ROOT}${NC}"

  if [ "$SKIP_BUILD" -eq 1 ]; then
    echo -e "${GRAY}Skipping build (--skip-build)...${NC}"
  else
    echo -e "${GRAY}Building Rust release binary...${NC}"
    (cd "$PROJECT_ROOT" && cargo build --release)
  fi

  BUILT="$PROJECT_ROOT/target/release/rlm-mcp"
  if [ ! -f "$BUILT" ]; then
    echo "Build failed: $BUILT not found" >&2
    exit 1
  fi

  mkdir -p "$CONFIG_DIR"
  cp "$BUILT" "$CONFIG_DIR/rlm-mcp"
  chmod +x "$CONFIG_DIR/rlm-mcp"
  mkdir -p "$INSTALL_DIR"
  ln -sf "$CONFIG_DIR/rlm-mcp" "$INSTALL_DIR/rlm-mcp" 2>/dev/null || cp "$CONFIG_DIR/rlm-mcp" "$INSTALL_DIR/rlm-mcp"
  echo -e "${GREEN}  ✓ Binary → ${CONFIG_DIR}/rlm-mcp${NC}"

  "$CONFIG_DIR/rlm-mcp" install --json >/dev/null

  if [ -f "$PROJECT_ROOT/SKILL.md" ]; then
    echo -e "${GRAY}Installing rlm skill...${NC}"
    install_skill "$PROJECT_ROOT/SKILL.md"
  fi

  echo ""
  echo -e "${GREEN}Binary installed: ${CONFIG_DIR}/rlm-mcp${NC}"
  echo ""
  echo -e "${GRAY}OpenCode MCP configured automatically:${NC}"
  echo -e '  "rlm-mcp": {'
  echo -e '    "command": ["'"$CONFIG_DIR/rlm-mcp"'"],'
  echo -e '    "enabled": true'
  echo -e '  }'
  echo ""
  echo -e "${GRAY}Standalone RLM — no CBM dependency. Optional: cbm-mcp dual-servers.example.json${NC}"
  exit 0
fi

# ═══════════════════════════════════════════════════════════════════
# Release mode — download & install from GitHub
# ═══════════════════════════════════════════════════════════════════

REPO="${RLM_REPO:-stevenke1981/rlm-mcp}"
VERSION="${RLM_VERSION:-latest}"

arch="$(uname -m)"
case "$arch" in
  x86_64|amd64) TARGET="x86_64-unknown-linux-gnu" ;;
  aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
  *)
    echo "Unsupported Linux architecture: $arch" >&2
    echo "Supported: x86_64, aarch64" >&2
    echo "For other architectures, clone the repo and run with --from-source" >&2
    exit 1
    ;;
esac

# Resolve "latest" to actual version tag
if [ "$VERSION" = "latest" ]; then
  API="https://api.github.com/repos/${REPO}/releases/latest"
  token="${GITHUB_TOKEN:-${GH_TOKEN:-}}"
  if [ -n "$token" ]; then
    VERSION="$(curl -fsSL -H "User-Agent: rlm-mcp-installer" -H "Authorization: Bearer ${token}" "$API" | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/' || true)"
  else
    VERSION="$(curl -fsSL -H "User-Agent: rlm-mcp-installer" "$API" | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/' || true)"
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
expected="$(grep "${ARCHIVE}$" "$TMP/SHA256SUMS.txt" | awk '{print $1}')"
if [ -z "$expected" ]; then
  echo "checksum for ${ARCHIVE} not found in SHA256SUMS.txt" >&2
  exit 1
fi
actual="$(sha256sum "$TMP/${ARCHIVE}" | awk '{print $1}')"
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
  echo -e "${GRAY}Installing rlm skill...${NC}"
  install_skill "$skill"
fi

echo ""
echo "Installed rlm-mcp ${VERSION} -> ${CONFIG_DIR}/rlm-mcp"
echo "OpenCode MCP configured: [\"${CONFIG_DIR}/rlm-mcp\"]"
if [ -n "${skill:-}" ]; then
  echo "Installed rlm skill for Codex, Claude Code, OpenCode, and agents."
fi
