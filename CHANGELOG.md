# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.0.0] - 2026-11-16

First release as **USB Tether**.

### Added
- New menu bar app: a native popover with connection status, live download and upload speed, a 60-second speed graph per direction, session data, connection time, and an IP address row.
- Settings menu with Open at Login and Show Speed in Menu Bar.
- New app icon.
- Installer: one universal disk image (`USB-Tether-1.0.0.dmg`) for Apple Silicon and Intel, holding `USB Tether.pkg` and an uninstaller. libusb is built from pinned, checksummed source and linked in, so Homebrew is not needed.
- Parser tests (`make test`) built with AddressSanitizer and UndefinedBehaviorSanitizer.
- CI on pull requests into `main` and pushes to `main`. Tagged releases are built and published by GitHub Actions with a SHA-256 file.

### Changed
- Renamed to USB Tether. The app installs as `/Applications/USB Tether.app`. The installer removes the old `macOs_tether.app`.
- The GitHub repository moved to `ashahinL/usb-tether`. Old links redirect.
- Auto-connect is on by default.
- A newly opened menu bar app shows the current connection state right away.
- Minimum macOS is now 13 Ventura.
- The service log moved to `/var/log/macostether.log`.

### Security
- RNDIS message lengths and DHCP replies from the USB device are bounds-checked. A large length value could wrap a 32-bit check and make the root service read past its buffer.
- The control socket moved from `/tmp` to `/var/run/macostether.sock` and accepts only root and the logged-in user.
- The service no longer writes root-owned files in `/tmp` (stats file, stop file, DNS backup), which closes symlink attacks on those paths.

## Before 1.0.0

USB Tether is a fork of [android-usb-tether-macos](https://github.com/hknsglm/android-usb-tether-macos) by Hakan Saglam. These are that project's releases that the fork started from.

### Fork base 2.0.0 (2026-03-16)

#### Added
- **Protocol Driver Architecture**: Extensible `proto_driver_t` interface for adding NCM/ECM support
- **macOS Menu Bar App**: Native `NSStatusItem` UI with live speed display and Start/Stop controls
- **IPC System**: Unix domain socket for real-time communication between daemon and UI
- **Config File Support**: INI-style config at `~/.config/macostether/config` with CLI overrides
- **Async TX Pool**: 64 async USB TX transfers for higher upload throughput
- **Structured Logging**: Tagged log system with timestamps and configurable log levels
- **Open at Login**: Menu bar app can register itself as a login item (macOS 13+)
- **Connection Duration**: UI shows elapsed connection time
- **macOS Compatibility Layer**: `compat.c` for version detection and conditional features
- **launchd Plist**: Optional launchd daemon configuration for background operation

#### Changed
- Async RX transfers increased from 1 to 16 in-flight for higher download throughput
- Socket buffers increased to 4MB for better burst handling
- DHCP client improved with proper lease parsing
- Build system supports Universal Binary (arm64 + x86_64) via `make UNIVERSAL=1`
- Swift UI build now auto-detects host architecture

#### Fixed
- Network routes and DNS are now reliably cleaned up on exit (atexit + signal handlers)
- File-based stop trigger prevents daemon from hanging on stop
- Guard route teardown with extra safety route removals

### Fork base 1.0.0 (2026-03-10)

#### Added
- Initial release
- RNDIS protocol implementation for Android USB tethering on macOS
- Async USB RX transfers via libusb
- macOS utun virtual network interface
- DHCP client for automatic IP configuration
- ARP request/reply handling
- DNS configuration via scutil
- Split-tunnel routing (0.0.0.0/1 + 128.0.0.0/1)
- Auto-reconnect on device disconnect
- Command-line options for static IP, no-route, no-dns modes
- MIT License
