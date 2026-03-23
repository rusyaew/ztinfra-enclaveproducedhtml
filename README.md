# ztinfra-enclaveproducedhtml

Canonical source repository for the real Nitro enclave workload used by ZTBrowser.

This repo owns only the measured enclave workload and its reproducible release process.
It does not own the parent proxy, browser extension, checker, or facts node.

## Release contract

Canonical releases publish:

- `ztbrowser-enclave.eif`
- `describe-eif.json`
- `provenance.json`
- `SHA256SUMS`

`ztbrowser` consumes those release artifacts directly for real AWS deploys.

## Local build prerequisites

- Linux host
- Docker
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
