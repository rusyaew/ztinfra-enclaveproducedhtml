#!/usr/bin/env bash
set -euo pipefail

NITRO_CLI_SOURCE_REPO="${NITRO_CLI_SOURCE_REPO:-https://github.com/aws/aws-nitro-enclaves-cli.git}"
NITRO_CLI_SOURCE_TAG="${NITRO_CLI_SOURCE_TAG:-v1.4.4}"
INSTALL_ROOT="${INSTALL_ROOT:-$(pwd)/.nitro-cli}"
BIN_DIR="$INSTALL_ROOT/bin"
BUILD_DIR="$INSTALL_ROOT/src/aws-nitro-enclaves-cli"

if [[ -x "$BIN_DIR/nitro-cli" ]]; then
  echo "$BIN_DIR/nitro-cli"
  exit 0
fi

mkdir -p "$INSTALL_ROOT/src" "$BIN_DIR"

if [[ ! -d "$BUILD_DIR/.git" ]]; then
  git clone --depth 1 --branch "$NITRO_CLI_SOURCE_TAG" "$NITRO_CLI_SOURCE_REPO" "$BUILD_DIR"
fi

pushd "$BUILD_DIR" >/dev/null
make nitro-cli
if [[ -x build/nitro_cli/release/nitro-cli ]]; then
  cp build/nitro_cli/release/nitro-cli "$BIN_DIR/nitro-cli"
elif [[ -x build/bin/nitro-cli ]]; then
  cp build/bin/nitro-cli "$BIN_DIR/nitro-cli"
else
  echo "Could not find built nitro-cli binary" >&2
  exit 1
fi
popd >/dev/null

echo "$BIN_DIR/nitro-cli"
