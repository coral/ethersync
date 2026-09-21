#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
buf lint
buf format --diff --exit-code
buf build -o /tmp/ethersync-descriptor.bin
python3 scripts/generate_docs.py --check
python3 scripts/smoke_examples.py
