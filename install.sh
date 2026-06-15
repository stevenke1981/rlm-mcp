#!/usr/bin/env bash
# Install rlm-mcp MCP server + rlm skill.
# Idempotent: re-run safely.
#
# For Linux: all-in-one script with --from-source support.
# For macOS: release-only (use --from-source on the repo directly).
# For Windows: use install.ps1 instead.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

case "$(uname -s)" in
  Linux) exec bash "$SCRIPT_DIR/packaging/linux/install.sh" "$@" ;;
  Darwin)
    # macOS release install (source install via repo clone)
    exec bash "$SCRIPT_DIR/packaging/macos/install.sh" "$@"
    ;;
  *)
    echo "Unsupported OS: $(uname -s)" >&2
    echo "For Linux/macOS, use install.sh; for Windows, use install.ps1" >&2
    exit 1
    ;;
esac
