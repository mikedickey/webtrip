# Build Container

Container image definitions live under `containers/<name>/Containerfile`, one
directory per image. This document covers `containers/build/Containerfile`, the
**build** image.

`containers/build/Containerfile` defines a base image for building and testing
WebTrip. It contains **no application source** — the repo is mounted or cloned
into `/workspace` at run time — but it does pre-build all dependencies so builds
start warm. This keeps the image reusable as both a CI/CD build environment and a
base for AI agent development environments.

All images are built with the **repo root as the build context** (so the
dependency pre-build can `COPY` the lockfiles); the root `.dockerignore` /
`.containerignore` apply to every image.

## What's inside

- Debian `bookworm-slim` base
- Rust nightly (pinned, see below) with the `wasm32-unknown-unknown` target and
  `rust-src` (required for the `-Zbuild-std` flag in `npm run build:wasm`)
- `wasm-pack` (pinned prebuilt binary) + a cached `wasm-bindgen` CLI
- Node.js LTS + npm (repo requires Node >= 18)
- Native build toolchain (`build-essential`, `pkg-config`) — needed because
  `npm run test` compiles `cargo test` for the host, not WASM
- Dev essentials: `git`, `curl`, `ca-certificates`, `ripgrep`, `jq`
- **Pre-built dependency cache**: every crate dependency and the `-Zbuild-std`
  std are compiled into `/opt/cargo-target` during the image build, so a build
  against mounted source only recompiles the `webtrip` crate itself.

## Warm dependency cache

The image pre-compiles dependencies via a stub crate (step 5 of the
`Containerfile`), driven through the project's own npm scripts so the
rustflags/`build-std` flags match exactly what a real build uses — otherwise the
cache would silently miss. Key points:

- `CARGO_TARGET_DIR` is fixed to `/opt/cargo-target` and the source mounts at
  `/workspace`. **Do not override `CARGO_TARGET_DIR`** at run time or the warm
  cache is bypassed.
- The stub's own `webtrip` artifacts are removed after prewarm (`cargo clean -p
  webtrip`) so the local crate always rebuilds from real source. Without this,
  cargo's mtime-based freshness check could treat older-mtime mounted source as
  already-built and run stale stub code.
- The cache depends on `Cargo.toml` + `Cargo.lock`. When dependencies change,
  rebuild the image to refresh it; until then cargo just recompiles whatever
  drifted (it degrades gracefully, it never produces wrong output).
- `XDG_CACHE_HOME` (wasm-pack/wasm-bindgen) and `npm_config_cache` (npm) point
  at a shared, writable `/opt/cache` so any uid (root in CI, non-root in agent
  runtimes) can use the caches without a writable `HOME`.

## Lockfiles

`Cargo.lock` and `package-lock.json` **must be committed** (they are, and are no
longer git-ignored). The dependency pre-build and `npm ci` both rely on them, and
the image build `COPY`s `Cargo.lock` from the build context.

## Building the image

```bash
# convenience recipe (builds + tags webtrip/webtrip-builder)
npm run build:container

# or directly, from the repo root
podman build -t webtrip/webtrip-builder -f containers/build/Containerfile .
```

Pinned versions are `--build-arg`s (`RUST_NIGHTLY`, `NODE_MAJOR`,
`WASM_PACK_VERSION`) so CI can bump them without editing the file.

## Version pinning

`RUST_NIGHTLY` in `Containerfile` and `channel` in `rust-toolchain.toml` **must
stay in sync**. They are different toolchains to rustup (`nightly` vs
`nightly-2026-06-13`); if they diverge, rustup downloads a second nightly at
build time inside the container and the baked-in pin is wasted. Bump both
together.

## Using it

```bash
# CI: build + test against a mounted checkout
podman run --rm -v "$PWD":/workspace:Z ghcr.io/mikedickey/webtrip-build:latest \
    bash -lc "npm ci && npm run build && npm run test"
```

The toolchain lives under `/usr/local/{cargo,rustup}` and is world-usable, so the
container works whether run as root (typical for CI) or as a non-root uid
(common for agent runtimes).
