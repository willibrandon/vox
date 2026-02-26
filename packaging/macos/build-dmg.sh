#!/bin/bash
# build-dmg.sh — Create a macOS DMG disk image containing Vox.app.
#
# Prerequisites:
#   - Run build-app.sh first to create the .app bundle
#   - hdiutil (macOS built-in)
#
# Usage:
#   ./packaging/macos/build-dmg.sh
#
# Output:
#   packaging/macos/output/Vox.dmg

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OUTPUT_DIR="$SCRIPT_DIR/output"
APP_BUNDLE="$OUTPUT_DIR/Vox.app"
DMG_PATH="$OUTPUT_DIR/Vox.dmg"
DMG_STAGING="$OUTPUT_DIR/dmg-staging"

echo "=== Vox DMG Builder ==="

# Verify .app bundle exists
if [ ! -d "$APP_BUNDLE" ]; then
    echo "ERROR: Vox.app not found at $APP_BUNDLE"
    echo "Run build-app.sh first."
    exit 1
fi

# Step 1: Prepare DMG staging area
echo ""
echo "[1/3] Preparing staging area..."
rm -rf "$DMG_STAGING"
mkdir -p "$DMG_STAGING"

# Copy .app bundle into staging
cp -R "$APP_BUNDLE" "$DMG_STAGING/"

# Create Applications symlink for drag-to-install
ln -s /Applications "$DMG_STAGING/Applications"

# Step 2: Create DMG
echo ""
echo "[2/3] Creating DMG..."
rm -f "$DMG_PATH"

hdiutil create \
    -volname "Vox" \
    -srcfolder "$DMG_STAGING" \
    -ov \
    -format UDZO \
    "$DMG_PATH"

# Step 3: Clean up and report
echo ""
echo "[3/3] Cleaning up..."
rm -rf "$DMG_STAGING"

DMG_SIZE=$(stat -f%z "$DMG_PATH" 2>/dev/null || stat -c%s "$DMG_PATH")
DMG_SIZE_MB=$(echo "scale=2; $DMG_SIZE / 1048576" | bc)

echo ""
echo "=== Build Complete ==="
echo "  DMG: ${DMG_SIZE_MB} MB"
echo "  Output: $DMG_PATH"
