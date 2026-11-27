#!/usr/bin/env bash
# Set the release version: `[workspace.package] version`, the version on every
# path dependency between the workspace crates, and Cargo.lock.
#
# Only edits files; committing and tagging are described in
# docs/contributing/releasing.md.
#
# Usage: scripts/bump-version.sh X.Y.Z[-pre]
set -euo pipefail

fail() { printf 'bump-version: error: %s\n' "$1" >&2; exit 1; }

[ $# -eq 1 ] || fail "usage: scripts/bump-version.sh X.Y.Z[-pre]"
version=$1
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] \
  || fail "'$version' is not a version like 1.2.3 or 1.2.3-rc.1 (no leading 'v')"

cd "$(dirname "$0")/.."

NEW=$version perl -pi -e '
  $in_package = $_ eq "[workspace.package]\n" if /^\[/;
  s/^version = "[^"]*"/version = "$ENV{NEW}"/ if $in_package;
' Cargo.toml

NEW=$version perl -pi -e '
  s/\bversion = "[^"]*"/version = "$ENV{NEW}"/ if /^webtrip.*\bpath = /;
' crates/*/Cargo.toml

# Rewrites only the workspace crates' entries; other dependencies stay pinned.
cargo update --workspace

git diff --stat -- Cargo.toml Cargo.lock 'crates/*/Cargo.toml'
