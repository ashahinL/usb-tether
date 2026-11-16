#!/bin/bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT_DIR/build"
VERSION=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$ROOT_DIR/resources/Info.plist")
APP="$BUILD/release/USBTether.app"
STAGE="$BUILD/pkg-root"
PKG_WORK="$BUILD/pkg-work"
DMG_ROOT="$BUILD/dmg-root"
if otool -L "$APP/Contents/MacOS/macostether" | grep -E 'libusb.*dylib|/opt/homebrew|/usr/local'; then
    echo 'Release daemon has an external library dependency.' >&2
    exit 1
fi
rm -rf "$STAGE" "$PKG_WORK" "$DMG_ROOT"
mkdir -p "$STAGE/Applications" "$STAGE/Library/LaunchDaemons" "$PKG_WORK/scripts" "$DMG_ROOT"
ditto "$APP" "$STAGE/Applications/USB Tether.app"
cp "$ROOT_DIR/resources/com.ashahin.macostether.plist" "$STAGE/Library/LaunchDaemons/"
cp "$ROOT_DIR/scripts/preinstall" "$ROOT_DIR/scripts/postinstall" "$PKG_WORK/scripts/"
chmod 755 "$PKG_WORK/scripts/"*
pkgbuild --analyze --root "$STAGE" "$PKG_WORK/component.plist"
/usr/libexec/PlistBuddy -c 'Set :0:BundleIsRelocatable false' "$PKG_WORK/component.plist"
pkgbuild --root "$STAGE" --scripts "$PKG_WORK/scripts" \
    --identifier com.ashahin.macostether --version "$VERSION" --install-location / \
    --component-plist "$PKG_WORK/component.plist" "$PKG_WORK/component.pkg"
cat > "$PKG_WORK/distribution.xml" <<XML
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <title>USB Tether</title>
    <welcome file="welcome.html"/>
    <conclusion file="conclusion.html"/>
    <options customize="never" require-scripts="false" hostArchitectures="arm64,x86_64"/>
    <allowed-os-versions><os-version min="13.0"/></allowed-os-versions>
    <choices-outline><line choice="default"/></choices-outline>
    <choice id="default" visible="false"><pkg-ref id="com.ashahin.macostether"/></choice>
    <pkg-ref id="com.ashahin.macostether" version="$VERSION" onConclusion="none">component.pkg</pkg-ref>
</installer-gui-script>
XML
set --
if [[ -n "${INSTALLER_SIGN_IDENTITY:-}" ]]; then
    set -- --sign "$INSTALLER_SIGN_IDENTITY"
fi
productbuild --distribution "$PKG_WORK/distribution.xml" --package-path "$PKG_WORK" \
    --resources "$ROOT_DIR/resources/installer" "$@" "$BUILD/USB Tether.pkg"
cp "$BUILD/USB Tether.pkg" "$DMG_ROOT/"
cp "$ROOT_DIR/scripts/uninstall.sh" "$DMG_ROOT/Uninstall USB Tether.command"
chmod 755 "$DMG_ROOT/Uninstall USB Tether.command"
cat > "$DMG_ROOT/README.txt" <<TXT
Install
Open USB Tether.pkg and follow the installer. Requires macOS 13 or later.

First run if macOS blocks the installer
Right-click > Open, or System Settings > Privacy & Security > Open Anyway.
Then open USB Tether from Applications and enable USB tethering on your phone.

Uninstall
Run Uninstall USB Tether.command and enter your admin password.
TXT
DMG="USB-Tether-$VERSION.dmg"
hdiutil create -ov -format UDZO -volname "USB Tether $VERSION" -srcfolder "$DMG_ROOT" "$BUILD/$DMG"
(cd "$BUILD" && shasum -a 256 "$DMG" > "$DMG.sha256")
echo "Built build/$DMG and build/$DMG.sha256"
