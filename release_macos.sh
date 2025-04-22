#!/bin/bash
set -e

# Configuration
APP_NAME="LibreCard"
BUNDLE_ID="net.ycao.librecard"
RUST_PACKAGE_NAME="librecard"

# Build universal binary
mkdir -p target/universal-apple-darwin

echo "Building for arm64..."
cargo build --release --target aarch64-apple-darwin

echo "Building for x86_64..."
cargo build --release --target x86_64-apple-darwin

echo "Creating universal binary..."
lipo -create \
    "target/aarch64-apple-darwin/release/$RUST_PACKAGE_NAME" \
    "target/x86_64-apple-darwin/release/$RUST_PACKAGE_NAME" \
    -output "target/universal-apple-darwin/$APP_NAME"

chmod +x "target/universal-apple-darwin/$APP_NAME"

# Pack into .app
echo "Creating app bundle..."
APP_BUNDLE="target/$APP_NAME.app"
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"

cp "target/universal-apple-darwin/$APP_NAME" "$APP_BUNDLE/Contents/MacOS/"

cat > "$APP_BUNDLE/Contents/Info.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleVersion</key>
    <string>0.1</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>10.13</string>
</dict>
</plist>
EOF

echo "App bundle created at $APP_BUNDLE"

# Create DMG with README and LICENSE
echo "Creating DMG..."
DMG_NAME="${APP_NAME}-macos_universal-$(git rev-parse --short HEAD).dmg"
DMG_PATH="target/$DMG_NAME"
TMP_DMG_PATH="target/${APP_NAME}_tmp.dmg"
DMG_STAGING="target/dmg_staging"

# Remove existing files if they exist
rm -f "$DMG_PATH" "$TMP_DMG_PATH"
rm -rf "$DMG_STAGING"

# Create staging directory for DMG contents
mkdir -p "$DMG_STAGING"

# Copy app bundle to staging directory
cp -R "$APP_BUNDLE" "$DMG_STAGING/"

# Copy README and LICENSE to staging directory
# Assuming these files exist in your project root
cp README.md "$DMG_STAGING/README.txt"  # Assume user don't have markdown reader
cp README_MACOS.txt "$DMG_STAGING/README_MACOS.txt"
cp LICENSE "$DMG_STAGING/LICENSE.txt"

# Create a temporary DMG from the staging directory
hdiutil create -volname "$APP_NAME" -srcfolder "$DMG_STAGING" -ov -format UDRW "$TMP_DMG_PATH"
# Convert the temporary DMG to the final compressed DMG
hdiutil convert "$TMP_DMG_PATH" -format UDZO -o "$DMG_PATH"

# Clean up
rm -f "$TMP_DMG_PATH"
rm -rf "$DMG_STAGING"

echo "DMG packed at $DMG_PATH"

