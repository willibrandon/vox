#!/bin/bash
# build-app.sh — Create a macOS .app bundle for Vox.
#
# Prerequisites:
#   - Rust toolchain with cargo
#   - Xcode Command Line Tools
#   - Metal toolchain (xcodebuild -downloadComponent MetalToolchain)
#
# Usage:
#   ./packaging/macos/build-app.sh
#
# Output:
#   packaging/macos/output/Vox.app/

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
OUTPUT_DIR="$SCRIPT_DIR/output"
APP_BUNDLE="$OUTPUT_DIR/Vox.app"

echo "=== Vox .app Bundle Builder ==="

# Step 1: Build release binary
echo ""
echo "[1/4] Building release binary..."
cd "$REPO_ROOT"
cargo build --release -p vox --features vox_core/metal

BINARY="$REPO_ROOT/target/release/vox"
if [ ! -f "$BINARY" ]; then
    echo "ERROR: Release binary not found at $BINARY"
    exit 1
fi

BINARY_SIZE=$(stat -f%z "$BINARY" 2>/dev/null || stat -c%s "$BINARY")
BINARY_SIZE_MB=$(echo "scale=2; $BINARY_SIZE / 1048576" | bc)
echo "  Binary size: ${BINARY_SIZE_MB} MB"

# Step 2: Create .app bundle structure
echo ""
echo "[2/4] Creating .app bundle structure..."
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

# Step 3: Populate bundle
echo ""
echo "[3/4] Populating bundle..."

# Copy binary
cp "$BINARY" "$APP_BUNDLE/Contents/MacOS/vox"

# Copy Info.plist
cp "$SCRIPT_DIR/Info.plist" "$APP_BUNDLE/Contents/"

# Create PkgInfo
echo -n "APPL????" > "$APP_BUNDLE/Contents/PkgInfo"

# Copy icon if it exists (convert PNG to icns if needed)
ICON_SRC="$REPO_ROOT/assets/icons/app-icon.png"
if [ -f "$ICON_SRC" ]; then
    ICONSET="$OUTPUT_DIR/AppIcon.iconset"
    mkdir -p "$ICONSET"

    # Generate all required sizes from the source PNG
    for SIZE in 16 32 128 256 512; do
        sips -z $SIZE $SIZE "$ICON_SRC" --out "$ICONSET/icon_${SIZE}x${SIZE}.png" >/dev/null 2>&1
        DOUBLE=$((SIZE * 2))
        sips -z $DOUBLE $DOUBLE "$ICON_SRC" --out "$ICONSET/icon_${SIZE}x${SIZE}@2x.png" >/dev/null 2>&1
    done

    iconutil -c icns "$ICONSET" -o "$APP_BUNDLE/Contents/Resources/AppIcon.icns"
    rm -rf "$ICONSET"
    echo "  Icon: AppIcon.icns created"
else
    echo "  Warning: No icon found at $ICON_SRC (bundle will use default icon)"
fi

# Step 4: Code sign (ad-hoc if no identity provided)
echo ""
echo "[4/4] Code signing..."

IDENTITY="${CODESIGN_IDENTITY:--}"
codesign --force --sign "$IDENTITY" \
    --entitlements "$SCRIPT_DIR/entitlements.plist" \
    --options runtime \
    "$APP_BUNDLE"

echo ""
echo "=== Build Complete ==="
echo "  Binary: ${BINARY_SIZE_MB} MB"
echo "  Bundle: $APP_BUNDLE"
echo ""
echo "To test: open $APP_BUNDLE"
echo "To create DMG: ./packaging/macos/build-dmg.sh"
