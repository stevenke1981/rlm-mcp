#!/usr/bin/env bash
# Install rlm-mcp MCP server + rlm skill from GitHub Release by default.
# Idempotent: re-run safely. Use --from-source only when developing this checkout.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ "${1:-}" != "--from-source" ]]; then
  case "$(uname -s)" in
    Linux) exec bash "$SCRIPT_DIR/packaging/linux/install.sh" "$@" ;;
    Darwin) exec bash "$SCRIPT_DIR/packaging/macos/install.sh" "$@" ;;
    *)
      echo "Unsupported OS for release install: $(uname -s)" >&2
      echo "Use --from-source for source installs." >&2
      exit 1
      ;;
  esac
fi
shift

SKILL_NAME="rlm"
BIN_DIR="$HOME/.local/bin"
CONFIG_BIN="$HOME/.config/rlm-mcp/bin"
SKIP_BUILD=0
if [[ "${1:-}" == "--skip-build" ]]; then
  SKIP_BUILD=1
fi

GREEN='\033[0;32m'
GRAY='\033[0;90m'
NC='\033[0m'

echo ""
if [[ "$SKIP_BUILD" -eq 1 ]]; then
  echo -e "${GRAY}Skipping build (--skip-build)...${NC}"
else
  echo -e "${GRAY}Building Rust release binary...${NC}"
  (cd "$SCRIPT_DIR" && cargo build --release)
fi

BUILT="$SCRIPT_DIR/target/release/rlm-mcp"
if [[ ! -f "$BUILT" ]]; then
  echo "Build failed: $BUILT not found" >&2
  exit 1
fi

mkdir -p "$CONFIG_BIN"
cp "$BUILT" "$CONFIG_BIN/rlm-mcp"
chmod +x "$CONFIG_BIN/rlm-mcp"
mkdir -p "$BIN_DIR"
ln -sf "$CONFIG_BIN/rlm-mcp" "$BIN_DIR/rlm-mcp" 2>/dev/null || cp "$CONFIG_BIN/rlm-mcp" "$BIN_DIR/rlm-mcp"
echo -e "${GREEN}  ✓ Binary → ${CONFIG_BIN}/rlm-mcp${NC}"
"$CONFIG_BIN/rlm-mcp" install --json >/dev/null

install_skill() {
  local target_dir="$1"
  local label="$2"
  mkdir -p "$target_dir"
  cp "$SCRIPT_DIR/SKILL.md" "$target_dir/SKILL.md"
  echo -e "${GREEN}  ✓ ${label}${NC}"
  echo -e "${GRAY}    → ${target_dir}/SKILL.md${NC}"
}

echo -e "${GRAY}Installing rlm skill...${NC}"
install_skill "$HOME/.codex/skills/$SKILL_NAME" "Codex (~/.codex/skills/)"
install_skill "$HOME/.claude/skills/$SKILL_NAME" "Claude Code (~/.claude/skills/)"
install_skill "$HOME/.agents/skills/$SKILL_NAME" "OpenCode / Codex (~/.agents/skills/)"
install_skill "$HOME/.config/opencode/skills/$SKILL_NAME" "OpenCode (~/.config/opencode/skills/)"

echo ""
echo -e "${GREEN}Binary installed: ${CONFIG_BIN}/rlm-mcp${NC}"
echo ""
echo -e "${GRAY}OpenCode MCP configured automatically:${NC}"
echo -e '${GRAY}  "rlm-mcp": {'
echo -e '${GRAY}    "command": ["'"$CONFIG_BIN/rlm-mcp"'"],'
echo -e '${GRAY}    "enabled": true'
echo -e '${GRAY}  }${NC}'
echo ""
echo -e "${GRAY}Standalone RLM — no CBM dependency. Optional: cbm-mcp dual-servers.example.json${NC}"
echo ""
