CC = clang
CFLAGS = -Wall -Wextra -O2 -Iinclude
LDFLAGS =

# Version from git (or "dev" if not in a git repo)
VERSION := $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")
CFLAGS += -DVERSION=\"$(VERSION)\"

ARCH_FLAGS = -arch $(shell uname -m)
CFLAGS += -mmacosx-version-min=13.0
LDFLAGS += -mmacosx-version-min=13.0
MAC_SDK := $(shell xcrun --show-sdk-path)
CFLAGS += -isysroot $(MAC_SDK)
LDFLAGS += -isysroot $(MAC_SDK)
SWIFT_FLAGS = -O -sdk $(MAC_SDK) -target $(shell uname -m)-apple-macos13.0

ifeq ($(RELEASE),1)
VERSION := $(shell /usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' resources/Info.plist)
ARCH_FLAGS = -arch arm64 -arch x86_64
LIBUSB_PREFIX = $(CURDIR)/build/deps
LDFLAGS += $(LIBUSB_PREFIX)/lib/libusb-1.0.a $(ARCH_FLAGS)
BUILDDIR = build/release
else
LIBUSB_PREFIX := $(shell brew --prefix libusb 2>/dev/null || echo /opt/homebrew)
LDFLAGS += -L$(LIBUSB_PREFIX)/lib -lusb-1.0 $(ARCH_FLAGS)
BUILDDIR = build
endif
CFLAGS += -I$(LIBUSB_PREFIX)/include/libusb-1.0 $(ARCH_FLAGS)
LDFLAGS += -framework CoreFoundation -framework IOKit -framework Security -lobjc

UI_SRCS = $(wildcard ui/*.swift)

SRCDIR = src
RESDIR = resources

SRCS = $(SRCDIR)/main.c $(SRCDIR)/rndis.c $(SRCDIR)/usb_device.c $(SRCDIR)/utun.c \
       $(SRCDIR)/log.c $(SRCDIR)/dhcp.c $(SRCDIR)/arp.c $(SRCDIR)/frame.c \
       $(SRCDIR)/proto_rndis.c $(SRCDIR)/config.c $(SRCDIR)/ipc.c $(SRCDIR)/compat.c
OBJS = $(patsubst $(SRCDIR)/%.c,$(BUILDDIR)/%.o,$(SRCS))

# Everything lives inside one .app bundle
APP = $(BUILDDIR)/USBTether.app
INSTALLED_APP = /Applications/USB Tether.app
APP_MACOS = $(APP)/Contents/MacOS
DAEMON = $(APP_MACOS)/macostether
UI = $(APP_MACOS)/USBTether

# Optional code signing
ifdef SIGN_IDENTITY
CODESIGN_CMD = codesign --force --sign "$(SIGN_IDENTITY)" --entitlements $(RESDIR)/entitlements.plist
else
CODESIGN_CMD = @true
endif

.PHONY: all clean install uninstall version pkg dist test icon screenshots

all: $(APP)

version:
	@echo "macostether $(VERSION)"

$(BUILDDIR):
	mkdir -p $(BUILDDIR)

$(BUILDDIR)/%.o: $(SRCDIR)/%.c Makefile | $(BUILDDIR)
	$(CC) $(CFLAGS) -c $< -o $@

ifeq ($(RELEASE),1)
$(APP): build/deps/lib/libusb-1.0.a
endif

# Build the single self-contained .app bundle
$(APP): $(OBJS) $(UI_SRCS) $(RESDIR)/Info.plist $(RESDIR)/AppIcon.icns | $(BUILDDIR)
	@# Create bundle structure
	mkdir -p $(APP_MACOS) $(APP)/Contents/Resources
	@# Link daemon binary
	$(CC) $(OBJS) $(LDFLAGS) -o $(DAEMON)
	@# Compile Swift UI
ifeq ($(RELEASE),1)
	swiftc -O -sdk $(MAC_SDK) -target arm64-apple-macos13.0 $(UI_SRCS) -o $(BUILDDIR)/USBTether-arm64
	swiftc -O -sdk $(MAC_SDK) -target x86_64-apple-macos13.0 $(UI_SRCS) -o $(BUILDDIR)/USBTether-x86_64
	lipo -create $(BUILDDIR)/USBTether-arm64 $(BUILDDIR)/USBTether-x86_64 -output $(UI)
else
	swiftc $(SWIFT_FLAGS) $(UI_SRCS) -o $(UI)
endif
	@# Copy Info.plist and the app icon
	cp $(RESDIR)/Info.plist $(APP)/Contents/Info.plist
	cp $(RESDIR)/AppIcon.icns $(APP)/Contents/Resources/AppIcon.icns
	@# Optional code signing
	$(CODESIGN_CMD) $(APP)
	@echo ""
	@echo "Build complete: $(APP) ($(VERSION))"
	@echo "  Daemon: $(DAEMON)"
	@echo "  UI:     $(UI)"

# Redraw resources/AppIcon.icns from scripts/make-icon.swift
icon: | $(BUILDDIR)
	rm -rf $(BUILDDIR)/AppIcon.iconset
	swift scripts/make-icon.swift $(BUILDDIR)/AppIcon.iconset
	iconutil -c icns $(BUILDDIR)/AppIcon.iconset -o $(RESDIR)/AppIcon.icns

# Render the README popover screenshots from the real SwiftUI views with sample data
SCREENSHOT_BIN = $(BUILDDIR)/render-screenshots
screenshots: | $(BUILDDIR)
	swiftc $(SWIFT_FLAGS) $(filter-out ui/App.swift,$(UI_SRCS)) scripts/render-screenshots.swift -o $(SCREENSHOT_BIN)
	$(SCREENSHOT_BIN) docs/images

TEST_SRCS = tests/test_parsers.c $(SRCDIR)/rndis.c $(SRCDIR)/dhcp.c $(SRCDIR)/log.c $(SRCDIR)/usb_device.c
TEST_BIN = $(BUILDDIR)/test_parsers

test: | $(BUILDDIR)
	$(CC) $(filter-out -O2,$(CFLAGS)) -g -fsanitize=address,undefined \
		$(TEST_SRCS) $(LDFLAGS) -o $(TEST_BIN)
	$(TEST_BIN)

clean:
	rm -rf build

PLIST_SRC = $(RESDIR)/com.ashahin.macostether.plist
PLIST_DST = /Library/LaunchDaemons/com.ashahin.macostether.plist

install: $(APP)
	@echo "Installing USB Tether to /Applications..."
	@# Stop existing daemon if running
	scripts/preinstall
	rm -rf /Applications/macOs_tether.app "$(INSTALLED_APP)"
	cp -R $(APP) "$(INSTALLED_APP)"
	@echo "Installing LaunchDaemon..."
	cp $(PLIST_SRC) $(PLIST_DST)
	chown root:wheel $(PLIST_DST)
	chmod 644 $(PLIST_DST)
	scripts/postinstall
	@echo ""
	@echo "Installed. The daemon is now running in watch mode."
	@echo "Open USB Tether from /Applications for the menu bar UI."
	@echo "The daemon runs as a system service, so it does not ask for a password again."

uninstall:
	scripts/uninstall.sh

pkg: dist

dist:
	scripts/build-libusb.sh
	$(MAKE) RELEASE=1 all
	scripts/build-pkg.sh
