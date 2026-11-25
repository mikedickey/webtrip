# Toolchain Requirements

Two unusual build requirements are worth explaining, since neither comes from the crate's
own source — WebTrip is plain stable Rust with no `#![feature(...)]` attributes.

## Why the nightly toolchain

WebTrip runs audio on real threads, which means SharedArrayBuffer, which means the WASM
module must be linked with `--shared-memory` and built with the `atomics` target feature.
The `wasm32-unknown-unknown` `std` that rustup ships is precompiled *without* atomics, so
linking against it fails:

```
rust-lld: error: --shared-memory is disallowed by std-....rcgu.o
  because it was not compiled with 'atomics' or 'bulk-memory' features.
```

The fix is to rebuild `std` from source with matching features, via `-Zbuild-std=std,panic_abort`.
That flag is nightly-only, and it is the sole reason for the pin.

**Blockers on removing nightly:**

| Flag | Used by | Blocks removal because |
|------|---------|------------------------|
| `-Zbuild-std=std,panic_abort` | `build:wasm`, `check`, `test:wasm`, and both wasm coverage scripts | Unstable ([cargo#8733](https://github.com/rust-lang/cargo/issues/8733)). Needed until upstream ships a prebuilt atomics-enabled wasm `std`. Also why `rust-toolchain.toml` requests the `rust-src` component. |
| `-Zno-profiler-runtime` | `build:wasm:coverage`, `coverage:wasm` | Unstable. Suppresses the LLVM profiler runtime, which doesn't build for wasm, so `minicov` can supply one instead. |
| `-Ctarget-feature=+atomics` | every wasm build | Not a hard blocker — stable accepts it, but warns that the feature is unstably supported. Moot while `-Zbuild-std` applies. |

The specific nightly version is not significant; nothing depends on a given nightly's
language features, so the pin can move freely. It exists only to match the build container
(see `RUST_NIGHTLY` in [containers/builder/Containerfile](../../containers/builder/Containerfile)) —
bump both together.

Native (non-WASM) work needs none of this: `npm run test` and `npm run coverage` pass no
`-Z` flags and would run on stable today.

## Why `--cfg=web_sys_unstable_apis`

This one is unrelated to the release channel — it's a plain `--cfg` and works fine on
stable. `web-sys` gates bindings for browser APIs that are not yet W3C-stable behind this
cfg, and WebTrip uses the WebTransport family (`WebTransport`, `WebTransportOptions`,
`WebTransportDatagramDuplexStream`, and friends) for its QUIC datagram transport. Without
the flag the build fails with `cannot find type 'WebTransport' in crate 'web_sys'`.

**Blocker on removing it:** the WebTransport bindings must graduate out of `web-sys`'s
unstable gate. Dropping the transport itself would also do it, but WebRTC data channels are
the fallback path, not a replacement — see [Browser Support](../browser-support.md).

Because both of the above are passed as flags rather than committed to `.cargo/config.toml`,
bare `cargo check` / `cargo test` invocations will fail. Always go through the npm scripts.
(`npm run check:core` is the one native, flag-free check: it builds the portable `webtrip-core`,
`webtrip-api` and `webtrip` facade crates with no browser dependencies, which is how the core/web split is enforced.)

