#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' resources/Info.plist)
APP='build/pkg-root/Applications/USB Tether.app'
for binary in macostether USBTether; do
    archs=$(lipo -archs "$APP/Contents/MacOS/$binary")
    [[ "$archs" == 'arm64 x86_64' || "$archs" == 'x86_64 arm64' ]]
    echo "$binary: $archs"
done
links=$(otool -L "$APP/Contents/MacOS/macostether")
printf '%s\n' "$links"
if printf '%s\n' "$links" | grep -E '/opt/homebrew|/usr/local|libusb.*dylib'; then
    exit 1
fi
WORK=$(mktemp -d)
MOUNT="$WORK/mount"
ATTACHED=0
cleanup() {
    if [[ $ATTACHED -eq 1 ]]; then hdiutil detach "$MOUNT"; fi
    rm -rf "$WORK"
}
trap cleanup EXIT
pkgutil --expand 'build/USB Tether.pkg' "$WORK/pkg"
PAYLOAD=$(pkgutil --payload-files 'build/USB Tether.pkg')
printf '%s\n' "$PAYLOAD"
printf '%s\n' "$PAYLOAD" | grep -F './Applications/USB Tether.app/Contents/MacOS/macostether'
printf '%s\n' "$PAYLOAD" | grep -F './Applications/USB Tether.app/Contents/MacOS/USBTether'
printf '%s\n' "$PAYLOAD" | grep -F './Library/LaunchDaemons/com.ashahin.macostether.plist'
grep -F '<title>USB Tether</title>' "$WORK/pkg/Distribution"
grep -E 'os-version min="13.0"' "$WORK/pkg/Distribution"
grep -F 'hostArchitectures="arm64,x86_64"' "$WORK/pkg/Distribution"
grep -F 'relocatable="false"' "$WORK/pkg/component.pkg/PackageInfo"
mkdir "$MOUNT"
hdiutil attach -nobrowse -readonly -mountpoint "$MOUNT" "build/USB-Tether-$VERSION.dmg"
ATTACHED=1
ls -l "$MOUNT"
test -f "$MOUNT/USB Tether.pkg"
test -f "$MOUNT/README.txt"
test -x "$MOUNT/Uninstall USB Tether.command"
cmp scripts/uninstall.sh "$MOUNT/Uninstall USB Tether.command"
hdiutil detach "$MOUNT"
ATTACHED=0
(cd build && shasum -a 256 -c "USB-Tether-$VERSION.dmg.sha256")
for script in scripts/build-libusb.sh scripts/build-pkg.sh scripts/preinstall scripts/postinstall scripts/uninstall.sh tests/test_dist.sh tests/test_release.sh; do
    bash -n "$script"
    if command -v shellcheck >/dev/null; then shellcheck "$script"; fi
    echo "$script: shell checks OK"
done
for workflow in .github/workflows/ci.yml .github/workflows/release.yml; do
    ruby -ryaml -e 'YAML.load_file(ARGV[0])' "$workflow"
    echo "$workflow: YAML OK"
done
echo 'Release artifact checks passed.'
