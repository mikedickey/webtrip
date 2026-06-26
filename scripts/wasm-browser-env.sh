#!/usr/bin/env bash
#
# Shared environment setup for the browser-driven WASM npm scripts
# (`test:wasm`, `coverage:wasm`). Both execute wasm in a headless Chrome driven
# by a chromedriver; `coverage:wasm` additionally compiles minicov's C profiler
# runtime to wasm32.
#
# Every block here is a NO-OP on the CI build container
# (`containers/build/Containerfile`), which already ships a wasm-capable clang, a
# version-matched wasm-bindgen-cli, and a matching chromedriver on PATH. The
# checks only kick in on developer machines (notably macOS) that lack one of
# those, where they either auto-configure the right tool or fail fast with an
# actionable message instead of an opaque downstream error.
#
# See docs/WASM_TESTING.md → "Local setup (macOS)".
#
# Usage: scripts/wasm-browser-env.sh <test|coverage> <command> [args...]
set -euo pipefail

mode="${1:?usage: wasm-browser-env.sh <test|coverage> <command...>}"
shift

note() { printf 'wasm-browser-env: %s\n' "$1" >&2; }
fail() { printf 'wasm-browser-env: error: %s\n' "$1" >&2; exit 1; }

# ── chromedriver (both modes) ────────────────────────────────────────────────
# wasm-pack / wasm-bindgen-test-runner honor $CHROMEDRIVER. Without it, wasm-pack
# auto-downloads a chromedriver whose major version may not match the installed
# Chrome; the mismatched driver is then SIGKILLed at session start. Prefer an
# explicit $CHROMEDRIVER, otherwise a developer-installed driver at
# ~/.local/bin/chromedriver (see docs for how to install a matching one).
if [ -z "${CHROMEDRIVER:-}" ] && [ -x "$HOME/.local/bin/chromedriver" ]; then
  CHROMEDRIVER="$HOME/.local/bin/chromedriver"
  export CHROMEDRIVER
  note "using CHROMEDRIVER=$CHROMEDRIVER"
fi

# ── coverage-only toolchain ──────────────────────────────────────────────────
if [ "$mode" = "coverage" ]; then
  # 1. A wasm-capable clang. minicov's build script compiles InstrProfiling.c for
  #    wasm32 via the `cc` crate, which invokes bare `clang`. Apple's
  #    /usr/bin/clang has no wasm32 target, so prefer Homebrew LLVM when present.
  if brew_llvm_bin="$(brew --prefix llvm 2>/dev/null)/bin" && [ -x "$brew_llvm_bin/clang" ]; then
    PATH="$brew_llvm_bin:$PATH"
    export PATH
    note "prepended $brew_llvm_bin to PATH for a wasm-capable clang"
  fi
  if ! printf 'int main(void){return 0;}' \
       | clang -x c --target=wasm32-unknown-unknown -nostdlibinc -c -o /dev/null - 2>/dev/null; then
    fail "the 'clang' on PATH cannot target wasm32 (needed by minicov for coverage).
       On macOS, Apple's clang won't work — run 'brew install llvm' and it will be
       picked up automatically here. See docs/WASM_TESTING.md."
  fi

  # 2. wasm-bindgen-test-runner must match the wasm-bindgen pinned in Cargo.lock.
  #    coverage:wasm uses the runner from PATH (unlike test:wasm, which wasm-pack
  #    version-matches); a mismatch fails with an opaque bindgen schema error.
  pin="$(awk '/^name = "wasm-bindgen"$/{f=1;next} f&&/^version = /{gsub(/[",]/,"",$3);print $3;exit}' Cargo.lock)"
  have="$(wasm-bindgen-test-runner --version 2>/dev/null | awk '{print $2}' || true)"
  if [ -n "$pin" ] && [ "$have" != "$pin" ]; then
    fail "wasm-bindgen-test-runner ${have:-<not found>} != pinned wasm-bindgen $pin.
       Run: cargo install wasm-bindgen-cli --version $pin --locked"
  fi
fi

exec "$@"
