#!/usr/bin/env bash
# Install rlm-mcp MCP server + rlm skill.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILL_NAME="rlm"
BIN_DIR="$HOME/.local/bin"
CONFIG_BIN="$HOME/.config/rlm-mcp/bin"

GREEN='\033[0;32m'
GRAY='\033[0;90m'
NC='\033[0m'

echo ""
echo -e "${GRAY}Building Rust release binary...${NC}"
(cd "$SCRIPT_DIR" && cargo build --release)

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
echo -e "${GRAY}Add to agent MCP config (or copy from packaging/mcp/):${NC}"
echo -e '${GRAY}  "rlm-mcp": {'
echo -e '${GRAY}    "command": ["'"$CONFIG_BIN/rlm-mcp"'"],'
echo -e '${GRAY}    "enabled": true'
echo -e '${GRAY}  }${NC}'
echo ""
echo -e "${GRAY}Standalone RLM — no CBM dependency. Optional: cbm-mcp dual-servers.example.json${NC}"
echo ""