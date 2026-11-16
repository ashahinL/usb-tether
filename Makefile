CC = clang
CFLAGS = -Wall -Wextra -O2 -Iinclude
LDFLAGS =

# Version from git (or "dev" if not in a git repo)
VERSION := $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")
CFLAGS += -DVERSION=\"$(VERSION)\"

# macOS Universal Binary Support
ARCH_FLAGS ?= $(shell uname -m | grep -q 'arm64' && echo '-arch arm64' || echo '-arch x86_64')
ifeq ($(UNIVERSAL), 1)
	ARCH_FLAGS = -arch arm64 -arch x86_64
endif

# Minimum macOS version (13 Ventura)
CFLAGS += -mmacosx-version-min=13.0
LDFLAGS += -mmacosx-version-min=13.0
MAC_SDK := $(shell xcrun --show-sdk-path)
ifeq ($(UNIVERSAL), 1)
	SWIFT_FLAGS = -O -sdk $(MAC_SDK) -target arm64-apple-macos13.0 -target x86_64-apple-macos13.0
else
	SWIFT_FLAGS = -O -sdk $(MAC_SDK) -target $(shell uname -m)-apple-macos13.0
endif

# libusb from Homebrew
LIBUSB_PREFIX := $(shell brew --prefix libusb 2>/dev/null || echo /opt/homebrew)
CFLAGS += -I$(LIBUSB_PREFIX)/include/libusb-1.0 $(ARCH_FLAGS)

# Static linking (for distribution): no external dylib dependencies
ifeq ($(STATIC), 1)
LDFLAGS += $(LIBUSB_PREFIX)/lib/libusb-1.0.a $(ARCH_FLAGS)
else
LDFLAGS += -L$(LIBUSB_PREFIX)/lib -lusb-1.0 $(ARCH_FLAGS)
endif

# macOS frameworks (IOKit + objc needed for static libusb)
LDFLAGS += -framework CoreFoundation -framework IOKit -framework Security -lobjc

UI_SRCS = $(wildcard ui/*.swift)

SRCDIR = src
BUILDDIR = build
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

.PHONY: all clean install uninstall version pkg test icon screenshots

all: $(APP)

version:
	@echo "macostether $(VERSION)"

$(BUILDDIR):
	mkdir -p $(BUILDDIR)

$(BUILDDIR)/%.o: $(SRCDIR)/%.c | $(BUILDDIR)
	$(CC) $(CFLAGS) -c $< -o $@

# Build the single self-contained .app bundle
$(APP): $(OBJS) $(UI_SRCS) $(RESDIR)/Info.plist $(RESDIR)/AppIcon.icns | $(BUILDDIR)
	@# Create bundle structure
	mkdir -p $(APP_MACOS) $(APP)/Contents/Resources
	@# Link daemon binary
	$(CC) $(OBJS) $(LDFLAGS) -o $(DAEMON)
	@# Compile Swift UI
	swiftc $(SWIFT_FLAGS) $(UI_SRCS) -o $(UI)
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
	rm -rf $(BUILDDIR)

PLIST_SRC = $(RESDIR)/com.ashahin.macostether.plist
PLIST_DST = /Library/LaunchDaemons/com.ashahin.macostether.plist

install: $(APP)
	@echo "Installing USB Tether to /Applications..."
	@# Stop existing daemon if running
	launchctl unload $(PLIST_DST) 2>/dev/null || true
	rm -rf /Applications/macOs_tether.app "$(INSTALLED_APP)"
	cp -R $(APP) "$(INSTALLED_APP)"
	@echo "Installing LaunchDaemon..."
	cp $(PLIST_SRC) $(PLIST_DST)
	chown root:wheel $(PLIST_DST)
	chmod 644 $(PLIST_DST)
	launchctl load $(PLIST_DST)
	@echo ""
	@echo "Installed. The daemon is now running in watch mode."
	@echo "Open USB Tether from /Applications for the menu bar UI."
	@echo "The daemon runs as a system service, so it does not ask for a password again."

uninstall:
	launchctl unload $(PLIST_DST) 2>/dev/null || true
	rm -f $(PLIST_DST)
	rm -rf "$(INSTALLED_APP)"
	rm -f /var/run/macostether.sock
	@echo "Uninstalled USB Tether and its LaunchDaemon"

pkg:
	@scripts/build-pkg.sh
