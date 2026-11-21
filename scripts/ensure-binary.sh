#!/usr/bin/env bash
# Print the absolute path of a working hkchinabus binary.
set -euo pipefail

if [ -n "${HKCHINABUS_BIN:-}" ] && [ -x "$HKCHINABUS_BIN" ]; then
  printf '%s\n' "$HKCHINABUS_BIN"
  exit 0
fi

if command -v hkchinabus >/dev/null 2>&1; then
  command -v hkchinabus
  exit 0
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [ -f "$ROOT/Cargo.toml" ] && command -v cargo >/dev/null 2>&1; then
  (cd "$ROOT" && cargo build --release --locked >/dev/null)
  BIN="$ROOT/target/release/hkchinabus"
  if [ -x "$BIN" ]; then
    printf '%s\n' "$BIN"
    exit 0
  fi
fi

echo "hkchinabus: no binary found and no Cargo build available" >&2
echo "Set HKCHINABUS_BIN, put hkchinabus on PATH, or build with: cargo build --release --locked" >&2
exit 1
