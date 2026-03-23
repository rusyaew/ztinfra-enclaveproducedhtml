#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT_DIR/build}"
IMAGE_TAG="${IMAGE_TAG:-ztinfra-enclaveproducedhtml:latest}"
EIF_PATH="$OUT_DIR/ztbrowser-enclave.eif"
DESCRIBE_PATH="$OUT_DIR/describe-eif.json"

mkdir -p "$OUT_DIR"

echo "[1/3] Building enclave image: $IMAGE_TAG"
docker build -t "$IMAGE_TAG" "$ROOT_DIR"

echo "[2/3] Building EIF: $EIF_PATH"
nitro-cli build-enclave --docker-uri "$IMAGE_TAG" --output-file "$EIF_PATH"

echo "[3/3] Describing EIF: $DESCRIBE_PATH"
nitro-cli describe-eif --eif-path "$EIF_PATH" >"$DESCRIBE_PATH"

echo "Built EIF at $EIF_PATH"
echo "Saved measurements to $DESCRIBE_PATH"
