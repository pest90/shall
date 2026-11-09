#!/usr/bin/env bash
# Publishes the workspace to crates.io, dependencies first, and skips what
# is already there, so that a failed run of the release workflow can simply
# be run again. cargo waits for each crate to appear in the index.
#
# Without verifying: that compiles, so it would run the build scripts and
# macros of every dependency with the token in the environment. The workflow
# has verified the packages before, without the token.
#
#   CARGO_REGISTRY_TOKEN=... scripts/publish-crates.sh 0.3.0
set -euo pipefail
cd "$(dirname "$0")/.."

version=${1:?usage: publish-crates.sh <version>}
[ -n "${CARGO_REGISTRY_TOKEN:-}" ] || {
  echo "CARGO_REGISTRY_TOKEN is not set: add it as a repository secret" >&2
  exit 1
}

index=$(mktemp)
trap 'rm -f "$index"' EXIT

for crate in shall-core shall-plugin shall-report shall-cli; do
  # All these names have the index path sh/al/<name>.
  status=$(curl -sS -o "$index" -w '%{http_code}' "https://index.crates.io/sh/al/$crate")
  case $status in
    200) ;;
    404) : >"$index" ;; # never published
    *)
      echo "crates.io index answered $status for $crate" >&2
      exit 1
      ;;
  esac
  if grep -qF "\"vers\":\"$version\"" "$index"; then
    echo "$crate $version is already published"
  else
    cargo publish --locked --no-verify -p "$crate"
  fi
done
