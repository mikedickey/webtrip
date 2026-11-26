#!/usr/bin/env bash
# Print the build-container tag (sha-<16 hex>) for the current checkout.
#
# The tag fingerprints every file whose change requires a new
# containers/builder image. CI builds and pushes ghcr.io/mikedickey/webtrip-build
# under this tag; the release workflow runs in the same image by recomputing it.
# Run from the repo root.
set -euo pipefail

cat containers/builder/Containerfile Cargo.toml crates/*/Cargo.toml Cargo.lock package.json \
    website/package.json website/package-lock.json \
  | sha256sum | cut -c1-16 | sed 's/^/sha-/'
