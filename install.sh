#!/usr/bin/env bash
# Install codebase-memory-rlm-mcp MCP server + rlm skill.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILL_NAME="rlm"

GREEN='\033[0;32m'
GRAY='\033[0;90m'
NC='\033[0m'

echo ""
echo -e "${GRAY}Installing Python package...${NC}"
pip install -e "$SCRIPT_DIR" --quiet

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
echo -e "${GREEN}Package installed: python -m codebase_memory_rlm_mcp${NC}"
echo ""
echo -e "${GRAY}Add to your agent MCP config:${NC}"
echo -e '${GRAY}  "codebase-memory-rlm-mcp": {'
echo -e '${GRAY}    "command": ["python", "-m", "codebase_memory_rlm_mcp"],'
echo -e '${GRAY}    "env": { "CBM_PROJECT": "your-project" }'
echo -e '${GRAY}  }${NC}'
echo ""
echo -e "${GRAY}Requires codebase-memory-mcp running separately.${NC}"
echo ""