#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<USAGE
Usage: $0 --repo-url <repo-url> --ref <git-ref> --expected-provenance-url <url-or-path> [--expected-release-dir <dir-or-url>]
USAGE
}

REPO_URL=""
REF=""
EXPECTED_PROVENANCE_URL=""
EXPECTED_RELEASE_DIR=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo-url) REPO_URL="$2"; shift 2 ;;
    --ref) REF="$2"; shift 2 ;;
    --expected-provenance-url|--provenance-url|--release-url) EXPECTED_PROVENANCE_URL="$2"; shift 2 ;;
    --expected-release-dir|--release-dir) EXPECTED_RELEASE_DIR="$2"; shift 2 ;;
    *) usage; exit 1 ;;
  esac
done

[[ -n "$REPO_URL" && -n "$REF" && -n "$EXPECTED_PROVENANCE_URL" ]] || { usage; exit 1; }

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK_DIR="${WORK_DIR:-$ROOT_DIR/build/rebuild-verify}"
CHECKOUT_DIR="$WORK_DIR/repo"
EXPECTED_JSON="$WORK_DIR/expected-provenance.json"
EXPECTED_MANIFEST="$WORK_DIR/expected-release-manifest.json"
EXPECTED_COCO_INITDATA="$WORK_DIR/expected-coco-initdata.json"
EXPECTED_COCO_IMAGE_DIGEST="$WORK_DIR/expected-coco-image-digest.txt"
EXPECTED_COCO_IMAGE_REF="$WORK_DIR/expected-coco-image-ref.txt"
EXPECTED_COCO_OCI_DIGEST="$WORK_DIR/expected-coco-oci-manifest-digest.txt"
ACTUAL_JSON="$WORK_DIR/actual-provenance.json"
ACTUAL_MANIFEST="$WORK_DIR/actual-release-manifest.json"
OUTPUT_DIR="$WORK_DIR/output"

rm -rf "$WORK_DIR"
mkdir -p "$WORK_DIR" "$OUTPUT_DIR"

git clone "$REPO_URL" "$CHECKOUT_DIR" >/dev/null 2>&1
git -C "$CHECKOUT_DIR" checkout "$REF" >/dev/null 2>&1

copy_or_fetch() {
  local source="$1"
  local dest="$2"
  case "$source" in
    http://*|https://*) curl -fsSL "$source" -o "$dest" ;;
    *) cp "$source" "$dest" ;;
  esac
}

join_release_asset() {
  local base="$1"
  local name="$2"
  case "$base" in
    http://*|https://*) printf '%s/%s\n' "${base%/}" "$name" ;;
    *) printf '%s/%s\n' "${base%/}" "$name" ;;
  esac
}

copy_or_fetch "$EXPECTED_PROVENANCE_URL" "$EXPECTED_JSON"

if [[ -z "$EXPECTED_RELEASE_DIR" ]]; then
  case "$EXPECTED_PROVENANCE_URL" in
    http://*|https://*) EXPECTED_RELEASE_DIR="${EXPECTED_PROVENANCE_URL%/*}" ;;
    *) EXPECTED_RELEASE_DIR="$(cd "$(dirname "$EXPECTED_PROVENANCE_URL")" && pwd)" ;;
  esac
fi

copy_or_fetch "$(join_release_asset "$EXPECTED_RELEASE_DIR" release-manifest.json)" "$EXPECTED_MANIFEST"
copy_or_fetch "$(join_release_asset "$EXPECTED_RELEASE_DIR" coco-initdata.json)" "$EXPECTED_COCO_INITDATA"
copy_or_fetch "$(join_release_asset "$EXPECTED_RELEASE_DIR" coco-image-digest.txt)" "$EXPECTED_COCO_IMAGE_DIGEST"
copy_or_fetch "$(join_release_asset "$EXPECTED_RELEASE_DIR" coco-image-ref.txt)" "$EXPECTED_COCO_IMAGE_REF"
copy_or_fetch "$(join_release_asset "$EXPECTED_RELEASE_DIR" coco-oci-manifest-digest.txt)" "$EXPECTED_COCO_OCI_DIGEST"

INSTALL_ROOT="$WORK_DIR/nitro-cli"
NITRO_CLI_BIN="$CHECKOUT_DIR/tools/install-nitro-cli.sh"
NITRO_CLI_SOURCE_TAG="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["nitro_cli_source_tag"])' "$EXPECTED_JSON")"
INSTALL_ROOT="$INSTALL_ROOT" NITRO_CLI_SOURCE_TAG="$NITRO_CLI_SOURCE_TAG" "$NITRO_CLI_BIN" >/dev/null
export PATH="$INSTALL_ROOT/bin:$PATH"

IMAGE_TAG="rebuild-verify:$(date +%s)"
IMAGE_DIGEST_PLACEHOLDER="sha256:rebuild-verify-local"

