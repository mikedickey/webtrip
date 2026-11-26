# Releasing

Pushing a `vX.Y.Z` tag runs [`.github/workflows/release.yml`](../../.github/workflows/release.yml), which:

1. **Checks the tag.** The tag without its `v` must equal `[workspace.package] version` in `Cargo.toml`, and the tagged commit must be on `main`.
2. **Tags the production image.** It waits (up to 45 min) for main CI to push `ghcr.io/mikedickey/webtrip:sha-<commit>`, then adds `:X.Y.Z` and, for non-pre-releases, `:X.Y`. Main CI pushes that image only after the build and integration jobs pass, so this step also blocks publishing a commit whose CI failed.
3. **Publishes the crates.** `npm run publish:crates` publishes `webtrip-core`, `webtrip-api`, `webtrip-web` and `webtrip` in dependency order. It authenticates with crates.io [trusted publishing](https://crates.io/docs/trusted-publishing), so no token is stored. docs.rs builds the docs for each new version on its own; the `[package.metadata.docs.rs]` sections set the wasm target and flags it needs.

## Cutting a release

1. Bump `version` under `[workspace.package]` in `Cargo.toml`. Also bump the `version = "…"` on the path dependencies in `crates/web/Cargo.toml` and `crates/webtrip/Cargo.toml`.
2. Optionally run `npm run publish:crates -- --dry-run`, which packages and verifies all four crates without uploading.
3. Merge to `main` and push the tag: `git tag vX.Y.Z && git push origin vX.Y.Z`. You don't need to wait for main CI, because the release waits for its image.

A crates.io version can't be overwritten, only yanked. If the crates job fails after uploading some crates, fix the problem in a new patch version.

## One-time setup

Trusted publishing can only be configured for a crate that already exists on crates.io, so the first version has to be published by hand:

1. `cargo login` with a crates.io API token, then `npm run publish:crates`.
2. On crates.io, open each of the four crates and go to **Settings → Trusted Publishing**. Add a GitHub publisher: owner `mikedickey`, repository `webtrip`, workflow `release.yml`.
3. Push the tag for that version. The image gets tagged as usual. The crates job fails because the version is already on crates.io, which is expected this one time.
