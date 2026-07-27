#!/usr/bin/env bash
# Run tests and clippy for the whole workspace.

set -e

cargo test --workspace
cargo clippy --workspace --all-targets --tests -- -D warnings
