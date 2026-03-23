#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT_DIR/build}"
IMAGE_TAG="${IMAGE_TAG:-ztinfra-enclaveproducedhtml:latest}"
EIF_PATH="$OUT_DIR/ztbrowser-enclave.eif"
DESCRIBE_PATH="$OUT_DIR/describe-eif.json"
NITRO_LOG_DIR="/var/log/nitro_enclaves"
NITRO_LOG_FILE="$NITRO_LOG_DIR/nitro_enclaves.log"

mkdir -p "$OUT_DIR"

ensure_nitro_log_path() {
  # nitro-cli build-enclave writes to a fixed log path even outside a Nitro parent
  # instance. GitHub runners and clean Linux hosts often do not have that path.
  if mkdir -p "$NITRO_LOG_DIR" 2>/dev/null; then
    touch "$NITRO_LOG_FILE" 2>/dev/null || true
    return 0
  fi

  if command -v sudo >/dev/null 2>&1; then
    sudo mkdir -p "$NITRO_LOG_DIR"
    sudo touch "$NITRO_LOG_FILE"
    sudo chown "$(id -u):$(id -g)" "$NITRO_LOG_DIR" "$NITRO_LOG_FILE"
    return 0
  fi

  cat >&2 <<EOF
Failed to prepare Nitro CLI log path at $NITRO_LOG_FILE.
nitro-cli build-enclave requires that path to exist and be writable.
Create it manually or rerun in an environment with sudo available.
EOF
  exit 1
}

ensure_nitro_log_path

echo "[1/3] Building enclave image: $IMAGE_TAG"
docker build -t "$IMAGE_TAG" "$ROOT_DIR"

echo "[2/3] Building EIF: $EIF_PATH"
nitro-cli build-enclave --docker-uri "$IMAGE_TAG" --output-file "$EIF_PATH"

echo "[3/3] Describing EIF: $DESCRIBE_PATH"
nitro-cli describe-eif --eif-path "$EIF_PATH" >"$DESCRIBE_PATH"

echo "Built EIF at $EIF_PATH"
echo "Saved measurements to $DESCRIBE_PATH"
