#!/bin/bash

set -e

TARGET_TRIPLE="x86_64-unknown-linux-gnu"

GIT_HASH=$(git rev-parse --short HEAD)
PACKAGE_NAME="librecard-linux_amd64-$GIT_HASH"
PACKAGE_DIR="target/$PACKAGE_NAME"

mkdir -p "$PACKAGE_DIR"

echo "Building release binary for $TARGET_TRIPLE..."
cargo build --release --target $TARGET_TRIPLE

echo "Assembling archive..."
cp "target/$TARGET_TRIPLE/release/librecard" "$PACKAGE_DIR"
cp "LICENSE" "$PACKAGE_DIR/LICENSE"
cp "README.md" "$PACKAGE_DIR/README.md"

TAR_PATH="target/$PACKAGE_NAME.tar.gz"
tar -czf "$TAR_PATH" -C "target" "$PACKAGE_NAME"

echo "Packed successfully at $TAR_PATH"

