#!/bin/sh
# Local equivalent of .github/workflows/ci.yml — run before pushing.
set -e

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
