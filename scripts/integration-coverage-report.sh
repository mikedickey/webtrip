#!/usr/bin/env bash
# Turn the integration harness's coverage dump (coverage/integration.profraw,
# written by tests/integration/run.mjs when INTEGRATION_COVERAGE is set) into an
# lcov report Codecov can union with lcov.info / lcov.wasm.info.
#
# The instrumented module is built by `build:wasm:coverage`; the counters are
# captured from the live browser via the `__coverageDump` export. This step just
# merges the raw profile and exports lcov against the instrumented wasm binary.
set -euo pipefail

PROFRAW=coverage/integration.profraw
PROFDATA=coverage/integration.profdata
WASM=pkg/webtrip_bg.wasm
OUT=lcov.integration.info

[ -f "$PROFRAW" ] || { echo "error: $PROFRAW missing — run the harness with INTEGRATION_COVERAGE set" >&2; exit 1; }
[ -f "$WASM" ]    || { echo "error: $WASM missing — run `npm run build:wasm:coverage` first" >&2; exit 1; }

# Resolve llvm-profdata/llvm-cov: prefer the rustup llvm-tools-preview binaries
# (present in the CI build container), fall back to whatever is on PATH (e.g.
# Homebrew LLVM locally — `brew install llvm`, on PATH).
rustlib_bin="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin"
profdata="$rustlib_bin/llvm-profdata"; cov="$rustlib_bin/llvm-cov"
[ -x "$profdata" ] || profdata=llvm-profdata
[ -x "$cov" ]      || cov=llvm-cov

"$profdata" merge -sparse "$PROFRAW" -o "$PROFDATA"

# Drop the standard library / build-std and registry-dependency sources that
# `-Zbuild-std` pulls into the coverage map; keep only this crate's files.
"$cov" export --format=lcov \
  --instr-profile="$PROFDATA" \
  --ignore-filename-regex='(\.rustup/|/rustlib/|/registry/|/\.cargo/)' \
  "$WASM" > "$OUT"

echo "wrote $OUT ($(grep -c '^SF:' "$OUT") project files)"
