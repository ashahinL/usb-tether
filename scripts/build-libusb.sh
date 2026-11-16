#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
DEPS="$ROOT_DIR/build/deps"
SDK=$(xcrun --show-sdk-path)
VERSION=1.0.30
SHA256=fea36f34f9156400209595e300840767ab1a385ede1dc7ee893015aea9c6dbaf
if [[ -f "$DEPS/lib/libusb-1.0.a" && -f "$DEPS/include/libusb-1.0/libusb.h" &&
      -f "$DEPS/version" && "$(cat "$DEPS/version")" == "$VERSION" ]]; then
    echo "Using cached universal libusb $VERSION"
    exit 0
fi
WORK="$ROOT_DIR/build/libusb-source"
rm -rf "$WORK" "$DEPS"
mkdir -p "$WORK" "$DEPS/lib" "$DEPS/include/libusb-1.0"
ARCHIVE="$WORK/libusb-$VERSION.tar.bz2"
curl -fL --retry 3 "https://github.com/libusb/libusb/releases/download/v$VERSION/libusb-$VERSION.tar.bz2" -o "$ARCHIVE"
printf '%s  %s\n' "$SHA256" "$ARCHIVE" | shasum -a 256 -c -
tar -xjf "$ARCHIVE" -C "$WORK"
for arch in arm64 x86_64; do
    mkdir -p "$WORK/$arch"
    (
        cd "$WORK/$arch"
        ac_cv_func_pipe2=no CC="$(xcrun -f clang)" CFLAGS="-O2 -isysroot $SDK -arch $arch -mmacosx-version-min=13.0" \
            LDFLAGS="-isysroot $SDK -arch $arch -mmacosx-version-min=13.0" \
            "$WORK/libusb-$VERSION/configure" --host="$arch-apple-darwin" \
            --disable-shared --enable-static --disable-dependency-tracking
        make -j "$(sysctl -n hw.ncpu)"
    )
done
lipo -create "$WORK/arm64/libusb/.libs/libusb-1.0.a" \
    "$WORK/x86_64/libusb/.libs/libusb-1.0.a" -output "$DEPS/lib/libusb-1.0.a"
cp "$WORK/libusb-$VERSION/libusb/libusb.h" "$DEPS/include/libusb-1.0/"
printf '%s\n' "$VERSION" > "$DEPS/version"
