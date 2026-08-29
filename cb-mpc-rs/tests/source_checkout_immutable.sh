#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
temporary_root="$(mktemp -d /tmp/cbmpc-rust-checkout.XXXXXX)"
trap 'rm -rf "$temporary_root"' EXIT

git clone --quiet --no-hardlinks "$repo_root" "$temporary_root/checkout"
test ! -e "$temporary_root/checkout/lib"

CARGO_TARGET_DIR="$temporary_root/cargo-target" \
  cargo test \
  --manifest-path "$temporary_root/checkout/cb-mpc-rs/Cargo.toml" \
  -p cb-mpc

if [ -e "$temporary_root/checkout/lib" ]; then
  echo "cargo test created native archives in the source checkout:" >&2
  find "$temporary_root/checkout/lib" -type f -print >&2
  exit 1
fi

if [ -n "$(git -C "$temporary_root/checkout" status --porcelain --ignored)" ]; then
  echo "cargo test modified the source checkout:" >&2
  git -C "$temporary_root/checkout" status --short --ignored >&2
  exit 1
fi
