# Container Images

Container image definitions live under `containers/<name>/Containerfile`, one
directory per image. This document covers both images:

- [`containers/builder/Containerfile`](#build-image-containersbuilder) — the
  **build** image (toolchain for CI and agent environments)
- [`containers/webtrip/Containerfile`](#production-image-containerswebtrip) —
  the **production** image (node static server serving webtrip.dev)

# Build image (containers/builder)

`containers/builder/Containerfile` defines a base image for building and testing
WebTrip. It contains **no application source** — the repo is mounted or cloned
into `/workspace` at run time — but it does pre-build all dependencies so builds
start warm. This keeps the image reusable as both a CI/CD build environment and a
base for AI agent development environments.

All images are built with the **repo root as the build context** (so the
dependency pre-build can `COPY` the lockfiles); the root `.dockerignore` /
`.containerignore` apply to every image unless an image ships its own
`Containerfile.dockerignore` next to its Containerfile (the production image
does — see below).

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
npm run build:container:builder

# or directly, from the repo root
podman build -t webtrip/webtrip-builder -f containers/builder/Containerfile .
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

# Production image (containers/webtrip)

`containers/webtrip/Containerfile` is a `node:22-slim` image running
`website/server.js` — the same static server `npm run serve` uses locally and
the integration-test harness (`tests/integration/run.mjs`) drives in CI —
serving the built website over TLS on port 443, with port 80 redirecting to
https. Production, local serving, and CI all exercise the same code path.

The jacktrip hub is **not** part of this image. Run it separately from
`jacktrip/jacktrip:edge` — the image the integration tests use; see
`tests/integration/docker-compose.integration.yml` for the option set webtrip
is tested against.

## Building the image

The image `COPY`s the server (`website/server.js` + `website/serve-common.cjs`,
node builtins only — no npm install) and prebuilt artifacts (`website/dist/`
and repo-root `pkg/`), so build the artifacts first; the image build fails
fast if they're missing:

```bash
npm run build
npm run build:container:webtrip

# or with podman (the sibling dockerignore must be passed explicitly)
podman build --ignorefile containers/webtrip/Containerfile.dockerignore \
    -t webtrip/webtrip -f containers/webtrip/Containerfile .
```

The root ignore files exclude `pkg/` and `dist/`, so this image ships its own
`containers/webtrip/Containerfile.dockerignore` (a whitelist of exactly what
the Containerfile COPYs). Docker picks it up automatically as
`<Dockerfile-path>.dockerignore` — this requires BuildKit, the default since
Docker 23; the classic builder would ignore it and the `COPY pkg/` would fail
loudly.

CI builds and pushes this image to `ghcr.io/mikedickey/webtrip` (`:latest` and
`:sha-<commit>`) on every push to main, after the build and integration jobs
pass (the `webtrip-image` job in `.github/workflows/ci.yml`).

## Running

The image **requires** a TLS key and full-chain certificate mounted at:

- `/certs/server.crt` — full chain
- `/certs/server.key`

```bash
docker run -d --name webtrip -p 80:80 -p 443:443 \
  -v "$CERT_DIR/fullchain.pem:/certs/server.crt:ro" \
  -v "$CERT_DIR/privkey.pem:/certs/server.key:ro" \
  webtrip/webtrip
```

Notes:

- Plain port mapping works everywhere, including macOS — no host networking,
  `--privileged`, or systemd involved.
- `PORT` overrides the https port (`-e PORT=8443`); the port-80 listener only
  issues redirects to it.
- On SELinux hosts add `,z` to the cert volume mounts (prefer `,z` over `:Z`,
  which relabels the host files).
- A root-owned `0600` key is fine: node runs as root (the node image default),
  which is also what lets it bind 80/443.
