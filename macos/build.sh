#!/bin/sh
# Buduje uniwersalny plik pluginu dla macOS (Apple Silicon + Intel).
# Wymaga Rusta: https://rustup.rs
# Wynik: dist/bin/opendeck-masterhub-mac (potem: python3 package.py)
set -e
cd "$(dirname "$0")/.."
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
mkdir -p dist/bin
lipo -create -output dist/bin/opendeck-masterhub-mac \
  target/aarch64-apple-darwin/release/opendeck-masterhub \
  target/x86_64-apple-darwin/release/opendeck-masterhub
echo "Gotowe: dist/bin/opendeck-masterhub-mac"
