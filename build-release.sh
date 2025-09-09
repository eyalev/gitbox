#!/bin/bash
set -e

# Build release binary
echo "Building gitbox v0.2.0 for Ubuntu..."
cargo build --release

# Create release directory
mkdir -p release

# Copy binary
cp target/release/gitbox release/gitbox-linux-x86_64
chmod +x release/gitbox-linux-x86_64

# Create tarball
cd release
tar -czf gitbox-linux-x86_64.tar.gz gitbox-linux-x86_64

echo "Release binary created: release/gitbox-linux-x86_64.tar.gz"
echo "File size: $(du -h gitbox-linux-x86_64.tar.gz | cut -f1)"
echo ""
echo "To test the binary:"
echo "./release/gitbox-linux-x86_64 --help"