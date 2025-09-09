#!/bin/bash
set -e

echo "Building gitbox v0.2.0 for Ubuntu compatibility..."

# Build standard binary
echo "Building standard x86_64 binary..."
cargo build --release

# Build musl binary (statically linked, no glibc dependency)
echo "Building musl binary (statically linked)..."
if ! rustup target list --installed | grep -q x86_64-unknown-linux-musl; then
    echo "Installing musl target..."
    rustup target add x86_64-unknown-linux-musl
fi

# Build with vendored OpenSSL for musl
echo "Building musl binary with vendored OpenSSL..."
cargo build --release --target x86_64-unknown-linux-musl --features vendored-openssl

# Create release directory
mkdir -p release

# Copy binaries
cp target/release/gitbox release/gitbox-linux-x86_64
cp target/x86_64-unknown-linux-musl/release/gitbox release/gitbox-linux-x86_64-musl
chmod +x release/gitbox-linux-x86_64 release/gitbox-linux-x86_64-musl

# Create tarballs
cd release

echo "Creating tarballs..."
tar -czf gitbox-linux-x86_64.tar.gz gitbox-linux-x86_64
tar -czf gitbox-linux-x86_64-musl.tar.gz gitbox-linux-x86_64-musl

echo ""
echo "✅ Binaries created successfully!"
echo "📦 Standard binary: gitbox-linux-x86_64.tar.gz ($(du -h gitbox-linux-x86_64.tar.gz | cut -f1)) - For newer systems"
echo "📦 Musl binary: gitbox-linux-x86_64-musl.tar.gz ($(du -h gitbox-linux-x86_64-musl.tar.gz | cut -f1)) - For Ubuntu 22.04+"
echo ""
echo "🧪 To test the binaries:"
echo "./gitbox-linux-x86_64 --help          # Standard (newer glibc)"
echo "./gitbox-linux-x86_64-musl --help     # Musl (all Linux systems)"
echo ""
echo "💡 Use the musl binary if you get glibc version errors!"