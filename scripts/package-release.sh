#!/usr/bin/env bash
# Package local release binaries + SHA256 checksums (mirrors CI layout).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-$(grep '^version' "$ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')}"
TARGET="${2:-$(rustc -vV | sed -n 's/^host: //p')}"
PROFILE="${CARGO_PROFILE:-release}"

BIN_NAME="rlm-mcp"
[[ "$TARGET" == *windows* ]] && BIN_NAME="rlm-mcp.exe"

BUILT=""
for candidate in "$ROOT/target/$TARGET/$PROFILE/$BIN_NAME" "$ROOT/target/$PROFILE/$BIN_NAME"; do
  if [[ -f "$candidate" ]]; then
    BUILT="$candidate"
    break
  fi
done
if [[ -z "$BUILT" ]]; then
  echo "Binary not found. Run: cargo build --release [--target $TARGET]" >&2
  exit 1
fi

DIST="$ROOT/dist"
STAGING="$DIST/rlm-mcp-${VERSION}-${TARGET}"
rm -rf "$STAGING"
mkdir -p "$STAGING"

cp "$BUILT" "$STAGING/"
cp "$ROOT/README.md" "$STAGING/"
cp "$ROOT/packaging/release/LICENSE-MIT" "$STAGING/LICENSE"
cp -r "$ROOT/packaging/mcp" "$STAGING/mcp-templates"
cp "$ROOT/SKILL.md" "$STAGING/"
echo "rlm-mcp ${VERSION} (${TARGET})" > "$STAGING/RELEASE.txt"

mkdir -p "$DIST"
ARCHIVE="$DIST/rlm-mcp-${VERSION}-${TARGET}.tar.gz"
tar -czf "$ARCHIVE" -C "$DIST" "rlm-mcp-${VERSION}-${TARGET}"
shasum -a 256 "$ARCHIVE" > "${ARCHIVE}.sha256"

echo "Packaged: $ARCHIVE"
cat "${ARCHIVE}.sha256"