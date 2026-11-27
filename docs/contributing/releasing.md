# Releasing

Pushing a `vX.Y.Z` tag runs [`.github/workflows/release.yml`](../../.github/workflows/release.yml), which:

1. **Checks the tag.** The tag without its `v` must equal `[workspace.package] version` in `Cargo.toml`, and the tagged commit must be on `main`.
2. **Tags the production image.** It waits (up to 45 min) for main CI to push `ghcr.io/mikedickey/webtrip:sha-<commit>`, then adds `:X.Y.Z` and, for non-pre-releases, `:X.Y`. Main CI pushes that image only after the build and integration jobs pass, so this step also blocks publishing a commit whose CI failed.
3. **Publishes the crates.** `npm run publish:crates` publishes `webtrip-core`, `webtrip-api`, `webtrip-web` and `webtrip` in dependency order. It authenticates with crates.io [trusted publishing](https://crates.io/docs/trusted-publishing), so no token is stored. docs.rs builds the docs for each new version on its own; the `[package.metadata.docs.rs]` sections set the wasm target and flags it needs.
4. **Publishes the npm package.** In parallel with the crates, it runs `npm run build:wasm` and publishes `pkg/` to npm as [`webtrip`](https://www.npmjs.com/package/webtrip) (`build:wasm` renames wasm-pack's `webtrip-web` to `webtrip`). This also uses [trusted publishing](https://docs.npmjs.com/trusted-publishers), which attaches provenance. Pre-release versions get the `next` dist-tag instead of `latest`.

## Cutting a release

1. Run `scripts/bump-version.sh X.Y.Z`. It sets `version` under `[workspace.package]` in `Cargo.toml` and on the path dependencies between the crates, and updates `Cargo.lock`.
2. Optionally run `npm run publish:crates -- --dry-run`, which packages and verifies all four crates without uploading, and `npm run build:wasm && npm publish ./pkg --dry-run` to check the npm package.
3. Merge to `main` and push the tag: `git tag vX.Y.Z && git push origin vX.Y.Z`. You don't need to wait for main CI, because the release waits for its image.

A crates.io or npm version can't be overwritten (crates can only be yanked, npm versions deprecated). If either publish job fails after uploading some crates, fix the problem in a new patch version.

## One-time setup

Trusted publishing can only be configured for a crate that already exists on crates.io, so the first version has to be published by hand:

1. `cargo login` with a crates.io API token, then `npm run publish:crates`.
2. On crates.io, open each of the four crates and go to **Settings → Trusted Publishing**. Add a GitHub publisher: owner `mikedickey`, repository `webtrip`, workflow `release.yml`.
3. Push the tag for that version. The image gets tagged as usual. The crates job fails because the version is already on crates.io, which is expected this one time.

npm works the same way: trusted publishing can only be configured once the package exists.

1. `npm login`, then `npm run build:wasm && npm publish ./pkg`.
2. On npmjs.com, open `webtrip` and go to **Settings → Trusted Publisher**. Add GitHub Actions: owner `mikedickey`, repository `webtrip`, workflow `release.yml`.
3. When you push the tag for that version, the npm job fails because the version is already published, which is expected this one time.