pushd "$CHECKOUT_DIR" >/dev/null
IMAGE_TAG="$IMAGE_TAG" scripts/build-enclave.sh "$OUTPUT_DIR" >/dev/null
COCO_IMAGE_REF="$IMAGE_TAG" scripts/build-coco-image.sh "$OUTPUT_DIR" >/dev/null
DOCKER_VERSION="$(docker --version | sed 's/^Docker version //; s/,.*//')"
RUST_VERSION="$(rustc --version | awk '{print $2}')"
CARGO_VERSION="$(cargo --version | awk '{print $2}')"
python3 tools/generate_provenance.py \
  --workload-id "$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["workload_id"])' "$EXPECTED_JSON")" \
  --repo-url "$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["repo_url"])' "$EXPECTED_JSON")" \
  --project-repo-url "$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["project_repo_url"])' "$EXPECTED_JSON")" \
  --release-tag "$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["release_tag"])' "$EXPECTED_JSON")" \
  --commit-sha "$(git rev-parse HEAD)" \
  --oci-image-digest "$IMAGE_DIGEST_PLACEHOLDER" \
  --eif-path "$OUTPUT_DIR/ztbrowser-enclave.eif" \
  --describe-eif-path "$OUTPUT_DIR/describe-eif.json" \
  --release-url "$EXPECTED_PROVENANCE_URL" \
  --nitro-cli-version "$(nitro-cli --version | awk '{print $NF}')" \
  --nitro-cli-source-repo "https://github.com/aws/aws-nitro-enclaves-cli.git" \
  --nitro-cli-source-tag "$NITRO_CLI_SOURCE_TAG" \
  --docker-version "$DOCKER_VERSION" \
  --rust-version "$RUST_VERSION" \
  --cargo-version "$CARGO_VERSION" \
  --output-path "$ACTUAL_JSON"
python3 tools/generate_coco_artifacts.py \
  --service-config ztinfra-service.yaml \
  --release-id "$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["release_tag"])' "$EXPECTED_JSON")" \
  --coco-image-digest "$(cat "$EXPECTED_COCO_IMAGE_DIGEST")" \
  --initdata-path "$OUTPUT_DIR/coco-initdata.json" \
  --runtime-config-path "$OUTPUT_DIR/coco-runtime-config.json"
python3 tools/generate_release_manifest.py \
  --service-config ztinfra-service.yaml \
  --provenance "$ACTUAL_JSON" \
  --release-url "$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["release_url"])' "$EXPECTED_JSON")" \
  --coco-image-digest "$(cat "$EXPECTED_COCO_IMAGE_DIGEST")" \
  --coco-initdata-path "$OUTPUT_DIR/coco-initdata.json" \
  --manifest-path "$ACTUAL_MANIFEST" \
  --coco-runtime-config-path "$OUTPUT_DIR/coco-runtime-config.json"
popd >/dev/null

python3 - <<'PY' "$EXPECTED_JSON" "$ACTUAL_JSON" "$EXPECTED_MANIFEST" "$ACTUAL_MANIFEST" "$EXPECTED_COCO_INITDATA" "$OUTPUT_DIR/coco-initdata.json" "$EXPECTED_COCO_IMAGE_DIGEST" "$EXPECTED_COCO_IMAGE_REF" "$EXPECTED_COCO_OCI_DIGEST" "$OUTPUT_DIR/coco-oci-manifest-digest.txt"
import json, sys
from pathlib import Path
import hashlib

expected = json.load(open(sys.argv[1]))
actual = json.load(open(sys.argv[2]))
expected_manifest = json.load(open(sys.argv[3]))
actual_manifest = json.load(open(sys.argv[4]))
keys = [
  'workload_id', 'repo_url', 'project_repo_url', 'release_tag', 'commit_sha',
  'eif_sha256', 'describe_eif_sha256', 'pcr0', 'pcr1', 'pcr2', 'pcr8', 'nitro_cli_source_tag'
]
mismatches = []
for key in keys:
    if expected.get(key) != actual.get(key):
        mismatches.append((key, expected.get(key), actual.get(key)))

def sha256_file(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()

def coco_identity(manifest):
    for entry in manifest['accepted_realizations']:
        if entry.get('platform') == 'aws_coco_snp':
            return entry['identity']['value'], entry.get('lowered_from')
    raise SystemExit('missing aws_coco_snp realization')

expected_coco, expected_lowered_from = coco_identity(expected_manifest)
actual_coco, actual_lowered_from = coco_identity(actual_manifest)
expected_initdata_hash = sha256_file(sys.argv[5])
actual_initdata_hash = sha256_file(sys.argv[6])
expected_coco_digest = Path(sys.argv[7]).read_text().strip()
expected_coco_ref = Path(sys.argv[8]).read_text().strip()
expected_oci_manifest_digest = Path(sys.argv[9]).read_text().strip()
actual_oci_manifest_digest = Path(sys.argv[10]).read_text().strip()

checks = {
    'coco_deployment_image_digest_bound': expected_coco['image_digest'] == actual_coco['image_digest'] == expected_coco_digest,
    'coco_deployment_image_ref_present': bool(expected_coco_ref),
    'coco_oci_manifest_rebuild_digest': expected_oci_manifest_digest == actual_oci_manifest_digest,
    'coco_initdata_hash': expected_coco['initdata_hash'] == actual_coco['initdata_hash'] == expected_initdata_hash == actual_initdata_hash,
    'coco_lowered_from_source_container': expected_lowered_from and actual_lowered_from and expected_lowered_from.get('type') == actual_lowered_from.get('type') == 'source_container',
}
for key, ok in checks.items():
    if not ok:
        mismatches.append((key, True, False))

if mismatches:
    print(json.dumps({'ok': False, 'mismatches': mismatches, 'coco_checks': checks}, indent=2))
    raise SystemExit(1)
print(json.dumps({'ok': True, 'checked_keys': keys, 'coco_checks': checks}, indent=2))
PY
