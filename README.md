# ztinfra-enclaveproducedhtml

Canonical source repository for the real Nitro enclave workload used by ZTBrowser.

This repo owns only the measured enclave workload and its reproducible release process.
It does not own the parent proxy, browser extension, checker, or facts node.

## Release contract

Canonical releases publish:

- `ztbrowser-enclave.eif`
- `describe-eif.json`
- `provenance.json`
- `release-manifest.json`
- `coco-runtime-config.json`
- `coco-initdata.json`
- `coco-image-digest.txt`
- `coco-image-ref.txt`
- `coco-oci-manifest-digest.txt`
- `coco-workload.oci.tar`
- `SHA256SUMS`

`ztbrowser` consumes those release artifacts directly for real AWS deploys.

The release model has one canonical source container build context. Nitro and
CoCo are lowerings from that same container:

- Nitro lowering: source container -> `ztbrowser-enclave.eif` -> PCR identity.
- CoCo lowering: source container image + external Init-Data -> `image_digest + initdata_hash` identity.

## Attested response signing

The workload generates an ephemeral P-256 response signing key inside the trusted
runtime. Attestation responses bind the public key into TEE evidence:

- Nitro puts the canonical public JWK in the NSM attestation `public_key` field
  and the SHA-256 response-key binding hash in `user_data`.
- AWS CoCo requests AA evidence with `runtime_data` set to the SHA-512
  response-key binding hash.

The common attestation envelope exposes `claims.response_signing_key`,
`claims.response_signing_key_id`, and `claims.response_signing_key_binding`.
When a request includes `X-ZT-Challenge`, the workload signs the exact response
body and returns `X-ZT-Signature-Version`, `X-ZT-Key-Id`,
`X-ZT-Content-Digest`, `X-ZT-Signature`, `X-ZT-Signed-At`, and
`X-ZT-Challenge`. Requests without `X-ZT-Challenge` remain valid unsigned
responses for normal browser visits and health checks.

GitHub release assets are distribution artifacts, not trust roots. A reviewer can
rebuild the source container and lowerings locally, compare `SHA256SUMS`,
`provenance.json`, `release-manifest.json`, `coco-oci-manifest-digest.txt`, and
the `sha256(coco-initdata.json)` value, then decide whether the GitHub-produced
assets match the deterministic local build. `coco-image-digest.txt` is the
deployment registry digest for the CoCo workload image; `coco-oci-manifest-digest.txt`
is the local OCI rebuild comparison point; `coco-image-ref.txt` is the pullable
registry reference for deployment.

## Local build prerequisites

- Linux host
- Docker
- Docker Buildx for the CoCo OCI rebuild artifact
- Nitro CLI installed on `PATH`
- Rust toolchain available on the host if you want to run the rebuild verifier locally

Build locally:

```bash
scripts/build-enclave.sh
```

Rebuild and compare against a published release manifest:

```bash
tools/rebuild-verify.sh \
  --repo-url https://github.com/rusyaew/ztinfra-enclaveproducedhtml \
  --ref v0.1.0 \
  --expected-provenance-url https://github.com/rusyaew/ztinfra-enclaveproducedhtml/releases/download/v0.1.0/provenance.json
```

## GitHub workflows

- `release-enclave.yml` builds the canonical EIF release and opens a PR against `rusyaew/ztbrowser` to update facts.
- `rebuild-verify.yml` reruns the public rebuild flow from `repo_url + ref + provenance_url` and publishes the comparison output as an artifact.

Required secret for the release workflow:

- `ZTINFRA_FACTS_PR_TOKEN`: token with permission to push a branch and open a PR against `rusyaew/ztbrowser`
